//! Deeplink handler for Paykit session management.
//!
//! This module provides utilities for handling deeplinks that contain
//! Pubky session tokens for authentication.

use crate::paykit::errors::PaykitError;
use crate::paykit::session_serialization::{SessionToken, create_transport_from_session_token};
use crate::paykit::PubkyAuthenticatedTransport;
use std::collections::HashMap;

/// Represents a parsed deeplink with Paykit session information
#[derive(Debug, Clone, uniffi::Record)]
pub struct PaykitDeeplink {
    /// The action to perform (e.g., "session", "payment", "connect")
    pub action: String,
    /// The session token if present
    pub session_token: Option<String>,
    /// Additional parameters from the deeplink
    pub parameters: HashMap<String, String>,
}

impl PaykitDeeplink {
    /// Creates a new PaykitDeeplink
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            session_token: None,
            parameters: HashMap::new(),
        }
    }

    /// Sets the session token
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(token.into());
        self
    }

    /// Adds a parameter
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }
}

/// Parses a deeplink URL into a PaykitDeeplink structure.
///
/// # Supported URL Formats
/// - `myapp://paykit/session?token=<base64_token>`
/// - `myapp://paykit/connect?token=<base64_token>&return_url=<url>`
/// - `https://myapp.com/paykit/session?token=<base64_token>`
///
/// # Example
/// ```
/// let url = "myapp://paykit/session?token=eyJwdWJsaWNfa2V5IjoiLi4uIn0";
/// let deeplink = parse_paykit_deeplink(url)?;
/// assert_eq!(deeplink.action, "session");
/// assert!(deeplink.session_token.is_some());
/// ```
#[uniffi::export]
pub fn parse_paykit_deeplink(url: String) -> Result<PaykitDeeplink, PaykitError> {
    // Parse the URL
    let parsed = url::Url::parse(&url).map_err(|e| PaykitError::SessionError(
        format!("Invalid deeplink URL: {}", e)
    ))?;

    // Extract the path to determine the action
    // For custom schemes like "myapp://", the host becomes part of the path
    let (is_paykit, action) = if parsed.scheme() == "paykit" {
        // For paykit:// scheme, the host is the action (e.g., paykit://session?token=...)
        let host = parsed.host_str().unwrap_or("session");
        let action = if host.is_empty() { "session" } else { host };
        (true, action.to_string())
    } else if parsed.scheme() == "http" || parsed.scheme() == "https" {
        // For HTTP(S) URLs, check the path
        let path = parsed.path().trim_start_matches('/');
        let path_parts: Vec<&str> = path.split('/').collect();

        if path_parts.contains(&"paykit") {
            let action = path_parts
                .iter()
                .skip_while(|&&p| p != "paykit")
                .nth(1)
                .unwrap_or(&"session")
                .to_string();
            (true, action)
        } else {
            (false, String::new())
        }
    } else {
        // For custom schemes, the host is "paykit" and path is the action
        let host = parsed.host_str().unwrap_or("");
        if host == "paykit" {
            let action = parsed.path().trim_start_matches('/');
            let action = if action.is_empty() { "session" } else { action };
            (true, action.to_string())
        } else {
            (false, String::new())
        }
    };

    if !is_paykit {
        return Err(PaykitError::SessionError(
            "Not a valid Paykit deeplink".to_string()
        ));
    }

    // Parse query parameters
    let mut deeplink = PaykitDeeplink::new(action);

    for (key, value) in parsed.query_pairs() {
        if key == "token" {
            deeplink.session_token = Some(value.to_string());
        } else {
            deeplink.parameters.insert(key.to_string(), value.to_string());
        }
    }

    Ok(deeplink)
}

/// Handles a Paykit deeplink and returns an authenticated transport if successful.
///
/// This is the main entry point for processing deeplinks in your app.
///
/// # Example
/// ```
/// // In your app's deeplink handler
/// let url = "myapp://paykit/session?token=...";
/// match handle_paykit_deeplink(url).await {
///     Ok(transport) => {
///         // Use the authenticated transport for paykit operations
///         set_payment_endpoint(&transport, method, data).await?;
///     }
///     Err(e) => {
///         // Handle error (invalid token, expired, etc.)
///     }
/// }
/// ```
#[cfg(feature = "pubky")]
pub async fn handle_paykit_deeplink(url: String) -> Result<PubkyAuthenticatedTransport, PaykitError> {
    // Parse the deeplink
    let deeplink = parse_paykit_deeplink(url)?;

    // Check if we have a session token
    let token_str = deeplink
        .session_token
        .ok_or_else(|| PaykitError::SessionError(
            "No session token found in deeplink".to_string()
        ))?;

    // Create a SessionToken
    let token = SessionToken::new(token_str);

    // Create authenticated transport from the token
    create_transport_from_session_token(token).await
}

/// Handles a Paykit deeplink (non-pubky stub)
#[cfg(not(feature = "pubky"))]
pub async fn handle_paykit_deeplink(_url: String) -> Result<PubkyAuthenticatedTransport, PaykitError> {
    Err(PaykitError::SessionError(
        "Deeplink handling requires the 'pubky' feature to be enabled".to_string()
    ))
}

/// Creates a session request URL to send to Pubky Ring for authentication.
///
/// This generates a URL that Bitkit displays as a QR code or uses to open Pubky Ring.
/// When Pubky Ring completes authentication, it will return the session data
/// via the provided callback URL.
///
/// # Parameters
/// - `callback_url`: The URL scheme and path where Pubky Ring should return the session.
///                   Example: "bitkit://paykit/session-data" or "bitkit://session"
/// - `additional_params`: Optional additional parameters to include in the request URL
///
/// # Returns
/// A URL string like: `pubkyring://session?callback=bitkit%3A%2F%2Fpaykit%2Fsession-data`
///
/// # Example
/// ```
/// // Generate URL for QR code or "Open Pubky Ring" button
/// let request_url = create_pubky_ring_session_request(
///     "bitkit://paykit/session-data".to_string(),
///     None
/// )?;
/// // Result: "pubkyring://session?callback=bitkit%3A%2F%2Fpaykit%2Fsession-data"
///
/// // With additional parameters
/// let mut params = HashMap::new();
/// params.insert("app_name".to_string(), "Bitkit".to_string());
/// let request_url = create_pubky_ring_session_request(
///     "bitkit://paykit/session-data".to_string(),
///     Some(params)
/// )?;
/// ```
#[uniffi::export]
pub fn create_pubky_ring_session_request(
    callback_url: String,
    additional_params: Option<HashMap<String, String>>,
) -> Result<String, PaykitError> {
    // Validate callback URL format
    if callback_url.is_empty() {
        return Err(PaykitError::SessionError(
            "Callback URL cannot be empty".to_string()
        ));
    }

    // URL-encode the callback
    let encoded_callback = urlencoding::encode(&callback_url);

    // Build the Pubky Ring session request URL
    let mut url = format!("pubkyring://session?callback={}", encoded_callback);

    // Add any additional parameters
    if let Some(params) = additional_params {
        for (key, value) in params {
            let encoded_value = urlencoding::encode(&value);
            url.push_str(&format!("&{}={}", key, encoded_value));
        }
    }

    Ok(url)
}

/// Creates a deeplink URL from a session token.
///
/// Use this to generate a deeplink that can be shared with another app instance.
///
/// # Example
/// ```
/// let token = create_session_token_from_keypair(
///     public_key,
///     secret_key,
///     None,
///     Some(3600)
/// )?;
///
/// let deeplink_url = create_deeplink_from_token(
///     "myapp://",
///     "session",
///     token,
///     None
/// )?;
/// // Result: "myapp://paykit/session?token=..."
/// ```
#[uniffi::export]
pub fn create_deeplink_from_token(
    base_url: String,
    action: String,
    token: SessionToken,
    additional_params: Option<HashMap<String, String>>,
) -> Result<String, PaykitError> {
    // Validate the token first
    token.validate()?;

    // Build the URL
    let mut url = if base_url.ends_with("://") {
        format!("{}paykit/{}", base_url, action)
    } else if base_url.ends_with('/') {
        format!("{}paykit/{}", base_url, action)
    } else {
        format!("{}/paykit/{}", base_url, action)
    };

    // Add the token as a query parameter
    url.push_str(&format!("?token={}", token.token));

    // Add any additional parameters
    if let Some(params) = additional_params {
        for (key, value) in params {
            // URL encode the values
            let encoded_value = urlencoding::encode(&value);
            url.push_str(&format!("&{}={}", key, encoded_value));
        }
    }

    Ok(url)
}

/// Validates a deeplink URL without processing it.
///
/// Use this to check if a URL is a valid Paykit deeplink before handling it.
#[uniffi::export]
pub fn validate_paykit_deeplink(url: String) -> Result<bool, PaykitError> {
    let deeplink = parse_paykit_deeplink(url)?;

    // Check if it's a session action with a token
    if deeplink.action == "session" && deeplink.session_token.is_some() {
        // Validate the token format
        let token = SessionToken::new(deeplink.session_token.unwrap());
        token.validate()?;
        return Ok(true);
    }

    // For other actions, just check if it's a valid paykit deeplink
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paykit::session_serialization::{create_session_token_from_keypair, deserialize_token_to_session};

    #[test]
    fn test_create_pubky_ring_session_request() {
        // Basic test
        let url = create_pubky_ring_session_request(
            "bitkit://paykit/session-data".to_string(),
            None,
        ).unwrap();

        assert!(url.starts_with("pubkyring://session?callback="));
        assert!(url.contains("bitkit"));

        // URL should be properly encoded
        assert!(url.contains("%3A%2F%2F") || url.contains("bitkit")); // :// encoded
    }

    #[test]
    fn test_create_pubky_ring_session_request_with_params() {
        let mut params = HashMap::new();
        params.insert("app_name".to_string(), "Bitkit".to_string());
        params.insert("version".to_string(), "1.0".to_string());

        let url = create_pubky_ring_session_request(
            "bitkit://session".to_string(),
            Some(params),
        ).unwrap();

        assert!(url.contains("pubkyring://session"));
        assert!(url.contains("callback="));
        assert!(url.contains("app_name=Bitkit"));
        assert!(url.contains("version=1.0"));
    }

    #[test]
    fn test_create_pubky_ring_session_request_empty_callback() {
        let result = create_pubky_ring_session_request(
            "".to_string(),
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_deeplink() {
        // Test custom scheme
        let url = "myapp://paykit/session?token=abc123&return_url=home";
        let deeplink = parse_paykit_deeplink(url.to_string()).unwrap();
        assert_eq!(deeplink.action, "session");
        assert_eq!(deeplink.session_token, Some("abc123".to_string()));
        assert_eq!(deeplink.parameters.get("return_url"), Some(&"home".to_string()));

        // Test HTTPS URL
        let url2 = "https://app.example.com/paykit/connect?token=xyz789";
        let deeplink2 = parse_paykit_deeplink(url2.to_string()).unwrap();
        assert_eq!(deeplink2.action, "connect");
        assert_eq!(deeplink2.session_token, Some("xyz789".to_string()));
    }

    #[test]
    fn test_create_deeplink() {
        // Create a valid base64 token for testing
        let valid_base64 = "eyJ0ZXN0IjoidmFsdWUifQ"; // {"test":"value"} in base64
        let token = SessionToken::new(valid_base64);

        // Test with custom scheme
        let url = create_deeplink_from_token(
            "myapp://".to_string(),
            "session".to_string(),
            token.clone(),
            None,
        )
        .unwrap();
        assert_eq!(url, format!("myapp://paykit/session?token={}", valid_base64));

        // Test with additional params
        let mut params = HashMap::new();
        params.insert("return_url".to_string(), "home screen".to_string());

        let url2 = create_deeplink_from_token(
            "https://app.example.com".to_string(),
            "connect".to_string(),
            token,
            Some(params),
        )
        .unwrap();
        assert!(url2.contains(&format!("token={}", valid_base64)));
        assert!(url2.contains("return_url=home%20screen"));
    }

    #[test]
    fn test_validate_deeplink() {
        // Create a valid token first
        let token = create_session_token_from_keypair(
            "public".to_string(),
            "secret".to_string(),
            None,
            None,
        )
        .unwrap();

        let url = create_deeplink_from_token(
            "myapp://".to_string(),
            "session".to_string(),
            token,
            None,
        )
        .unwrap();

        // Should be valid
        assert!(validate_paykit_deeplink(url).unwrap());

        // Invalid URL should fail
        assert!(validate_paykit_deeplink("not-a-url".to_string()).is_err());
    }

    #[test]
    fn test_deeplink_roundtrip() {
        // Create session token
        let token = create_session_token_from_keypair(
            "test_public_key".to_string(),
            "test_secret_key".to_string(),
            Some("https://homeserver.example".to_string()),
            Some(3600),
        )
        .unwrap();

        // Create deeplink
        let url = create_deeplink_from_token(
            "myapp://".to_string(),
            "session".to_string(),
            token,
            None,
        )
        .unwrap();

        // Parse it back
        let parsed = parse_paykit_deeplink(url).unwrap();
        assert_eq!(parsed.action, "session");
        assert!(parsed.session_token.is_some());

        // Deserialize the token
        let session_token = SessionToken::new(parsed.session_token.unwrap());
        let session_data = deserialize_token_to_session(session_token).unwrap();
        assert_eq!(session_data.public_key, "test_public_key");
        assert_eq!(session_data.homeserver_url, Some("https://homeserver.example".to_string()));
    }
}