use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
pub enum ActivityError {
    #[error("Invalid Activity: {error_details}")]
    InvalidActivity {
        error_details: String,
    },

    #[error("Database initialization failed: {error_details}")]
    InitializationError {
        error_details: String,
    },

    #[error("Failed to insert activity: {error_details}")]
    InsertError {
        error_details: String,
    },

    #[error("Failed to retrieve activities: {error_details}")]
    RetrievalError {
        error_details: String,
    },

    #[error("Invalid data format: {error_details}")]
    DataError {
        error_details: String,
    },

    #[error("Database connection error: {error_details}")]
    ConnectionError {
        error_details: String,
    },

    #[error("Serialization error: {error_details}")]
    SerializationError {
        error_details: String,
    }
}