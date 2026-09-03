//! Blockstream Jade hardware wallet integration.
//!
//! The protocol lives in the `jade-client-rs` crate. This module is the FFI
//! adapter: it attaches UniFFI scaffolding to that crate's types, exposes the
//! transport contract the native application implements, and owns the session
//! state a free-function FFI surface implies.
//!
//! One hard rule for anything added here: no `#[uniffi::export]` item may be
//! `cfg` gated. All three build scripts generate bindings from the host library
//! rather than the target one, so a host only export would appear in the
//! generated Swift and Kotlin while being absent from the device library.

mod callbacks;
mod implementation;
#[cfg(test)]
mod tests;
mod types;

pub use callbacks::{
    jade_set_transport_callback, JadeNativeDevice, JadeTransportCallback, JadeTransportReadResult,
    JadeTransportResult,
};
pub use implementation::JadeManager;
pub(crate) use types::account_type_to_variant;
pub use types::{
    JadeAccount, JadeAccountExport, JadeAddressVariant, JadeDeviceInfo, JadeError, JadeNetwork,
    JadePingStatus, JadeSignedMessage, JadeState, JadeTransportErrorCode, JadeTransportKind,
    JadeVersionInfo, JadeXpubResponse,
};
