//! Blockstream Jade hardware wallet integration.
//!
//! Jade speaks a JSON-RPC shaped protocol encoded as CBOR over either Bluetooth
//! (the Nordic UART Service) or USB CDC serial. Unlike the `trezor` module,
//! which adapts an external crate, the protocol is implemented here.
//!
//! Layering, outermost first:
//!
//! - `src/lib.rs` exports thin `jade_*` async wrappers over a global manager.
//! - `implementation.rs` owns session state and the single active connection.
//! - `pinserver.rs` runs the blind pinserver exchange that unlocks the device.
//! - `transport.rs` frames requests onto a byte stream and correlates replies.
//! - `protocol.rs` is pure CBOR framing, envelopes and id correlation.
//! - `callbacks.rs` is the trait the native app implements to do Bluetooth I/O.
//! - `serial.rs` is a Rust side serial transport for desktop and Python.
//!
//! One hard rule for anything added here: no `#[uniffi::export]` item may be
//! `cfg` gated. All three build scripts generate bindings from the host library
//! rather than the target one, so a host only export would appear in the
//! generated Swift and Kotlin while being absent from the device library.

mod callbacks;
mod errors;
mod implementation;
mod path;
mod pinserver;
mod protocol;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod serial;
#[cfg(test)]
mod tests;
mod transport;
mod types;

pub use callbacks::{
    jade_set_transport_callback, JadeNativeDevice, JadeTransportCallback, JadeTransportErrorCode,
    JadeTransportReadResult, JadeTransportResult,
};
pub use errors::JadeError;
pub use implementation::JadeManager;
pub use types::{
    JadeAccount, JadeAccountExport, JadeAddressVariant, JadeDeviceInfo, JadeGetXpubParams,
    JadeNetwork, JadePingStatus, JadeSignMessageParams, JadeSignPsbtParams, JadeSignedMessage,
    JadeState, JadeTransportKind, JadeVerifyAddressParams, JadeVersionInfo, JadeXpubResponse,
};
