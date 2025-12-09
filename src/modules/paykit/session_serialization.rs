//! Session serialization module for passing Pubky sessions through deeplinks.
//!
//! This module provides utilities for converting authenticated sessions to/from
//! strings that can be passed through deeplinks or other communication channels.

use crate::paykit::errors::PaykitError;
use crate::paykit::PubkyAuthenticatedTransport;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

/// Represents serializable session data that can be passed through a deeplink
#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct SessionData {
    /// The user's public key
    pub public_key: String,
    /// The user's secret key (encrypted or encoded)
    pub secret_key: String,
    /// Optional homeserver URL
    pub homeserver_url: Option<String>,
    /// Session expiry timestamp (Unix timestamp)
    pub expires_at: Option<i64>,
    /// Additional metadata
    pub metadata: Option<String>,
}

/// Represents a session token that can be passed through deeplinks
#[derive(Debug, Clone, uniffi::Record)]
pub struct SessionToken {
    /// Base64 URL-safe encoded session data
    pub token: String,
}

impl SessionToken {
    /// Creates a new session token from a string
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// Validates that the token is properly formatted
    pub fn validate(&self) -> Result<(), PaykitError> {
        // Check if it's valid base64
        URL_SAFE_NO_PAD
            .decode(&self.token)
            .map_err(|e| PaykitError::SessionError(
                format!("Invalid session token encoding: {}", e)
            ))?;

        Ok(())
    }
}

/// Serializes session data into a token string suitable for deeplinks.
///
/// # Security Considerations
/// - The secret key should be encrypted before serialization
/// - Use HTTPS/secure channels when transmitting
/// - Consider adding expiration times
/// - Implement token rotation for long-lived sessions
///
/// # Example
/// ```
/// let session_data = SessionData {
///     public_key: "user_public_key".to_string(),
///     secret_key: "encrypted_secret".to_string(),
///     homeserver_url: Some("https://homeserver.example".to_string()),
///     expires_at: Some(1234567890),
///     metadata: None,
/// };
///
/// let token = serialize_session_to_token(session_data)?;
/// // token.token can now be passed through a deeplink
/// ```
#[uniffi::export]
pub fn serialize_session_to_token(session_data: SessionData) -> Result<SessionToken, PaykitError> {
    // Serialize the session data to JSON
    let json = serde_json::to_string(&session_data).map_err(|e| PaykitError::SessionError(
        format!("Failed to serialize session data: {}", e)
    ))?;

    // Encode as base64 URL-safe (no padding) for deeplink compatibility
    let encoded = URL_SAFE_NO_PAD.encode(json.as_bytes());

    Ok(SessionToken::new(encoded))
}

/// Deserializes a session token back into session data.
///
/// # Example
/// ```
/// let token = SessionToken::new("base64_encoded_session_data");
/// let session_data = deserialize_token_to_session(token)?;
/// // Now you have the session data back
/// ```
#[uniffi::export]
pub fn deserialize_token_to_session(token: SessionToken) -> Result<SessionData, PaykitError> {
    // Validate the token first
    token.validate()?;

    // Decode from base64
    let decoded = URL_SAFE_NO_PAD
        .decode(&token.token)
        .map_err(|e| PaykitError::SessionError(
            format!("Failed to decode session token: {}", e)
        ))?;

    // Parse JSON
    let session_data: SessionData =
        serde_json::from_slice(&decoded).map_err(|e| PaykitError::SessionError(
            format!("Failed to parse session data: {}", e)
        ))?;

    Ok(session_data)
}

/// Creates an authenticated transport from a session token received via deeplink.
///
/// This is the main entry point for reconstructing a session from a deeplink.
///
/// # Flow
/// 1. App receives deeplink with session token
/// 2. Extract token from deeplink URL
/// 3. Call this function to create authenticated transport
/// 4. Use transport for paykit operations
///
/// # Example Deeplink Format
/// ```
/// myapp://paykit/session?token=eyJwdWJsaWNfa2V5IjoiLi4uIiwic2VjcmV0X2tleSI6Ii4uLiJ9
/// ```
#[cfg(feature = "pubky")]
pub async fn create_transport_from_session_token(
    token: SessionToken,
) -> Result<PubkyAuthenticatedTransport, PaykitError> {
    use paykit_lib::PubkyAuthenticatedTransport as ExternalTransport;
    use pubky::{Keypair, Pubky};

    // Deserialize the session data
    let session_data = deserialize_token_to_session(token)?;

    // Check expiration if present
    if let Some(expires_at) = session_data.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| PaykitError::SessionError(format!("System time error: {}", e)))?
            .as_secs() as i64;

        if now > expires_at {
            return Err(PaykitError::SessionError("Session token has expired".to_string()));
        }
    }

    // Create Pubky instance
    let pubky = Pubky::new().map_err(|e| PaykitError::SessionError(
        format!("Failed to create Pubky instance: {}", e)
    ))?;

    // Reconstruct the keypair from the session data
    // Decode the secret key from hex or base64
    let secret_key_bytes = hex::decode(&session_data.secret_key)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&session_data.secret_key))
        .map_err(|e| PaykitError::SessionError(format!("Failed to decode secret key: {}", e)))?;

    // Create keypair from the secret key bytes
    // Note: from_secret_key expects a 32-byte array
    if secret_key_bytes.len() != 32 {
        return Err(PaykitError::SessionError(
            format!("Invalid secret key length: expected 32 bytes, got {}", secret_key_bytes.len())
        ));
    }
    let mut secret_key_array = [0u8; 32];
    secret_key_array.copy_from_slice(&secret_key_bytes);

    let keypair = Keypair::from_secret_key(&secret_key_array);

    // Create signer with the keypair
    let signer = pubky.signer(keypair);

    // Sign in to create a session
    let session = signer.signin().await.map_err(|e| PaykitError::SessionError(
        format!("Failed to sign in: {}", e)
    ))?;

    // Create authenticated transport from session
    Ok(ExternalTransport::new(session).into())
}

/// Creates an authenticated transport from a session token (non-pubky stub)
#[cfg(not(feature = "pubky"))]
pub async fn create_transport_from_session_token(
    _token: SessionToken,
) -> Result<PubkyAuthenticatedTransport, PaykitError> {
    Err(PaykitError::SessionError(
        "Session creation requires the 'pubky' feature to be enabled".to_string()
    ))
}

/// Helper function to create a session token from raw keypair data.
///
/// This is useful when you have the keypair but need to create a shareable token.
#[uniffi::export]
pub fn create_session_token_from_keypair(
    public_key: String,
    secret_key: String,
    homeserver_url: Option<String>,
    expires_in_seconds: Option<i64>,
) -> Result<SessionToken, PaykitError> {
    let expires_at = expires_in_seconds.map(|seconds| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        now + seconds
    });

    let session_data = SessionData {
        public_key,
        secret_key,
        homeserver_url,
        expires_at,
        metadata: None,
    };

    serialize_session_to_token(session_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_token_serialization_roundtrip() {
        let session_data = SessionData {
            public_key: "test_public_key_123".to_string(),
            secret_key: "test_secret_key_456".to_string(),
            homeserver_url: Some("https://example.com".to_string()),
            expires_at: Some(1234567890),
            metadata: Some("test_metadata".to_string()),
        };

        // Serialize to token
        let token = serialize_session_to_token(session_data.clone()).unwrap();
        assert!(!token.token.is_empty());

        // Deserialize back
        let deserialized = deserialize_token_to_session(token).unwrap();
        assert_eq!(deserialized.public_key, session_data.public_key);
        assert_eq!(deserialized.secret_key, session_data.secret_key);
        assert_eq!(deserialized.homeserver_url, session_data.homeserver_url);
        assert_eq!(deserialized.expires_at, session_data.expires_at);
        assert_eq!(deserialized.metadata, session_data.metadata);
    }

    #[test]
    fn test_session_token_validation() {
        // Valid token
        let valid_token = SessionToken::new("eyJwdWJsaWNfa2V5IjoidGVzdCJ9");
        assert!(valid_token.validate().is_ok());

        // Invalid token (not base64)
        let invalid_token = SessionToken::new("not-base64!@#$%");
        assert!(invalid_token.validate().is_err());
    }

    #[test]
    fn test_create_token_with_expiration() {
        let token = create_session_token_from_keypair(
            "public_key".to_string(),
            "secret_key".to_string(),
            None,
            Some(3600), // 1 hour
        )
        .unwrap();

        let session_data = deserialize_token_to_session(token).unwrap();
        assert!(session_data.expires_at.is_some());
    }
}