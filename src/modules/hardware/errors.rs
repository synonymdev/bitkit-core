use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum HardwareError {
    #[error("Failed to initialize hardware wallet: {0}")]
    InitializationError(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Failed to get executable directory")]
    ExecutableDirectoryError,

    #[error("Failed to communicate with hardware device: {0}")]
    CommunicationError(String),

    #[error("JSON serialization/deserialization error: {0}")]
    JsonError(String),
}