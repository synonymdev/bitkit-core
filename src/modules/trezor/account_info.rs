//! Trezor-specific account info helpers.
//!
//! Functions that bridge generic account types to Trezor's signing protocol.

use crate::modules::onchain::AccountType;
use super::types::TrezorScriptType;

/// Map AccountType to Trezor's ScriptType for transaction inputs.
pub fn account_type_to_script_type(account_type: AccountType) -> TrezorScriptType {
    match account_type {
        AccountType::Legacy => TrezorScriptType::SpendAddress,
        AccountType::WrappedSegwit => TrezorScriptType::SpendP2shWitness,
        AccountType::NativeSegwit => TrezorScriptType::SpendWitness,
        AccountType::Taproot => TrezorScriptType::SpendTaproot,
    }
}
