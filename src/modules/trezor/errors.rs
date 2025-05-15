use thiserror::Error;

/// Error types for Trezor Connect operations
#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum TrezorConnectError {
    #[error("Serialization error: {error_details}")]
    /// Error during serialization/deserialization
    SerdeError {
        error_details: String,
    },

    #[error("URL error: {error_details}")]
    /// Error with URL parsing or formatting
    UrlError {
        error_details: String,
    },

    #[error("Environment error: {error_details}")]
    /// Environment-related errors
    EnvironmentError {
        error_details: String,
    },

    #[error("Error: {error_details}")]
    /// General errors
    Other {
        error_details: String,
    },

    #[error("Unable to create client: {error_details}")]
    ClientError {
        error_details: String,
    },
}

impl From<serde_json::Error> for TrezorConnectError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerdeError {
            error_details: error.to_string(),
        }
    }
}

impl From<url::ParseError> for TrezorConnectError {
    fn from(error: url::ParseError) -> Self {
        Self::UrlError {
            error_details: error.to_string(),
        }
    }
}
