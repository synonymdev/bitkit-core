//! Compose transaction functions for Trezor.
//!
//! Provides offline coin selection and fee calculation, bridging
//! bitkit-core's account types to trezor-connect-rs's compose engine.

use super::errors::TrezorError;
use super::types::{
    TrezorCoinType, TrezorPrecomposeParams, TrezorPrecomposedInput,
    TrezorPrecomposedOutput, TrezorPrecomposedResult,
    TrezorSignTxParams, TrezorTxInput, TrezorTxOutput,
};

/// Compose a transaction offline for multiple fee levels.
///
/// Takes account data (UTXOs, addresses) and desired outputs, runs coin
/// selection and fee calculation for each fee level, and returns results.
/// No device interaction needed — pure computation.
pub fn precompose_transaction(params: TrezorPrecomposeParams) -> Vec<TrezorPrecomposedResult> {
    let tc_params: trezor_connect_rs::api::compose::PrecomposeParams = params.into();
    let results = trezor_connect_rs::api::compose::precompose(tc_params);
    results.into_iter().map(|r| r.into()).collect()
}

/// Convert precomposed inputs and outputs into TrezorSignTxParams for device signing.
///
/// The returned `TrezorSignTxParams` has empty `prev_txs` — the caller must
/// provide previous transaction data for non-SegWit inputs before calling
/// `trezor_sign_tx()`.
pub fn precomposed_to_sign_params(
    inputs: Vec<TrezorPrecomposedInput>,
    outputs: Vec<TrezorPrecomposedOutput>,
    coin: Option<TrezorCoinType>,
) -> Result<TrezorSignTxParams, TrezorError> {
    let sign_inputs: Vec<TrezorTxInput> = inputs
        .into_iter()
        .map(|input| -> Result<TrezorTxInput, TrezorError> {
            Ok(TrezorTxInput {
                prev_hash: input.txid,
                prev_index: input.vout,
                path: input.path,
                amount: input.amount.parse::<u64>().map_err(|e| TrezorError::DeviceError {
                    error_details: format!("Invalid input amount '{}': {}", input.amount, e),
                })?,
                script_type: input.script_type,
                sequence: None,
                orig_hash: None,
                orig_index: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let sign_outputs: Vec<TrezorTxOutput> = outputs
        .into_iter()
        .map(|output| -> Result<TrezorTxOutput, TrezorError> {
            Ok(match output {
                TrezorPrecomposedOutput::Payment { address, amount } => TrezorTxOutput {
                    address: Some(address),
                    path: None,
                    amount: amount.parse::<u64>().map_err(|e| TrezorError::DeviceError {
                        error_details: format!("Invalid output amount '{}': {}", amount, e),
                    })?,
                    script_type: None,
                    op_return_data: None,
                    orig_hash: None,
                    orig_index: None,
                },
                TrezorPrecomposedOutput::Change {
                    path,
                    amount,
                    script_type,
                    ..
                } => TrezorTxOutput {
                    address: None,
                    path: Some(path),
                    amount: amount.parse::<u64>().map_err(|e| TrezorError::DeviceError {
                        error_details: format!("Invalid change amount '{}': {}", amount, e),
                    })?,
                    script_type: Some(script_type),
                    op_return_data: None,
                    orig_hash: None,
                    orig_index: None,
                },
                TrezorPrecomposedOutput::OpReturn { data_hex } => TrezorTxOutput {
                    address: None,
                    path: None,
                    amount: 0,
                    script_type: None,
                    op_return_data: Some(data_hex),
                    orig_hash: None,
                    orig_index: None,
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TrezorSignTxParams {
        inputs: sign_inputs,
        outputs: sign_outputs,
        coin,
        lock_time: None,
        version: None,
        prev_txs: vec![],
    })
}
