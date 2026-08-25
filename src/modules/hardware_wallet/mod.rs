//! Signer-neutral hardware-wallet types.
//!
//! Device protocols remain in vendor adapters such as `trezor`. This module owns
//! only the vendor, transport, and catalog metadata shared across those adapters.

mod catalog;
#[cfg(test)]
mod tests;
mod types;

pub use catalog::get_supported_hardware_wallets;
pub use types::{HardwareWalletTransport, HardwareWalletVendor, SupportedHardwareWallet};
