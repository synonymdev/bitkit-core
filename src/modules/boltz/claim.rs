use crate::modules::boltz::client::{build_boltz_client, build_chain_client};
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::guard::lock_swap;
use crate::modules::boltz::models::{BoltzDB, SwapRecord};
use crate::modules::boltz::validation::validate_fee_rate;
use boltz_client::swaps::bitcoin::{BtcSwapScript, BtcSwapTx};
use boltz_client::swaps::boltz::SwapTxKind;
use boltz_client::swaps::{SwapScript, SwapTransactionParams, TransactionOptions};

/// Default claim fee rate in sat/vByte used when the caller doesn't specify one.
pub(crate) const DEFAULT_FEERATE_SAT_PER_VB: f64 = 2.0;

/// Result of a guarded claim attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// A claim transaction was built and broadcast by this call.
    Broadcast(String),
    /// The swap already had a claim txid recorded, so nothing was broadcast.
    AlreadyClaimed(String),
}

impl ClaimOutcome {
    /// The claim transaction id, however it was arrived at.
    pub fn txid(self) -> String {
        match self {
            ClaimOutcome::Broadcast(txid) | ClaimOutcome::AlreadyClaimed(txid) => txid,
        }
    }
}

/// Claim a reverse swap, serialized against any other claim of the same swap.
///
/// This is the only path that should broadcast a claim. It holds the swap's lock
/// across the whole read-broadcast-record sequence, so the automatic claim from
/// the updates stream and a manual recovery call cannot both broadcast: whichever
/// arrives second re-reads the swap under the lock, finds the first one's txid,
/// and returns it as [`ClaimOutcome::AlreadyClaimed`].
pub async fn claim_reverse_swap_guarded(
    db: &BoltzDB,
    swap_id: &str,
    mnemonic: &str,
    bip39_passphrase: Option<&str>,
    fee_rate_sat_per_vb: Option<f64>,
) -> Result<ClaimOutcome, BoltzError> {
    let _operation = db.recovery_gate.read().await;
    validate_fee_rate(fee_rate_sat_per_vb)?;
    let _guard = lock_swap(swap_id).await;

    // Re-read under the lock. The record the caller checked may be stale: a
    // concurrent claim can have completed while we waited to acquire.
    let record = db
        .get_swap(swap_id)
        .await?
        .ok_or_else(|| BoltzError::NotFound {
            error_details: format!("Swap {} not found", swap_id),
        })?;
    if record.swap_type != super::types::BoltzSwapType::Reverse {
        return Err(BoltzError::InvalidInput {
            error_details: "Swap direction does not support this claim".into(),
        });
    }
    if let Some(existing) = record.claim_tx_id {
        return Ok(ClaimOutcome::AlreadyClaimed(existing));
    }

    if let Some(txid) = recover_broadcast(db, &record).await? {
        return Ok(ClaimOutcome::Broadcast(txid));
    }
    let txid =
        claim_reverse_swap(db, &record, mnemonic, bip39_passphrase, fee_rate_sat_per_vb).await?;
    db.set_claim_tx(swap_id, &txid).await?;
    Ok(ClaimOutcome::Broadcast(txid))
}

/// Claim a reverse swap's onchain funds to the address captured at creation,
/// revealing the preimage so Boltz can settle the Lightning invoice.
///
/// A cooperative (key-path) claim is attempted first for a smaller, cheaper
/// transaction; if Boltz declines to cooperate it falls back to the script-path
/// spend, which is always available while the lockup is unspent. Returns the
/// broadcast claim transaction id.
pub async fn claim_reverse_swap(
    db: &BoltzDB,
    record: &SwapRecord,
    mnemonic: &str,
    bip39_passphrase: Option<&str>,
    fee_rate_sat_per_vb: Option<f64>,
) -> Result<String, BoltzError> {
    let reverse_resp = record.reverse_response()?;
    let keypair = record.keypair(mnemonic, bip39_passphrase)?;
    let preimage = record.preimage(mnemonic, bip39_passphrase)?;
    let our_pubkey = bitcoin::PublicKey::new(keypair.public_key());

    let claim_address = record
        .onchain_address
        .clone()
        .ok_or_else(|| BoltzError::InvalidInput {
            error_details: "Reverse swap is missing a claim address".to_string(),
        })?;

    if record.backend_binding.is_some() {
        super::validation::validate_reverse_response(
            &reverse_resp,
            &preimage,
            &our_pubkey,
            record.amount_sat,
            record.network,
        )?;
        let transaction = pubky_spend_transaction(record, &keypair, &claim_address, false).await?;
        let fee = super::send::claim_fee(record, fee_rate_sat_per_vb)?;
        let signed = transaction
            .sign_claim(&keypair, &preimage, fee, None)
            .await?;
        super::send::validate_claim_output(record, &signed)?;
        return broadcast_journaled(db, record, signed, None).await;
    }

    let chain = record.network.as_chain();
    let swap_script = SwapScript::reverse_from_swap_resp(chain, &reverse_resp, our_pubkey)?;
    let chain_client = build_chain_client(record.network, &record.electrum_url)?;
    let boltz_client = build_boltz_client(record.network);
    let fee = super::send::claim_fee(record, fee_rate_sat_per_vb)?;

    let make_params = |cooperative: bool| SwapTransactionParams {
        keys: keypair,
        output_address: claim_address.clone(),
        fee,
        swap_id: record.id.clone(),
        chain_client: &chain_client,
        boltz_client: &boltz_client,
        options: Some(TransactionOptions::default().with_cooperative(cooperative)),
    };

    // Try the cooperative key-path claim first; fall back to the script path.
    let tx = match swap_script
        .construct_claim(&preimage, make_params(true))
        .await
    {
        Ok(tx) => tx,
        Err(coop_err) => swap_script
            .construct_claim(&preimage, make_params(false))
            .await
            .map_err(|script_err| BoltzError::SwapError {
                error_details: format!(
                    "Claim failed (cooperative: {}; script-path: {})",
                    coop_err, script_err
                ),
            })?,
    };

    chain_client
        .broadcast_tx(&tx)
        .await
        .map_err(|e| BoltzError::BroadcastError {
            error_details: format!("Failed to broadcast claim transaction: {}", e),
        })
}

/// Build a spend from the independently verified accepted outpoint. The SDK's
/// high-level constructor falls back to Boltz HTTP on chain lookup failures,
/// which is inappropriate for a Pubky swap and can select unrelated dust.
pub(crate) async fn pubky_spend_transaction(
    record: &SwapRecord,
    keys: &boltz_client::Keypair,
    destination: &str,
    refund: bool,
) -> Result<BtcSwapTx, BoltzError> {
    let bridge = record.pubky_bridge().await?;
    let id = uuid::Uuid::parse_str(&record.id).map_err(|e| BoltzError::InvalidInput {
        error_details: e.to_string(),
    })?;
    let (utxos, timeout) = if refund {
        let info = bridge
            .refund_info(id)
            .await
            .map_err(super::api::bridge_error)?;
        validate_spend_window(true, 0, 0, info.tip, info.timeout_block_height)?;
        (info.utxos, info.timeout_block_height)
    } else {
        let info = bridge
            .spend_info(id)
            .await
            .map_err(super::api::bridge_error)?;
        validate_spend_window(
            false,
            info.confirmations,
            info.required_confirmations,
            info.tip,
            info.timeout_block_height,
        )?;
        (
            vec![(info.outpoint, info.output)],
            info.timeout_block_height,
        )
    };
    if u64::from(timeout) != record.timeout_block_height {
        return Err(BoltzError::SwapError {
            error_details: "Recovery timeout does not match the accepted swap".into(),
        });
    }
    pubky_transaction_from_outputs(record, keys, destination, refund, utxos)
}

pub(crate) fn pubky_transaction_from_outputs(
    record: &SwapRecord,
    keys: &boltz_client::Keypair,
    destination: &str,
    refund: bool,
    utxos: Vec<(bitcoin::OutPoint, bitcoin::TxOut)>,
) -> Result<BtcSwapTx, BoltzError> {
    let key = bitcoin::PublicKey::new(keys.public_key());
    let script = if refund {
        BtcSwapScript::submarine_from_swap_resp(&record.submarine_response()?, key)?
    } else {
        BtcSwapScript::reverse_from_swap_resp(&record.reverse_response()?, key)?
    };
    if utxos.is_empty() || (!refund && utxos.len() != 1) {
        return Err(BoltzError::SwapError {
            error_details: "No unique claim output or refundable outputs found".into(),
        });
    }
    let expected_script = script
        .to_address(record.network.as_bitcoin_chain())?
        .script_pubkey();
    let mut observed = std::collections::HashSet::new();
    let mut total = 0_u64;
    for (outpoint, output) in &utxos {
        if output.script_pubkey != expected_script || !observed.insert(*outpoint) {
            return Err(BoltzError::SwapError {
                error_details: "Recovery outputs do not match the accepted script".into(),
            });
        }
        total = total
            .checked_add(output.value.to_sat())
            .ok_or_else(|| BoltzError::SwapError {
                error_details: "Recovery output value exceeds the supported amount".into(),
            })?;
        if !refund {
            let expected = record
                .onchain_amount_sat
                .ok_or_else(|| BoltzError::SwapError {
                    error_details: "Missing reverse lockup amount".into(),
                })?;
            if output.value.to_sat() < expected
                || output.value.to_sat() > expected.saturating_add(10_000)
            {
                return Err(BoltzError::SwapError {
                    error_details: "Recovery outpoint does not match the accepted amount".into(),
                });
            }
        }
    }
    let address = destination
        .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
        .map_err(|e| BoltzError::InvalidInput {
            error_details: e.to_string(),
        })?
        .require_network(record.network.as_bitcoin_network())
        .map_err(|e| BoltzError::InvalidInput {
            error_details: e.to_string(),
        })?;
    Ok(BtcSwapTx {
        kind: if refund {
            SwapTxKind::Refund
        } else {
            SwapTxKind::Claim
        },
        swap_script: script,
        output_address: address,
        utxos,
    })
}

pub(crate) fn validate_spend_window(
    refund: bool,
    confirmations: u32,
    required: u32,
    tip: u32,
    timeout: u32,
) -> Result<(), BoltzError> {
    let allowed = if refund {
        tip >= timeout
    } else {
        confirmations >= required.max(1)
            && tip.checked_add(18).is_some_and(|height| height <= timeout)
    };
    if !allowed {
        return Err(BoltzError::SwapError {
            error_details: if refund {
                "The refund timelock has not matured"
            } else {
                "The reverse lockup needs confirmed funds and sufficient time to claim"
            }
            .into(),
        });
    }
    Ok(())
}

pub(crate) async fn broadcast_pubky(
    record: &SwapRecord,
    transaction: bitcoin::Transaction,
) -> Result<String, BoltzError> {
    let binding = record
        .backend_binding
        .as_deref()
        .ok_or_else(|| BoltzError::InvalidInput {
            error_details: "Pubky broadcast requires the original provider binding".into(),
        })?;
    let chain = super::pubky::chain_for_binding(record.network, binding).await?;
    chain
        .broadcast(transaction)
        .await
        .map(|txid| txid.to_string())
        .map_err(super::api::bridge_error)
}

pub(crate) async fn broadcast_journaled(
    db: &BoltzDB,
    record: &SwapRecord,
    transaction: bitcoin::Transaction,
    refund_address: Option<&str>,
) -> Result<String, BoltzError> {
    let expected = transaction.compute_txid().to_string();
    db.journal_spend(&record.id, &expected, refund_address)
        .await?;
    let actual = broadcast_pubky(record, transaction).await?;
    if actual != expected {
        return Err(BoltzError::BroadcastError {
            error_details: "Broadcast returned a different transaction id".into(),
        });
    }
    Ok(actual)
}

/// Heal a crash or lost broadcast response using the exact transaction id that
/// was committed before broadcast. No signing material is needed for this check.
pub(crate) async fn recover_broadcast(
    db: &BoltzDB,
    record: &SwapRecord,
) -> Result<Option<String>, BoltzError> {
    let _operation = db.recovery_gate.read().await;
    if db.pending_spends(&record.id).await?.is_empty() {
        return Ok(None);
    }
    let binding = record
        .backend_binding
        .as_deref()
        .ok_or_else(|| BoltzError::InvalidInput {
            error_details: "Pubky recovery requires the original provider binding".into(),
        })?;
    let chain = super::pubky::chain_for_binding(record.network, binding).await?;
    recover_broadcast_on_chain(db, record, chain.as_ref()).await
}

pub(crate) async fn recover_broadcast_on_chain(
    db: &BoltzDB,
    record: &SwapRecord,
    chain: &dyn pubky_swap_boltz::chain::Chain,
) -> Result<Option<String>, BoltzError> {
    // Preserve earlier attempts: an older transaction can confirm after a replacement.
    for (expected, refund_address) in db.pending_spends(&record.id).await? {
        let id = expected
            .parse::<bitcoin::Txid>()
            .map_err(|e| BoltzError::DatabaseError {
                error_details: e.to_string(),
            })?;
        let Ok(info) = chain.transaction(id).await else {
            continue;
        };
        let transaction: bitcoin::Transaction =
            bitcoin::consensus::deserialize(&hex::decode(&info.hex).map_err(|e| {
                BoltzError::SwapError {
                    error_details: e.to_string(),
                }
            })?)
            .map_err(|e| BoltzError::SwapError {
                error_details: e.to_string(),
            })?;
        if transaction.compute_txid() != id || info.id != expected {
            return Err(BoltzError::SwapError {
                error_details: "Chain returned a different recovery transaction".into(),
            });
        }
        if let Some(address) = refund_address {
            db.set_refund_tx(&record.id, &expected, &address).await?;
        } else {
            db.set_claim_tx(&record.id, &expected).await?;
        }
        return Ok(Some(expected));
    }
    Ok(None)
}
