#[derive(uniffi::Error, thiserror::Error, Debug)]
#[non_exhaustive]
pub enum PubkyError {
    #[error("Invalid capabilities: {reason}")]
    InvalidCapabilities { reason: String },

    #[error("Auth flow failed: {reason}")]
    AuthFailed { reason: String },

    #[error("No active auth flow")]
    NoActiveFlow,

    #[error("Resolution failed: {reason}")]
    ResolutionFailed { reason: String },

    #[error("Fetch failed: {reason}")]
    FetchFailed { reason: String },

    #[error("Profile not found")]
    ProfileNotFound,

    #[error("Profile parse failed: {reason}")]
    ProfileParseFailed { reason: String },

    #[error("Key error: {reason}")]
    KeyError { reason: String },

    #[error("Write failed: {reason}")]
    WriteFailed { reason: String },
}
