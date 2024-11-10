use thiserror::Error;

#[derive(uniffi::Enum, Debug, Error)]
pub enum AddressError {
    #[error("Invalid Bitcoin address format")]
    InvalidAddress,
    #[error("Invalid network type")]
    InvalidNetwork,
}