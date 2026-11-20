use thiserror::Error;

/// Errors surfaced by the Pubky Swaps route module.
#[derive(uniffi::Error, Debug, Error)]
pub enum PubkySwapError {
    #[error("Invalid input: {error_details}")]
    InvalidInput { error_details: String },

    #[error("Provider unreachable: {error_details}")]
    Transport { error_details: String },

    /// The provider refused the request with one of its closed refusal codes.
    #[error("Provider refused ({code}): {reason}")]
    Refused { code: String, reason: String },

    /// The provider's quote, contract or invoice did not match what was asked for.
    #[error("Validation failed: {error_details}")]
    Validation { error_details: String },

    #[error("Chain error: {error_details}")]
    Chain { error_details: String },

    #[error("Storage error: {error_details}")]
    Storage { error_details: String },

    #[error("Swap not found: {error_details}")]
    NotFound { error_details: String },
}

impl PubkySwapError {
    pub(crate) fn invalid(details: impl ToString) -> Self {
        Self::InvalidInput {
            error_details: details.to_string(),
        }
    }

    pub(crate) fn transport(details: impl ToString) -> Self {
        Self::Transport {
            error_details: details.to_string(),
        }
    }

    pub(crate) fn validation(details: impl ToString) -> Self {
        Self::Validation {
            error_details: details.to_string(),
        }
    }

    pub(crate) fn chain(details: impl ToString) -> Self {
        Self::Chain {
            error_details: details.to_string(),
        }
    }

    pub(crate) fn storage(details: impl ToString) -> Self {
        Self::Storage {
            error_details: details.to_string(),
        }
    }

    /// This error with every copy of `secret` replaced, so a bearer token never reaches the app
    /// or a log, even when a provider echoes it.
    pub(crate) fn redact(self, secret: &str) -> Self {
        if secret.is_empty() {
            return self;
        }
        let hide = |text: String| text.replace(secret, "<redacted>");
        match self {
            Self::InvalidInput { error_details } => Self::InvalidInput {
                error_details: hide(error_details),
            },
            Self::Transport { error_details } => Self::Transport {
                error_details: hide(error_details),
            },
            Self::Refused { code, reason } => Self::Refused {
                code: hide(code),
                reason: hide(reason),
            },
            Self::Validation { error_details } => Self::Validation {
                error_details: hide(error_details),
            },
            Self::Chain { error_details } => Self::Chain {
                error_details: hide(error_details),
            },
            Self::Storage { error_details } => Self::Storage {
                error_details: hide(error_details),
            },
            Self::NotFound { error_details } => Self::NotFound {
                error_details: hide(error_details),
            },
        }
    }
}

impl From<serde_json::Error> for PubkySwapError {
    fn from(err: serde_json::Error) -> Self {
        Self::storage(err)
    }
}

impl From<std::io::Error> for PubkySwapError {
    fn from(err: std::io::Error) -> Self {
        Self::storage(err)
    }
}

impl From<electrum_client::Error> for PubkySwapError {
    fn from(err: electrum_client::Error) -> Self {
        Self::chain(err)
    }
}
