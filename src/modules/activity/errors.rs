use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum ActivityError {
    #[error("Invalid Activity")]
    InvalidActivity,
    #[error("Database initialization failed: {message}")]
    InitializationError {
        message: String,
    },

    #[error("Failed to insert activity: {message}")]
    InsertError {
        message: String,
    },

    #[error("Failed to retrieve activities: {message}")]
    RetrievalError {
        message: String,
    },

    #[error("Invalid data format: {message}")]
    DataError {
        message: String,
    },

    #[error("Database connection error: {message}")]
    ConnectionError {
        message: String,
    },

    #[error("Serialization error: {message}")]
    SerializationError {
        message: String,
    }
}