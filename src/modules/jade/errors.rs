//! Error types for the Jade module.

use thiserror::Error;

/// Error codes defined by Jade firmware in `main/utils/cbor_rpc.h`.
///
/// The standard JSON-RPC codes occupy -32600 to -32603; Jade's own codes occupy
/// -32000 to -32099.
pub(crate) mod rpc_code {
    pub const INVALID_REQUEST: i64 = -32600;
    pub const UNKNOWN_METHOD: i64 = -32601;
    pub const BAD_PARAMETERS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    pub const USER_CANCELLED: i64 = -32000;
    pub const PROTOCOL_ERROR: i64 = -32001;
    pub const HW_LOCKED: i64 = -32002;
    pub const NETWORK_MISMATCH: i64 = -32003;
}

/// Jade-related errors exposed via FFI.
#[derive(uniffi::Error, Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JadeError {
    /// Transport layer error (Bluetooth or serial communication).
    #[error("Transport error: {error_details}")]
    TransportError { error_details: String },

    /// No Jade device matched the requested identifier.
    #[error("No Jade device found")]
    DeviceNotFound,

    /// The device went away during an operation.
    #[error("Device disconnected during operation")]
    DeviceDisconnected,

    /// Another operation holds the device; back off and retry.
    #[error("Device is busy")]
    DeviceBusy,

    /// No connection is open. Call `jade_connect` first.
    #[error("Not connected to a Jade device")]
    NotConnected,

    /// No transport callback has been registered.
    #[error("Jade transport callback has not been set")]
    NotInitialized,

    /// Failed to open or establish a connection.
    #[error("Connection error: {error_details}")]
    ConnectionError { error_details: String },

    /// The device sent something that does not conform to the wire protocol.
    #[error("Protocol error: {error_details}")]
    ProtocolError { error_details: String },

    /// The operation exceeded its deadline.
    #[error("Operation timed out")]
    Timeout,

    /// The user declined on the device, or the host aborted the operation.
    #[error("Operation cancelled")]
    UserCancelled,

    /// The device has a PIN set and is locked. Call `jade_unlock`.
    #[error("Device is locked")]
    DeviceLocked,

    /// The device has no wallet. Setup must be completed on the device itself.
    #[error("Device has no wallet configured")]
    DeviceUninitialized,

    /// The PIN entered on the device was rejected by the pinserver.
    #[error("Incorrect PIN")]
    InvalidPin,

    /// The requested network does not match what the device is configured for.
    #[error("Network mismatch: {error_details}")]
    NetworkMismatch { error_details: String },

    /// The device firmware predates a feature this module requires.
    #[error("Jade firmware {installed} is too old, {required} or newer is required")]
    UnsupportedFirmware { installed: String, required: String },

    /// A BIP32 derivation path was malformed or not permitted here.
    #[error("Invalid derivation path: {error_details}")]
    InvalidPath { error_details: String },

    /// A PSBT failed to parse, or the device returned one that does not match.
    #[error("Invalid PSBT: {error_details}")]
    InvalidPsbt { error_details: String },

    /// The PSBT exceeds what the device can receive in one message.
    #[error("PSBT is {size} bytes, which exceeds the {max} byte limit")]
    PsbtTooLarge { size: u64, max: u64 },

    /// No PSBT input carries the connected device's master fingerprint, so the
    /// device would sign nothing.
    #[error("PSBT is for master fingerprint {psbt}, but the connected device is {device}")]
    FingerprintMismatch { device: String, psbt: String },

    /// The device returned a PSBT with no new signatures.
    #[error("Device did not add any signatures")]
    NothingSigned,

    /// The device returned an address that does not match the host-derived one.
    #[error("Address mismatch: expected {expected}, device returned {returned}")]
    AddressMismatch { expected: String, returned: String },

    /// The blind pinserver exchange failed.
    #[error("Pin server error: {error_details}")]
    PinServerError { error_details: String },

    /// The device reported an error that has no more specific mapping.
    #[error("Device error: {error_details}")]
    DeviceError { error_details: String },

    /// An internal or runtime failure on the host side.
    #[error("IO error: {error_details}")]
    IoError { error_details: String },
}

impl JadeError {
    /// Build a `ProtocolError` from anything displayable.
    pub(crate) fn protocol(details: impl std::fmt::Display) -> Self {
        JadeError::ProtocolError {
            error_details: details.to_string(),
        }
    }

    /// Build a `TransportError` from anything displayable.
    pub(crate) fn transport(details: impl std::fmt::Display) -> Self {
        JadeError::TransportError {
            error_details: details.to_string(),
        }
    }

    /// Map an error reply from the device onto a typed error.
    ///
    /// `UNKNOWN_METHOD` maps to `UnsupportedFirmware` rather than `ProtocolError`:
    /// the single-signature `get_receive_address` and `sign_psbt` calls this module
    /// relies on were added in later firmware, so an older unit answering -32601 is
    /// reporting its age, not a host bug.
    pub(crate) fn from_rpc(code: i64, message: String, min_firmware: &str) -> Self {
        match code {
            rpc_code::USER_CANCELLED => JadeError::UserCancelled,
            rpc_code::HW_LOCKED => JadeError::DeviceLocked,
            rpc_code::NETWORK_MISMATCH => JadeError::NetworkMismatch {
                error_details: message,
            },
            rpc_code::UNKNOWN_METHOD => JadeError::UnsupportedFirmware {
                installed: "unknown".to_string(),
                required: min_firmware.to_string(),
            },
            rpc_code::PROTOCOL_ERROR | rpc_code::INVALID_REQUEST => JadeError::ProtocolError {
                error_details: message,
            },
            rpc_code::BAD_PARAMETERS | rpc_code::INTERNAL_ERROR => JadeError::DeviceError {
                error_details: message,
            },
            other => JadeError::DeviceError {
                error_details: format!("device error {other}: {message}"),
            },
        }
    }
}
