//! Generic transaction composition using BDK's TxBuilder.
//!
//! Produces signer-agnostic PSBTs that can be signed by any
//! PSBT-compatible hardware or software wallet (Trezor, Ledger, etc.).

use base64::{engine::general_purpose, Engine as _};
use bdk::bitcoin::address::NetworkUnchecked;
use bdk::bitcoin::{Address as BdkAddress, Network as BdkNetwork};
use bdk::database::MemoryDatabase;
use bdk::wallet::coin_selection::{
    CoinSelectionAlgorithm, LargestFirstCoinSelection, OldestFirstCoinSelection,
};
use bdk::wallet::Wallet;
use bdk::FeeRate;

use super::errors::AccountInfoError;
use super::implementation::{
    connect_electrum, create_and_sync_wallet, resolve_wallet_setup, run_account_info_blocking,
};
use super::types::{CoinSelection, ComposeOutput, ComposeParams, ComposeResult};

/// Compose transactions for multiple fee rates, returning one PSBT per rate.
///
/// Creates a BDK wallet from the extended key, syncs via Electrum, then
/// builds a PSBT for each requested fee rate. The PSBTs include BIP32
/// derivation paths (when `fingerprint` is provided) and are ready for
/// signing by any PSBT-aware signer without additional network calls.
pub async fn compose_transaction(params: ComposeParams) -> Vec<ComposeResult> {
    let num_rates = params.fee_rates.len();
    if num_rates == 0 {
        return vec![];
    }
    match compose_inner(params).await {
        Ok(results) => results,
        Err(e) => {
            let err = ComposeResult::Error {
                error: e.to_string(),
            };
            vec![err; num_rates]
        }
    }
}

async fn compose_inner(params: ComposeParams) -> Result<Vec<ComposeResult>, AccountInfoError> {
    let setup = resolve_wallet_setup(
        &params.wallet.extended_key,
        params.wallet.network,
        params.wallet.account_type,
        params.wallet.fingerprint.as_deref(),
    )?;

    validate_outputs(&params.outputs, setup.network)
        .map_err(|e| AccountInfoError::WalletError { error_details: e })?;

    for rate in &params.fee_rates {
        if !rate.is_finite() || *rate <= 0.0 {
            return Err(AccountInfoError::WalletError {
                error_details: format!("Fee rate must be a positive number, got {}", rate),
            });
        }
    }

    let electrum_url = params.wallet.electrum_url;
    let outputs = params.outputs;
    let fee_rates = params.fee_rates;
    let coin_selection = params.coin_selection;

    run_account_info_blocking("compose transaction", move || {
        let client = connect_electrum(&electrum_url)?;
        let mut wallet = create_and_sync_wallet(&setup, client)?;

        let mut results = Vec::with_capacity(fee_rates.len());
        for rate in &fee_rates {
            let result = match build_psbt(
                &mut wallet,
                &outputs,
                *rate,
                setup.network,
                coin_selection.as_ref(),
            ) {
                Ok(r) => r,
                Err(msg) => ComposeResult::Error { error: msg },
            };
            results.push(result);
        }
        Ok(results)
    })
    .await
}

fn build_psbt(
    wallet: &mut Wallet<MemoryDatabase>,
    outputs: &[ComposeOutput],
    fee_rate: f32,
    network: BdkNetwork,
    coin_selection: Option<&CoinSelection>,
) -> Result<ComposeResult, String> {
    match coin_selection.unwrap_or(&CoinSelection::BranchAndBound) {
        CoinSelection::BranchAndBound => {
            let builder = wallet.build_tx();
            finish_psbt(builder, outputs, fee_rate, network)
        }
        CoinSelection::LargestFirst => {
            let builder = wallet.build_tx().coin_selection(LargestFirstCoinSelection);
            finish_psbt(builder, outputs, fee_rate, network)
        }
        CoinSelection::OldestFirst => {
            let builder = wallet.build_tx().coin_selection(OldestFirstCoinSelection);
            finish_psbt(builder, outputs, fee_rate, network)
        }
    }
}

/// Build a PSBT from pre-validated outputs and fee rate.
fn finish_psbt<Cs: CoinSelectionAlgorithm<MemoryDatabase>>(
    mut builder: bdk::wallet::tx_builder::TxBuilder<
        '_,
        MemoryDatabase,
        Cs,
        bdk::wallet::tx_builder::CreateTx,
    >,
    outputs: &[ComposeOutput],
    fee_rate: f32,
    network: BdkNetwork,
) -> Result<ComposeResult, String> {
    builder.fee_rate(FeeRate::from_sat_per_vb(fee_rate));
    builder.enable_rbf();

    for output in outputs {
        match output {
            ComposeOutput::Payment {
                address,
                amount_sats,
            } => {
                let script = parse_address(address, network)?;
                builder.add_recipient(script, *amount_sats);
            }
            ComposeOutput::SendMax { address } => {
                let script = parse_address(address, network)?;
                builder.drain_to(script);
                builder.drain_wallet();
            }
            ComposeOutput::OpReturn { data_hex } => {
                let data =
                    hex::decode(data_hex).map_err(|e| format!("Invalid OP_RETURN hex: {}", e))?;
                let push_data = bdk::bitcoin::script::PushBytesBuf::try_from(data)
                    .map_err(|e| format!("OP_RETURN data too large: {}", e))?;
                let script = bdk::bitcoin::blockdata::script::Builder::new()
                    .push_opcode(bdk::bitcoin::blockdata::opcodes::all::OP_RETURN)
                    .push_slice(push_data)
                    .into_script();
                builder.add_recipient(script, 0);
            }
        }
    }

    let (psbt, details) = builder.finish().map_err(|e| format!("{}", e))?;

    let fee = details
        .fee
        .ok_or("BDK could not determine the transaction fee")?;
    // Note: for self-transfers (e.g. consolidation), received includes the
    // destination output so total_spent will undercount. See ComposeResult docs.
    let total_spent = details.sent.saturating_sub(details.received);
    let psbt_base64 = general_purpose::STANDARD.encode(psbt.serialize());

    Ok(ComposeResult::Success {
        psbt: psbt_base64,
        fee,
        fee_rate,
        total_spent,
    })
}

fn parse_address(address: &str, network: BdkNetwork) -> Result<bdk::bitcoin::ScriptBuf, String> {
    let addr = address
        .parse::<BdkAddress<NetworkUnchecked>>()
        .map_err(|e| format!("Invalid address '{}': {}", address, e))?
        .require_network(network)
        .map_err(|e| format!("Address network mismatch: {}", e))?;
    Ok(addr.script_pubkey())
}

/// Pre-validate outputs before the Electrum sync for fast feedback.
pub(crate) fn validate_outputs(
    outputs: &[ComposeOutput],
    network: BdkNetwork,
) -> Result<(), String> {
    if outputs.is_empty() {
        return Err("At least one output is required".into());
    }
    let has_recipient = outputs.iter().any(|o| {
        matches!(
            o,
            ComposeOutput::Payment { .. } | ComposeOutput::SendMax { .. }
        )
    });
    if !has_recipient {
        return Err("At least one Payment or SendMax output is required".into());
    }

    let mut has_drain = false;
    for output in outputs {
        match output {
            ComposeOutput::Payment {
                address,
                amount_sats,
            } => {
                if *amount_sats == 0 {
                    return Err("Payment amount must be greater than zero".into());
                }
                parse_address(address, network)?;
            }
            ComposeOutput::SendMax { address } => {
                if has_drain {
                    return Err("Only one SendMax output is allowed".into());
                }
                parse_address(address, network)?;
                has_drain = true;
            }
            ComposeOutput::OpReturn { data_hex } => {
                if data_hex.is_empty() {
                    return Err("OP_RETURN data must not be empty".into());
                }
                let data =
                    hex::decode(data_hex).map_err(|e| format!("Invalid OP_RETURN hex: {}", e))?;
                if data.len() > 80 {
                    return Err(format!(
                        "OP_RETURN data exceeds 80-byte standard relay limit ({} bytes)",
                        data.len()
                    ));
                }
            }
        }
    }
    Ok(())
}
