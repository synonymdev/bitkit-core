use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
pub enum ActivityError {
    #[error("{0}")]
    InvalidActivity(String),

    #[error("{0}")]
    InitializationError(String),

    #[error("{0}")]
    InsertError(String),

    #[error("{0}")]
    RetrievalError(String),

    #[error("{0}")]
    DataError(String),

    #[error("{0}")]
    ConnectionError(String),

    #[error("{0}")]
    SerializationError(String),
}