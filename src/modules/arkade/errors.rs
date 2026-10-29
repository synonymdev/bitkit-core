use std::fmt::Display;
use thiserror::Error;

/// Errors surfaced by the Arkade module.
#[derive(uniffi::Error, Debug, Error)]
pub enum ArkadeError {
    #[error("Arkade wallet not started")]
    NotStarted,

    #[error("Invalid input: {error_details}")]
    InvalidInput { error_details: String },

    #[error("Arkade operator error: {error_details}")]
    ArkError { error_details: String },

    #[error("Lightning address service error: {error_details}")]
    LnurlError { error_details: String },

    #[error("Network error: {error_details}")]
    NetworkError { error_details: String },

    #[error("Database error: {error_details}")]
    DatabaseError { error_details: String },

    #[error("Internal error: {error_details}")]
    InternalError { error_details: String },
}

impl ArkadeError {
    pub(crate) fn invalid(e: impl Display) -> Self {
        Self::InvalidInput {
            error_details: e.to_string(),
        }
    }

    pub(crate) fn internal(e: impl Display) -> Self {
        Self::InternalError {
            error_details: e.to_string(),
        }
    }

    pub(crate) fn ark(e: impl Display) -> Self {
        Self::ArkError {
            error_details: format!("{e:#}"),
        }
    }

    pub(crate) fn lnurl(e: impl Display) -> Self {
        Self::LnurlError {
            error_details: e.to_string(),
        }
    }

    pub(crate) fn network(e: impl Display) -> Self {
        Self::NetworkError {
            error_details: e.to_string(),
        }
    }

    pub(crate) fn storage(e: impl Display) -> Self {
        Self::DatabaseError {
            error_details: e.to_string(),
        }
    }
}

impl From<ark_client::Error> for ArkadeError {
    fn from(e: ark_client::Error) -> Self {
        Self::ark(e)
    }
}

impl From<rusqlite::Error> for ArkadeError {
    fn from(e: rusqlite::Error) -> Self {
        Self::storage(e)
    }
}
