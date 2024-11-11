use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum AddressError {
    #[error("Invalid Bitcoin address format")]
    InvalidAddress,
    #[error("Invalid network type")]
    InvalidNetwork,
}