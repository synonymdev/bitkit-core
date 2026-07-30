use thiserror::Error;

/// Errors surfaced by the Boltz swaps module.
#[derive(uniffi::Error, Debug, Error)]
pub enum BoltzError {
    #[error("Database initialization failed: {error_details}")]
    InitializationError { error_details: String },

    #[error("Database connection error: {error_details}")]
    ConnectionError { error_details: String },

    #[error("Database error: {error_details}")]
    DatabaseError { error_details: String },

    #[error("Boltz API error: {error_details}")]
    ApiError { error_details: String },

    #[error("Swap error: {error_details}")]
    SwapError { error_details: String },

    #[error("Broadcast error: {error_details}")]
    BroadcastError { error_details: String },

    #[error("Invalid input: {error_details}")]
    InvalidInput { error_details: String },

    #[error("Serialization error: {error_details}")]
    SerializationError { error_details: String },

    #[error("Swap not found: {error_details}")]
    NotFound { error_details: String },
}

impl From<boltz_client::error::Error> for BoltzError {
    fn from(err: boltz_client::error::Error) -> Self {
        BoltzError::SwapError {
            error_details: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for BoltzError {
    fn from(err: serde_json::Error) -> Self {
        BoltzError::SerializationError {
            error_details: err.to_string(),
        }
    }
}

impl From<rusqlite::Error> for BoltzError {
    fn from(err: rusqlite::Error) -> Self {
        BoltzError::DatabaseError {
            error_details: err.to_string(),
        }
    }
}
