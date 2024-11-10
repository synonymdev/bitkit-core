use thiserror::Error;

#[derive(uniffi::Enum, Debug, Error)]
pub enum DecodingError {
    #[error("Invalid invoice format")]
    InvalidFormat,
    #[error("Invalid network type")]
    InvalidNetwork,
    #[error("Invalid amount")]
    InvalidAmount,
    #[error("Invalid LNURL pay amount: {amount_satoshis} sats (must be between {min} and {max} sats)")]
    InvalidLNURLPayAmount {
        amount_satoshis: u64,
        min: u64,
        max: u64,
    },
    #[error("Invalid timestamp")]
    InvalidTimestamp,
    #[error("Invalid checksum")]
    InvalidChecksum,
    #[error("Invalid response")]
    InvalidResponse,
    #[error("Unsupported invoice type")]
    UnsupportedType,
    #[error("Invalid address")]
    InvalidAddress,
    #[error("Lnurl request failed")]
    RequestFailed,
    #[error("Client creation failed")]
    ClientCreationFailed,
    #[error("Invoice creation failed: {message}")]
    InvoiceCreationFailed {
        message: String,
    },
}