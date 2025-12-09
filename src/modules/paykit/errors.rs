use thiserror::Error;
use paykit_lib::PaykitError as ExternalPaykitError;

/// Domain-specific error type for Paykit operations.
#[derive(Debug, Clone, uniffi::Error, Error)]
#[non_exhaustive]
pub enum PaykitError {
    #[error("Not implemented: {0}")]
    Unimplemented(String),
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("Invalid method ID: {0}")]
    InvalidMethodId(String),
    #[error("Invalid endpoint data: {0}")]
    InvalidEndpointData(String),
    #[error("Session error: {0}")]
    SessionError(String),
}

impl From<ExternalPaykitError> for PaykitError {
    fn from(value: ExternalPaykitError) -> Self {
        match value {
            ExternalPaykitError::Unimplemented(msg) => PaykitError::Unimplemented(msg.to_string()),
            ExternalPaykitError::Transport(msg) => PaykitError::Transport(msg),
        }
    }
}

impl From<PaykitError> for ExternalPaykitError {
    fn from(value: PaykitError) -> Self {
        match value {
            PaykitError::Unimplemented(message) => {
                ExternalPaykitError::Transport(format!("Unimplemented: {}", message))
            }
            PaykitError::Transport(message) => ExternalPaykitError::Transport(message),
            PaykitError::InvalidPublicKey(message) => {
                ExternalPaykitError::Transport(format!("Invalid public key: {}", message))
            }
            PaykitError::InvalidMethodId(message) => {
                ExternalPaykitError::Transport(format!("Invalid method ID: {}", message))
            }
            PaykitError::InvalidEndpointData(message) => {
                ExternalPaykitError::Transport(format!("Invalid endpoint data: {}", message))
            }
            PaykitError::SessionError(message) => {
                ExternalPaykitError::Transport(format!("Session error: {}", message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_conversion() {
        let external_error = ExternalPaykitError::Transport("test error".to_string());
        let paykit_error: PaykitError = external_error.into();

        match paykit_error {
            PaykitError::Transport(message) => assert_eq!(message, "test error"),
            _ => panic!("Wrong error variant"),
        }
    }
}
