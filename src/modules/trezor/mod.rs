//! Trezor hardware wallet integration module.
//!
//! This module provides FFI-compatible interfaces for interacting with
//! Trezor hardware wallets via USB and Bluetooth connections.

pub mod account_info;
mod callbacks;
mod errors;
mod implementation;
#[cfg(test)]
mod tests;
mod types;

pub use account_info::account_type_to_script_type;
pub use callbacks::*;
pub use errors::*;
pub use implementation::*;
pub use types::*;
