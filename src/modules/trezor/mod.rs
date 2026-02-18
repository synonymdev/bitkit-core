//! Trezor hardware wallet integration module.
//!
//! This module provides FFI-compatible interfaces for interacting with
//! Trezor hardware wallets via USB and Bluetooth connections.

mod errors;
mod types;
mod implementation;
mod callbacks;
#[cfg(test)]
mod tests;

pub use errors::*;
pub use types::*;
pub use implementation::*;
pub use callbacks::*;
