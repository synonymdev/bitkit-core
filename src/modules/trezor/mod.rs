//! Trezor hardware wallet integration module.
//!
//! This module provides FFI-compatible interfaces for interacting with
//! Trezor hardware wallets via USB and Bluetooth connections.

mod errors;
mod types;
mod implementation;
mod callbacks;
pub mod account_info;
pub mod compose;
#[cfg(test)]
mod tests;

pub use errors::*;
pub use types::*;
pub use implementation::*;
pub use callbacks::*;
pub use account_info::{account_type_to_script_type, fetch_prev_txs};
pub use compose::{precompose_transaction, precomposed_to_sign_params};
