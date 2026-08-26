use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum UrError {
    #[error("Invalid UR: {reason}")]
    InvalidUr { reason: String },
    #[error("UR exceeds size limits: {reason}")]
    TooLarge { reason: String },
    #[error("Invalid UR payload: {reason}")]
    InvalidPayload { reason: String },
    #[error("Invalid PSBT: {reason}")]
    InvalidPsbt { reason: String },
    #[error("Invalid Passport account export: {reason}")]
    InvalidPassportExport { reason: String },
}
