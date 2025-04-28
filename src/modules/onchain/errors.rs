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
    #[error("Address derivation failed")]
    AddressDerivationFailed,
}