use thiserror::Error;

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
                TcTransportError::DeviceBusy => TrezorError::ConnectionError {
                    error_details: "Device is busy".to_string(),
                },
                TcTransportError::UnableToOpen(msg) => TrezorError::TransportError {
                    error_details: format!("Unable to open device: {}", msg),
                },
                TcTransportError::UnableToClose(msg) => TrezorError::TransportError {
                    error_details: format!("Unable to close device: {}", msg),
                },
                TcTransportError::DataTransfer(msg) => TrezorError::TransportError {
                    error_details: format!("Data transfer error: {}", msg),
                },
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
                TcDeviceError::Failure { code, message } => TrezorError::DeviceError {
                    error_details: format!("Device failure (code {:?}): {}", code, message),
                },
                TcDeviceError::DeviceError { code, message } => TrezorError::DeviceError {
                    error_details: format!("Device error (code {}): {}", code, message),
                },
                TcDeviceError::ButtonRequest(msg) => TrezorError::DeviceError {
                    error_details: format!("Button request: {}", msg),
                },
                TcDeviceError::ProtobufDecode(msg) => TrezorError::ProtocolError {
                    error_details: format!("Protobuf decode error: {}", msg),
                },
                TcDeviceError::InvalidInput(msg) => TrezorError::DeviceError {
                    error_details: format!("Invalid input: {}", msg),
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
            },
        }
    }
}
