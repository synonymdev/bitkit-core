//! Trezor-specific account info helpers.
//!
//! Functions that bridge generic account types to Trezor's signing protocol.

use std::collections::HashSet;
use std::str::FromStr;

use bdk::bitcoin::consensus::deserialize;
use bdk::bitcoin::{Transaction, Txid};
use bdk::electrum_client::ElectrumApi;

use crate::modules::onchain::{AccountInfoError, AccountType};
use super::types::{
    TrezorPrevTx, TrezorPrevTxInput, TrezorPrevTxOutput, TrezorScriptType,
};

/// Map AccountType to Trezor's ScriptType for transaction inputs.
pub fn account_type_to_script_type(account_type: AccountType) -> TrezorScriptType {
    match account_type {
        AccountType::Legacy => TrezorScriptType::SpendAddress,
        AccountType::WrappedSegwit => TrezorScriptType::SpendP2shWitness,
        AccountType::NativeSegwit => TrezorScriptType::SpendWitness,
        AccountType::Taproot => TrezorScriptType::SpendTaproot,
    }
}

/// Fetch previous transactions from Electrum and convert to TrezorPrevTx format.
///
/// Takes a list of transaction ID hex strings, fetches the raw transactions
/// from an Electrum server, and returns them as `TrezorPrevTx` structures
/// suitable for inclusion in `TrezorSignTxParams.prev_txs`.
///
/// Duplicate txids are automatically deduplicated.
pub async fn fetch_prev_txs(
    txids: Vec<String>,
    electrum_url: &str,
) -> Result<Vec<TrezorPrevTx>, AccountInfoError> {
    // Deduplicate txids
    let unique_txids: Vec<String> = txids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Parse all txid strings upfront, fail fast on bad input
    let parsed_txids: Vec<(String, Txid)> = unique_txids
        .into_iter()
        .map(|txid_str| {
            let txid = Txid::from_str(&txid_str).map_err(|e| {
                AccountInfoError::InvalidTxid {
                    error_details: format!("Invalid txid '{}': {}", txid_str, e),
                }
            })?;
            Ok((txid_str, txid))
        })
        .collect::<Result<Vec<_>, AccountInfoError>>()?;

    let electrum_url_owned = electrum_url.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;

        let mut prev_txs: Vec<TrezorPrevTx> = Vec::with_capacity(parsed_txids.len());

        for (txid_hex, txid) in &parsed_txids {
            let tx_bytes = client.transaction_get_raw(txid).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to fetch tx {}: {}", txid_hex, e),
                }
            })?;

            let tx: Transaction = deserialize(&tx_bytes).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to deserialize tx {}: {}", txid_hex, e),
                }
            })?;

            prev_txs.push(transaction_to_prev_tx(&tx));
        }

        Ok::<_, AccountInfoError>(prev_txs)
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Task failed: {}", e),
    })??;

    Ok(result)
}

/// Convert a bitcoin::Transaction to TrezorPrevTx.
pub(crate) fn transaction_to_prev_tx(tx: &Transaction) -> TrezorPrevTx {
    let inputs = tx
        .input
        .iter()
        .map(|input| TrezorPrevTxInput {
            prev_hash: input.previous_output.txid.to_string(),
            prev_index: input.previous_output.vout,
            script_sig: hex::encode(input.script_sig.as_bytes()),
            sequence: input.sequence.0,
        })
        .collect();

    let outputs = tx
        .output
        .iter()
        .map(|output| TrezorPrevTxOutput {
            amount: output.value,
            script_pubkey: hex::encode(output.script_pubkey.as_bytes()),
        })
        .collect();

    TrezorPrevTx {
        hash: tx.txid().to_string(),
        version: tx.version as u32,
        lock_time: tx.lock_time.to_consensus_u32(),
        inputs,
        outputs,
    }
}
