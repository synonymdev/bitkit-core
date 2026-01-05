use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum AddressError {
    #[error("Invalid Bitcoin address format")]
    InvalidAddress,
    #[error("Invalid network type")]
    InvalidNetwork,
    #[error("Mnemonic generation failed")]
    MnemonicGenerationFailed,
    #[error("Invalid mnemonic format")]
    InvalidMnemonic,
    #[error("Invalid entropy")]
    InvalidEntropy,
    #[error("Address derivation failed")]
    AddressDerivationFailed,
}

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum SweepError {
    #[error("Sweep operation failed: {0}")]
    SweepFailed(String),
    #[error("No UTXOs found to sweep")]
    NoUtxosFound,
    #[error("Insufficient funds to cover fees")]
    InsufficientFunds,
    #[error("Invalid mnemonic format")]
    InvalidMnemonic,
}
