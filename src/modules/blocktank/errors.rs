use thiserror::Error;
use crate::modules::blocktank::BtChannelOrderErrorType;

#[derive(uniffi::Error, Debug, Error)]
pub enum BlocktankError {
    #[error("HTTP client error: {error_details}")]
    HttpClient {
        error_details: String
    },

    #[error("Blocktank error: {error_details}")]
    BlocktankClient {
        error_details: String,
    },
    #[error("Invalid Blocktank: {error_details}")]
    InvalidBlocktank {
        error_details: String,
    },
    #[error("Database initialization failed: {error_details}")]
    InitializationError {
        error_details: String,
    },

    #[error("Failed to insert blocktank: {error_details}")]
    InsertError {
        error_details: String,
    },

    #[error("Failed to retrieve blocktanks: {error_details}")]
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
    },


    #[error("Channel open error: {error_type:?} - {error_details}")]
    ChannelOpen {
        error_type: BtChannelOrderErrorType,
        error_details: String,
    },

    #[error("Order state error: {error_details}")]
    OrderState {
        error_details: String
    },

    #[error("Invalid parameter: {error_details}")]
    InvalidParameter {
        error_details: String
    },
    #[error("Database error: {error_details}")]
    DatabaseError {
        error_details: String,
    },
}

impl From<serde_json::Error> for BlocktankError {
    fn from(err: serde_json::Error) -> Self {
        BlocktankError::SerializationError {
            error_details: err.to_string()
        }
    }
}

impl From<url::ParseError> for BlocktankError {
    fn from(err: url::ParseError) -> Self {
        BlocktankError::ConnectionError {
            error_details: err.to_string()
        }
    }
}

#[derive(uniffi::Record, Debug)]
pub struct ErrorData {
    pub error_details: String,
}