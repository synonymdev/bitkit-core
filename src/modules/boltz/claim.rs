use crate::modules::boltz::client::{build_boltz_client, build_chain_client};
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::guard::lock_swap;
use crate::modules::boltz::models::{BoltzDB, SwapRecord};
use crate::modules::boltz::validation::validate_fee_rate;
use boltz_client::swaps::{SwapScript, SwapTransactionParams, TransactionOptions};
use boltz_client::util::fees::Fee;

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
    if let Some(existing) = record.claim_tx_id {
        return Ok(ClaimOutcome::AlreadyClaimed(existing));
    }

    let txid = claim_reverse_swap(&record, mnemonic, bip39_passphrase, fee_rate_sat_per_vb).await?;
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

    let chain = record.network.as_chain();
    let swap_script = SwapScript::reverse_from_swap_resp(chain, &reverse_resp, our_pubkey)?;
    let chain_client = build_chain_client(record.network, &record.electrum_url)?;
    let boltz_client = build_boltz_client(record.network);
    let fee = Fee::Relative(fee_rate_sat_per_vb.unwrap_or(DEFAULT_FEERATE_SAT_PER_VB));

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
