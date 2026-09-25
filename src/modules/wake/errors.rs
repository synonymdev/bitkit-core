/// Errors of the wake module. Messages never contain secrets, push tokens or
/// decrypted plaintext.
#[derive(uniffi::Error, thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WakeError {
    #[error("Invalid input: {reason}")]
    InvalidInput { reason: String },

    #[error("Invalid key: {reason}")]
    InvalidKey { reason: String },

    #[error("Decryption failed: {reason}")]
    DecryptionFailed { reason: String },

    #[error("Unsupported envelope: {reason}")]
    UnsupportedEnvelope { reason: String },

    #[error("Request failed: {reason}")]
    RequestFailed { reason: String },

    #[error("Gateway rejected the request with HTTP {status} ({code}): {message}")]
    GatewayRejected {
        status: u16,
        code: String,
        message: String,
    },

    #[error("Invalid response: {reason}")]
    InvalidResponse { reason: String },
}

impl WakeError {
    pub(crate) fn invalid_input(reason: impl Into<String>) -> Self {
        WakeError::InvalidInput {
            reason: reason.into(),
        }
    }

    pub(crate) fn invalid_key(reason: impl Into<String>) -> Self {
        WakeError::InvalidKey {
            reason: reason.into(),
        }
    }

    pub(crate) fn unsupported(reason: impl Into<String>) -> Self {
        WakeError::UnsupportedEnvelope {
            reason: reason.into(),
        }
    }

    pub(crate) fn invalid_response(reason: impl Into<String>) -> Self {
        WakeError::InvalidResponse {
            reason: reason.into(),
        }
    }
}

/// The category and position of a JSON error, without its message: serde_json
/// messages quote the offending value, which may be decrypted plaintext.
pub(crate) fn json_error(e: &serde_json::Error) -> String {
    use serde_json::error::Category;
    let kind = match e.classify() {
        Category::Io => "io error",
        Category::Syntax => "syntax error",
        Category::Data => "unexpected type or value",
        Category::Eof => "unexpected end of input",
    };
    format!(
        "invalid json: {kind} at line {} column {}",
        e.line(),
        e.column()
    )
}
