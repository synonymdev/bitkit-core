use thiserror::Error;

use super::callbacks::TrezorTransportErrorCode;

const CALLBACK_ERROR_CODE_DEVICE_BUSY: &str = "__bitkit_trezor_callback_error:device_busy";

/// Trezor-related errors exposed via FFI.
#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum TrezorError {
    /// Transport layer error (USB/Bluetooth communication)
    #[error("Transport error: {error_details}")]
    TransportError { error_details: String },

    /// No Trezor device found
    #[error("No Trezor device found")]
    DeviceNotFound,

    /// Device disconnected during operation
    #[error("Device disconnected during operation")]
    DeviceDisconnected,

    /// Device is busy and the caller should back off before retrying
    #[error("Device is busy")]
    DeviceBusy,

    /// Connection error
    #[error("Connection error: {error_details}")]
    ConnectionError { error_details: String },

    /// Protocol error (encoding/decoding)
    #[error("Protocol error: {error_details}")]
    ProtocolError { error_details: String },

    /// Pairing required for Bluetooth connection
    #[error("Pairing required")]
    PairingRequired,

    /// Pairing failed
    #[error("Pairing failed: {error_details}")]
    PairingFailed { error_details: String },

    /// PIN is required
    #[error("PIN is required")]
    PinRequired,

    /// PIN entry cancelled
    #[error("PIN entry cancelled")]
    PinCancelled,

    /// Invalid PIN entered
    #[error("Invalid PIN")]
    InvalidPin,

    /// Passphrase is required
    #[error("Passphrase is required")]
    PassphraseRequired,

    /// Passphrase entry cancelled
    #[error("Passphrase entry cancelled")]
    PassphraseCancelled,

    /// Action cancelled by user on device
    #[error("Action cancelled by user")]
    UserCancelled,

    /// Operation timed out
    #[error("Operation timed out")]
    Timeout,

    /// Invalid derivation path
    #[error("Invalid path: {error_details}")]
    InvalidPath { error_details: String },

    /// Device returned an error
    #[error("Device error: {error_details}")]
    DeviceError { error_details: String },

    /// Trezor manager not initialized
    #[error("Trezor not initialized. Call trezor_initialize first.")]
    NotInitialized,

    /// No device connected
    #[error("No device connected. Call trezor_connect first.")]
    NotConnected,

    /// Session error
    #[error("Session error: {error_details}")]
    SessionError { error_details: String },

    /// IO error
    #[error("IO error: {error_details}")]
    IoError { error_details: String },

    /// The device reported a firmware-level fault (protocol failure code 99,
    /// `Failure_FirmwareError`). The session is unusable; the caller should ask
    /// the user to reconnect the hardware rather than retry in place.
    #[error("Firmware error: {error_details}")]
    FirmwareError { error_details: String },

    /// The device has PIN protection enabled and is currently locked, so the
    /// user must unlock it before anything else can proceed. Distinct from
    /// [`TrezorError::DeviceBusy`], which means transport/session contention
    /// and is worth a backoff-and-retry.
    #[error("Device is locked")]
    DeviceLocked,
}

/// `FailureType.Failure_FirmwareError` from Trezor's `messages-common.proto`.
const FAILURE_CODE_FIRMWARE_ERROR: i32 = 99;

#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
pub(crate) fn encode_callback_transport_error(
    error_details: String,
    error_code: Option<TrezorTransportErrorCode>,
) -> String {
    match error_code {
        Some(TrezorTransportErrorCode::DeviceBusy) => {
            if error_details.is_empty() {
                CALLBACK_ERROR_CODE_DEVICE_BUSY.to_string()
            } else {
                format!("{}: {}", CALLBACK_ERROR_CODE_DEVICE_BUSY, error_details)
            }
        }
        None => error_details,
    }
}

fn callback_transport_error_from_message(error_details: &str) -> Option<TrezorError> {
    if error_details == CALLBACK_ERROR_CODE_DEVICE_BUSY
        || error_details.starts_with(&format!("{}:", CALLBACK_ERROR_CODE_DEVICE_BUSY))
    {
        return Some(TrezorError::DeviceBusy);
    }

    None
}

impl From<trezor_connect_rs::TrezorError> for TrezorError {
    fn from(err: trezor_connect_rs::TrezorError) -> Self {
        use trezor_connect_rs::error::{
            BitcoinError, DeviceError as TcDeviceError, ProtocolError as TcProtocolError,
            SessionError as TcSessionError, ThpError, TransportError as TcTransportError,
        };
        use trezor_connect_rs::TrezorError as TE;

        match err {
            // Top-level errors
            TE::Cancelled => TrezorError::UserCancelled,
            TE::Timeout => TrezorError::Timeout,
            TE::IoError(msg) => TrezorError::IoError { error_details: msg },

            // Transport errors
            TE::Transport(transport_err) => match transport_err {
                TcTransportError::DeviceNotFound => TrezorError::DeviceNotFound,
                TcTransportError::DeviceDisconnected => TrezorError::DeviceDisconnected,
                TcTransportError::DeviceBusy => TrezorError::DeviceBusy,
                TcTransportError::UnableToOpen(msg) => callback_transport_error_from_message(&msg)
                    .unwrap_or_else(|| TrezorError::TransportError {
                        error_details: format!("Unable to open device: {}", msg),
                    }),
                TcTransportError::UnableToClose(msg) => TrezorError::TransportError {
                    error_details: format!("Unable to close device: {}", msg),
                },
                TcTransportError::DataTransfer(msg) => callback_transport_error_from_message(&msg)
                    .unwrap_or_else(|| TrezorError::TransportError {
                        error_details: format!("Data transfer error: {}", msg),
                    }),
                TcTransportError::PermissionDenied(msg) => TrezorError::TransportError {
                    error_details: format!("Permission denied: {}", msg),
                },
                #[cfg(feature = "usb")]
                TcTransportError::Usb(msg) => TrezorError::TransportError {
                    error_details: format!("USB error: {}", msg),
                },
                #[cfg(feature = "bluetooth")]
                TcTransportError::Bluetooth(msg) => TrezorError::TransportError {
                    error_details: format!("Bluetooth error: {}", msg),
                },
                #[allow(unreachable_patterns)]
                _ => TrezorError::TransportError {
                    error_details: transport_err.to_string(),
                },
            },

            // Protocol errors
            TE::Protocol(protocol_err) => match protocol_err {
                TcProtocolError::Malformed(msg) => TrezorError::ProtocolError {
                    error_details: format!("Malformed message: {}", msg),
                },
                TcProtocolError::InvalidMessageType(mt) => TrezorError::ProtocolError {
                    error_details: format!("Invalid message type: {}", mt),
                },
                TcProtocolError::MessageTooShort { expected, actual } => {
                    TrezorError::ProtocolError {
                        error_details: format!(
                            "Message too short: expected {}, got {}",
                            expected, actual
                        ),
                    }
                }
                TcProtocolError::InvalidHeader => TrezorError::ProtocolError {
                    error_details: "Invalid header".to_string(),
                },
                TcProtocolError::ChunkHeaderMismatch => TrezorError::ProtocolError {
                    error_details: "Chunk header mismatch".to_string(),
                },
                TcProtocolError::ProtobufEncode(msg) => TrezorError::ProtocolError {
                    error_details: format!("Protobuf encode error: {}", msg),
                },
                TcProtocolError::ProtobufDecode(msg) => TrezorError::ProtocolError {
                    error_details: format!("Protobuf decode error: {}", msg),
                },
                TcProtocolError::UnexpectedResponse { expected, actual } => {
                    TrezorError::ProtocolError {
                        error_details: format!(
                            "Unexpected response: expected {}, got {}",
                            expected, actual
                        ),
                    }
                }
                _ => TrezorError::ProtocolError {
                    error_details: protocol_err.to_string(),
                },
            },

            // Device errors
            TE::Device(device_err) => match device_err {
                TcDeviceError::NotConnected => TrezorError::NotConnected,
                TcDeviceError::ActionCancelled => TrezorError::UserCancelled,
                TcDeviceError::PinRequired => TrezorError::PinRequired,
                TcDeviceError::InvalidPin => TrezorError::InvalidPin,
                TcDeviceError::PinCancelled => TrezorError::PinCancelled,
                TcDeviceError::PassphraseRequired => TrezorError::PassphraseRequired,
                TcDeviceError::PassphraseCancelled => TrezorError::PassphraseCancelled,
                TcDeviceError::NotInitialized => TrezorError::DeviceError {
                    error_details: "Device is not initialized".to_string(),
                },
                TcDeviceError::FirmwareUpdateRequired => TrezorError::DeviceError {
                    error_details: "Firmware update required".to_string(),
                },
                TcDeviceError::SeedNotBackedUp => TrezorError::DeviceError {
                    error_details: "Seed is not backed up".to_string(),
                },
                TcDeviceError::NotSupported(msg) => TrezorError::DeviceError {
                    error_details: format!("Feature not supported: {}", msg),
                },
                // Both failure shapes keep their existing `error_details` text;
                // only the variant changes for firmware faults, so consumers get
                // a typed signal without losing the diagnostic string.
                TcDeviceError::Failure { code, message } => {
                    let error_details = format!("Device failure (code {:?}): {}", code, message);
                    if code == Some(FAILURE_CODE_FIRMWARE_ERROR) {
                        TrezorError::FirmwareError { error_details }
                    } else {
                        TrezorError::DeviceError { error_details }
                    }
                }
                TcDeviceError::DeviceError { code, message } => {
                    let error_details = format!("Device error (code {}): {}", code, message);
                    if code == FAILURE_CODE_FIRMWARE_ERROR {
                        TrezorError::FirmwareError { error_details }
                    } else {
                        TrezorError::DeviceError { error_details }
                    }
                }
                TcDeviceError::ButtonRequest(msg) => TrezorError::DeviceError {
                    error_details: format!("Button request: {}", msg),
                },
                TcDeviceError::ProtobufDecode(msg) => TrezorError::ProtocolError {
                    error_details: format!("Protobuf decode error: {}", msg),
                },
                TcDeviceError::InvalidInput(msg) => TrezorError::DeviceError {
                    error_details: format!("Invalid input: {}", msg),
                },
                // Wrong passphrase for a remembered wallet (static-session-id
                // mismatch). Only produced if the caller opts into
                // ConnectedDevice::verify_session_state, which bitkit-core does
                // not currently call — mapped here to keep the match exhaustive.
                TcDeviceError::InvalidState => TrezorError::DeviceError {
                    error_details: "Passphrase is incorrect (device state mismatch)".to_string(),
                },
                _ => TrezorError::DeviceError {
                    error_details: device_err.to_string(),
                },
            },

            // THP (Trezor Host Protocol) errors
            TE::Thp(thp_err) => match thp_err {
                ThpError::PairingRequired => TrezorError::PairingRequired,
                ThpError::PairingFailed(msg) => TrezorError::PairingFailed { error_details: msg },
                ThpError::InvalidCredentials => TrezorError::PairingFailed {
                    error_details: "Invalid credentials".to_string(),
                },
                ThpError::ChannelAllocationFailed => TrezorError::ConnectionError {
                    error_details: "THP channel allocation failed".to_string(),
                },
                ThpError::HandshakeFailed(msg) => TrezorError::ConnectionError {
                    error_details: format!("THP handshake failed: {}", msg),
                },
                // The device is locked; the caller should prompt the user to
                // unlock rather than retrying the connection in a loop.
                ThpError::DeviceLocked => TrezorError::DeviceLocked,
                ThpError::EncryptionError(msg) => TrezorError::ProtocolError {
                    error_details: format!("THP encryption error: {}", msg),
                },
                ThpError::DecryptionError(msg) => TrezorError::ProtocolError {
                    error_details: format!("THP decryption error: {}", msg),
                },
                ThpError::AckNotReceived => TrezorError::ProtocolError {
                    error_details: "THP ACK not received".to_string(),
                },
                ThpError::InvalidSyncBit => TrezorError::ProtocolError {
                    error_details: "THP invalid sync bit".to_string(),
                },
                ThpError::StateMissing => TrezorError::SessionError {
                    error_details: "THP state missing".to_string(),
                },
                ThpError::SessionError(msg) => TrezorError::SessionError {
                    error_details: format!("THP session error: {}", msg),
                },
                _ => TrezorError::ConnectionError {
                    error_details: format!("THP error: {}", thp_err),
                },
            },

            // Session errors
            TE::Session(session_err) => match session_err {
                TcSessionError::NotFound => TrezorError::SessionError {
                    error_details: "Session not found".to_string(),
                },
                TcSessionError::WrongPrevious => TrezorError::SessionError {
                    error_details: "Wrong previous session".to_string(),
                },
                TcSessionError::AlreadyAcquired => TrezorError::SessionError {
                    error_details: "Session already acquired by another client".to_string(),
                },
                TcSessionError::Expired => TrezorError::SessionError {
                    error_details: "Session expired".to_string(),
                },
                _ => TrezorError::SessionError {
                    error_details: session_err.to_string(),
                },
            },

            // Bitcoin errors
            TE::Bitcoin(bitcoin_err) => match bitcoin_err {
                BitcoinError::InvalidPath(msg) => TrezorError::InvalidPath { error_details: msg },
                BitcoinError::InvalidAddress(msg) => TrezorError::DeviceError {
                    error_details: format!("Invalid address: {}", msg),
                },
                BitcoinError::InvalidTransaction(msg) => TrezorError::DeviceError {
                    error_details: format!("Invalid transaction: {}", msg),
                },
                BitcoinError::InsufficientFunds => TrezorError::DeviceError {
                    error_details: "Insufficient funds".to_string(),
                },
                BitcoinError::InvalidSignature => TrezorError::DeviceError {
                    error_details: "Invalid signature".to_string(),
                },
                BitcoinError::NetworkMismatch { expected, actual } => TrezorError::DeviceError {
                    error_details: format!(
                        "Network mismatch: expected {}, got {}",
                        expected, actual
                    ),
                },
                _ => TrezorError::DeviceError {
                    error_details: bitcoin_err.to_string(),
                },
            },

            // The upstream error enums are `#[non_exhaustive]`, so any future
            // variant we don't explicitly map falls through to a generic error.
            _ => TrezorError::DeviceError {
                error_details: err.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trezor_connect_rs::error::ThpError;
    use trezor_connect_rs::TrezorError as TE;

    #[test]
    fn thp_device_locked_maps_to_device_locked() {
        // A locked device must surface as its own typed signal so callers can
        // prompt the user to unlock, rather than as DeviceBusy (back off and
        // retry) or a generic ConnectionError.
        let mapped: TrezorError = TE::Thp(ThpError::DeviceLocked).into();
        assert!(matches!(mapped, TrezorError::DeviceLocked));
    }
}
