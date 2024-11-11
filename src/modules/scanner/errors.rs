use thiserror::Error;
use crate::lnurl::LnurlError;
use crate::onchain::AddressError;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
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
    #[error("LNURL request failed")]
    RequestFailed,
    #[error("Client creation failed")]
    ClientCreationFailed,
    #[error("Invoice creation failed: {error_message}")]
    InvoiceCreationFailed {
        error_message: String,
    },
}

impl From<LnurlError> for DecodingError {
    fn from(error: LnurlError) -> Self {
        match error {
            LnurlError::InvoiceCreationFailed { error_details } => {
                DecodingError::InvoiceCreationFailed {
                    error_message: error_details
                }
            },
            LnurlError::InvalidAddress => DecodingError::InvalidFormat,
            LnurlError::ClientCreationFailed => DecodingError::ClientCreationFailed,
            LnurlError::RequestFailed => DecodingError::RequestFailed,
            LnurlError::InvalidResponse => DecodingError::InvalidResponse,
            LnurlError::InvalidAmount { amount_satoshis, min, max } => {
                DecodingError::InvalidLNURLPayAmount {
                    amount_satoshis,
                    min,
                    max
                }
            }
        }
    }
}

impl From<AddressError> for DecodingError {
    fn from(error: AddressError) -> Self {
        match error {
            AddressError::InvalidAddress => DecodingError::InvalidAddress,
            AddressError::InvalidNetwork => DecodingError::InvalidNetwork,
        }
    }
}