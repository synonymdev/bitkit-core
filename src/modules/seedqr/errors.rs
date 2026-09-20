use thiserror::Error;

#[derive(uniffi::Error, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeedQrError {
    #[error("Invalid Standard SeedQR payload")]
    InvalidStandardPayload,
    #[error("Invalid Compact SeedQR payload")]
    InvalidCompactPayload,
    #[error("SeedQR contains an invalid BIP39 mnemonic")]
    InvalidMnemonic,
}
