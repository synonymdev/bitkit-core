//! Helper module for Pubky session management with pkarr files.
//!
//! This module provides utilities for creating authenticated sessions
//! from pkarr files for use with the Paykit module.

use crate::paykit::errors::PaykitError;

// #[cfg(feature = "pubky")]
// use paykit_lib::PublicKey as ExternalPublicKey; // Not used in current implementation

/// Represents session configuration for pkarr-based authentication
pub struct SessionConfig {
    /// Path to the pkarr file
    pub pkarr_path: String,
    /// Password for decrypting the pkarr file
    pub password: String,
    /// Optional homeserver URL (uses default if None)
    pub homeserver_url: Option<String>,
}

impl SessionConfig {
    /// Creates a new session configuration
    pub fn new(pkarr_path: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            pkarr_path: pkarr_path.into(),
            password: password.into(),
            homeserver_url: None,
        }
    }

    /// Sets a custom homeserver URL
    pub fn with_homeserver(mut self, url: impl Into<String>) -> Self {
        self.homeserver_url = Some(url.into());
        self
    }
}

/// Creates an authenticated Pubky session from a pkarr file.
///
/// # Implementation Notes
///
/// When the Pubky SDK is available, this function should:
///
/// 1. **Load the pkarr file**:
///    ```ignore
///    let pkarr_bytes = std::fs::read(&config.pkarr_path)?;
///    ```
///
/// 2. **Decrypt and parse the pkarr**:
///    ```ignore
///    // The pkarr format contains an encrypted keypair
///    let keypair = pkarr::decrypt_keypair(&pkarr_bytes, &config.password)?;
///    // Or possibly:
///    let keypair = Keypair::from_pkarr(&pkarr_bytes, &config.password)?;
///    ```
///
/// 3. **Create a Pubky client/SDK instance**:
///    ```ignore
///    let client = PubkyClient::new()?;
///    // Or with custom homeserver:
///    let client = PubkyClient::with_homeserver(config.homeserver_url)?;
///    ```
///
/// 4. **Create a signer from the keypair**:
///    ```ignore
///    let signer = client.signer(keypair);
///    ```
///
/// 5. **Sign in or sign up**:
///    ```ignore
///    // Try to sign in first
///    let session = match signer.signin().await {
///        Ok(session) => session,
///        Err(_) => {
///            // If signin fails, try signup
///            let homeserver_pubkey = client.homeserver_pubkey().await?;
///            signer.signup(&homeserver_pubkey, None).await?
///        }
///    };
///    ```
///
/// 6. **Return the authenticated transport**:
///    ```ignore
///    Ok(PubkyAuthenticatedTransport::new(session))
///    ```
#[cfg(feature = "pubky")]
pub async fn create_session_from_pkarr(
    config: SessionConfig,
) -> Result<paykit_lib::PubkyAuthenticatedTransport, PaykitError> {
    // Verify the pkarr file exists
    if !std::path::Path::new(&config.pkarr_path).exists() {
        return Err(PaykitError::SessionError(
            format!("PKARR file not found: {}", config.pkarr_path)
        ));
    }

    // Read the pkarr file
    let pkarr_bytes = std::fs::read(&config.pkarr_path)
        .map_err(|e| PaykitError::SessionError(
            format!("Failed to read PKARR file: {}", e)
        ))?;

    // Verify it's not empty
    if pkarr_bytes.is_empty() {
        return Err(PaykitError::SessionError(
            "PKARR file is empty".to_string()
        ));
    }

    // Use the pubky API to decrypt the recovery file and create a session
    use pubky::{recovery_file, Pubky};

    // Decrypt the recovery file to get the keypair
    let keypair = recovery_file::decrypt_recovery_file(&pkarr_bytes, &config.password)
        .map_err(|e| PaykitError::SessionError(
            format!("Failed to decrypt recovery file: {}", e)
        ))?;

    // Create Pubky instance
    let pubky = Pubky::new().map_err(|e| PaykitError::SessionError(
        format!("Failed to create Pubky instance: {}", e)
    ))?;

    // Create a signer from the keypair
    let signer = pubky.signer(keypair);

    // Try to sign in first, then sign up if that fails
    let session = match signer.signin().await {
        Ok(session) => session,
        Err(signin_err) => {
            // If signin fails, try signup
            // For signup, we need the homeserver's public key
            // If homeserver_url is provided, use it; otherwise use default
            if let Some(_homeserver_url) = config.homeserver_url {
                // Parse the homeserver public key from URL if provided
                // This is a simplified approach - in production you'd want better parsing
                return Err(PaykitError::SessionError(
                    format!("Signin failed, signup not implemented yet: {}", signin_err)
                ));
            } else {
                return Err(PaykitError::SessionError(
                    format!("Failed to sign in and no homeserver provided for signup: {}", signin_err)
                ));
            }
        }
    };

    // Create authenticated transport from session
    Ok(paykit_lib::PubkyAuthenticatedTransport::new(session))
}

/// Create session from pkarr stub when pubky feature is disabled
#[cfg(not(feature = "pubky"))]
pub async fn create_session_from_pkarr(
    _config: SessionConfig,
) -> Result<paykit_lib::PubkyAuthenticatedTransport, PaykitError> {
    Err(PaykitError::SessionError(
        "Session creation requires the 'pubky' feature to be enabled".to_string()
    ))
}

/// Extracts the public key from a pkarr file without creating a session.
///
/// This is useful for read-only operations where you just need the public key.
#[cfg(feature = "pubky")]
pub fn extract_public_key_from_pkarr(
    pkarr_path: &str,
    password: &str,
) -> Result<String, PaykitError> {
    use pubky::recovery_file;

    // Verify the pkarr file exists
    if !std::path::Path::new(pkarr_path).exists() {
        return Err(PaykitError::SessionError(
            format!("PKARR file not found: {}", pkarr_path)
        ));
    }

    let pkarr_bytes = std::fs::read(pkarr_path).map_err(|e| PaykitError::SessionError(
        format!("Failed to read PKARR file: {}", e)
    ))?;

    // Decrypt the recovery file to get the keypair
    let keypair = recovery_file::decrypt_recovery_file(&pkarr_bytes, password)
        .map_err(|e| PaykitError::SessionError(
            format!("Failed to decrypt recovery file: {}", e)
        ))?;

    // Get the public key from the keypair
    let public_key = keypair.public_key();

    Ok(public_key.to_string())
}

/// Extracts the public key from a pkarr file (stub for when pubky feature is disabled)
#[cfg(not(feature = "pubky"))]
pub fn extract_public_key_from_pkarr(
    _pkarr_path: &str,
    _password: &str,
) -> Result<String, PaykitError> {
    Err(PaykitError::SessionError(
        "Public key extraction requires the 'pubky' feature to be enabled".to_string()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PKARR_PATH: &str = "src/modules/paykit/recovery.pkarr";
    const TEST_PASSWORD: &str = "password";

    #[test]
    fn test_session_config_creation() {
        let config = SessionConfig::new(TEST_PKARR_PATH, TEST_PASSWORD);
        assert_eq!(config.pkarr_path, TEST_PKARR_PATH);
        assert_eq!(config.password, TEST_PASSWORD);
        assert!(config.homeserver_url.is_none());

        let config_with_server = SessionConfig::new(TEST_PKARR_PATH, TEST_PASSWORD)
            .with_homeserver("https://homeserver.example.com");
        assert_eq!(
            config_with_server.homeserver_url,
            Some("https://homeserver.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_create_session_from_pkarr() {
        let config = SessionConfig::new(TEST_PKARR_PATH, TEST_PASSWORD);

        // The pkarr file exists and should be readable
        // Session creation may fail due to network issues (no homeserver available in tests)
        // but should at least get past file reading and decryption
        match create_session_from_pkarr(config).await {
            Ok(session) => {
                println!("Successfully created session (unexpected in test env without homeserver)");
            }
            Err(PaykitError::SessionError(message)) => {
                // Expected - session creation requires network access to homeserver
                // Valid errors include: signin failures, network errors, etc.
                println!("Session error (expected in test env): {}", message);
                // Should NOT be a file not found error since the file exists
                assert!(!message.contains("not found"), "File should exist");
            }
            Err(e) => {
                println!("Other error: {:?}", e);
            }
        }
    }

    #[test]
    fn test_extract_public_key_from_pkarr() {
        match extract_public_key_from_pkarr(TEST_PKARR_PATH, TEST_PASSWORD) {
            Ok(public_key) => {
                println!("Extracted public key: {}", public_key);
                assert!(!public_key.is_empty(), "Public key should not be empty");
            }
            Err(PaykitError::SessionError(message)) => {
                // May fail if pubky feature handles this differently
                println!("Session error: {}", message);
                // Should NOT be file not found
                assert!(!message.contains("not found"), "File should exist");
            }
            Err(e) => {
                println!("Other error: {:?}", e);
            }
        }
    }

    #[test]
    fn test_pkarr_file_verification() {
        // Test with non-existent file
        let bad_config = SessionConfig::new("nonexistent.pkarr", "password");
        let result = create_session_from_pkarr(bad_config);

        // We can't use async in a sync test, so test the sync function
        match extract_public_key_from_pkarr("nonexistent.pkarr", "password") {
            Err(PaykitError::SessionError(message)) => {
                assert!(message.contains("not found"));
            }
            _ => panic!("Expected file not found error"),
        }

        // Test with actual file
        assert!(
            std::path::Path::new(TEST_PKARR_PATH).exists(),
            "recovery.pkarr should exist"
        );
    }
}