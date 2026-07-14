use crate::modules::boltz::claim::DEFAULT_FEERATE_SAT_PER_VB;
use crate::modules::boltz::client::{build_boltz_client, build_chain_client};
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::guard::lock_swap;
use crate::modules::boltz::models::{BoltzDB, SwapRecord};
use boltz_client::swaps::{SwapScript, SwapTransactionParams, TransactionOptions};
use boltz_client::util::fees::Fee;

/// Refund a submarine swap, serialized against any other refund of the same swap.
///
/// The counterpart to [`crate::modules::boltz::claim::claim_reverse_swap_guarded`]:
/// it holds the swap's lock across the read-broadcast-record sequence so two
/// concurrent recovery calls cannot both broadcast a refund. If the swap already
/// has a refund txid recorded, that txid is returned without re-broadcasting.
pub async fn refund_submarine_swap_guarded(
    db: &BoltzDB,
    swap_id: &str,
    refund_address: String,
    mnemonic: &str,
    bip39_passphrase: Option<&str>,
    fee_rate_sat_per_vb: Option<f64>,
) -> Result<String, BoltzError> {
    let _guard = lock_swap(swap_id).await;

    // Re-read under the lock: a concurrent refund may have completed while we waited.
    let record = db
        .get_swap(swap_id)
        .await?
        .ok_or_else(|| BoltzError::NotFound {
            error_details: format!("Swap {} not found", swap_id),
        })?;
    if let Some(existing) = record.refund_tx_id {
        return Ok(existing);
    }

    let txid = refund_submarine_swap(
        &record,
        refund_address,
        mnemonic,
        bip39_passphrase,
        fee_rate_sat_per_vb,
    )
    .await?;
    db.set_refund_tx(swap_id, &txid).await?;
    Ok(txid)
}

/// Refund a submarine swap's locked onchain funds back to `refund_address`.
///
/// Used when Boltz fails to pay the invoice (`invoice.failedToPay`), the wrong
/// amount was locked (`transaction.lockupFailed`), or the swap expired. A
/// cooperative refund is attempted first; if unavailable it falls back to the
/// script-path refund, which becomes spendable after the swap's onchain
/// timeout. Returns the broadcast refund transaction id.
pub async fn refund_submarine_swap(
    record: &SwapRecord,
    refund_address: String,
    mnemonic: &str,
    bip39_passphrase: Option<&str>,
    fee_rate_sat_per_vb: Option<f64>,
) -> Result<String, BoltzError> {
    let submarine_resp = record.submarine_response()?;
    let keypair = record.keypair(mnemonic, bip39_passphrase)?;
    let our_pubkey = bitcoin::PublicKey::new(keypair.public_key());

    let chain = record.network.as_chain();
    let swap_script = SwapScript::submarine_from_swap_resp(chain, &submarine_resp, our_pubkey)?;
    let chain_client = build_chain_client(record.network, &record.electrum_url)?;
    let boltz_client = build_boltz_client(record.network);
    let fee = Fee::Relative(fee_rate_sat_per_vb.unwrap_or(DEFAULT_FEERATE_SAT_PER_VB));

    let make_params = |cooperative: bool| SwapTransactionParams {
        keys: keypair,
        output_address: refund_address.clone(),
        fee,
        swap_id: record.id.clone(),
        chain_client: &chain_client,
        boltz_client: &boltz_client,
        options: Some(TransactionOptions::default().with_cooperative(cooperative)),
    };

    // Prefer a cooperative refund; fall back to the timeout script path.
    let tx = match swap_script.construct_refund(make_params(true)).await {
        Ok(tx) => tx,
        Err(coop_err) => swap_script
            .construct_refund(make_params(false))
            .await
            .map_err(|script_err| BoltzError::SwapError {
                error_details: format!(
                    "Refund failed (cooperative: {}; script-path: {})",
                    coop_err, script_err
                ),
            })?,
    };

    chain_client
        .broadcast_tx(&tx)
        .await
        .map_err(|e| BoltzError::BroadcastError {
            error_details: format!("Failed to broadcast refund transaction: {}", e),
        })
}
