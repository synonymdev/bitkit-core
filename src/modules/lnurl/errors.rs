use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum LnurlError {
    #[error("Invalid Lightning Address format")]
    InvalidAddress,
    #[error("Failed to create LNURL client")]
    ClientCreationFailed,
    #[error("LNURL request failed")]
    RequestFailed,
    #[error("Invalid response from LNURL service")]
    InvalidResponse,
    #[error("Amount {amount_satoshis} is outside allowed range ({min} - {max} sats)")]
    InvalidAmount {
        amount_satoshis: u64,
        min: u64,
        max: u64,
    },
    #[error("Failed to generate invoice: {error_details}")]
    InvoiceCreationFailed {
        error_details: String,
    },
}