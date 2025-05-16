//! # trezor-connect-rs
//!
//! A Rust library for generating Trezor Connect deep links according to the
//! Trezor Suite Lite deep linking specification.

use std::fmt::format;
use serde::{Serialize};
use serde_json;
use url::Url;
use uuid::Uuid;
use crate::modules::trezor::{AccountInfoResponse, AddressResponse, DeepLinkResult, FeatureResponse, GetAccountInfoParams, GetAddressParams, GetPublicKeyParams, PublicKeyResponse, TrezorConnectClient, TrezorConnectError, TrezorConnectResult, TrezorResponse, TrezorResponsePayload};
use crate::modules::trezor::types::{TrezorEnvironment, Empty};

impl TrezorEnvironment {
    /// Get the base URL for the selected environment
    pub fn base_url(&self) -> Result<Url, TrezorConnectError> {
        match self {
            TrezorEnvironment::Production => {
                Err(TrezorConnectError::EnvironmentError { error_details: "Production environment is not available".to_string() })
            },
            TrezorEnvironment::Development => {
                Url::parse("https://dev.suite.sldev.cz/connect/develop/deeplink/1/")
                    .map_err(|e| TrezorConnectError::UrlError { error_details: e.to_string() })
            },
            TrezorEnvironment::Local => {
                Url::parse("trezorsuitelite://connect/1/")
                    .map_err(|e| TrezorConnectError::UrlError { error_details: e.to_string() })
            },
        }
    }
}

impl TrezorConnectClient {
    /// Create a new TrezorConnectClient
    pub fn new(environment: TrezorEnvironment, callback_base: impl AsRef<str>) -> Result<Self, url::ParseError> {
        let callback_base = Url::parse(callback_base.as_ref())?;

        Ok(Self {
            environment,
            callback_base,
        })
    }

    /// Generate a deep link for a Trezor Connect method with a specific request ID
    fn generate_deep_link<T: Serialize>(&self, method: &str, params: T, request_id: Option<String>) -> TrezorConnectResult<DeepLinkResult> {
        // Serialize params to JSON
        let params_json = serde_json::to_string(&params)?;

        // Get base URL directly without re-parsing
        let mut url = self.environment.base_url()?;

        // Generate a UUID if request_id is None
        let id = request_id.map(|s| s.to_string()).unwrap_or_else(|| Uuid::new_v4().to_string());

        // Create callback URL with the request ID, preserving existing query params
        let mut callback_url = self.callback_base.clone();
        callback_url.query_pairs_mut().append_pair("id", &id);

        // Add query parameters
        url.query_pairs_mut()
            .append_pair("method", method)
            .append_pair("params", &params_json)
            .append_pair("callback", &callback_url.to_string());

        Ok(DeepLinkResult {
            url: url.to_string(),
            request_id: id,
        })
    }

    /// Get device features
    pub fn get_features(&self, request_id: Option<String>) -> TrezorConnectResult<DeepLinkResult> {
        self.generate_deep_link("getFeatures", Empty {}, request_id)
    }

    /// Get address for the specified path
    pub fn get_address(&self, params: GetAddressParams, request_id: Option<String>) -> TrezorConnectResult<DeepLinkResult> {
        self.generate_deep_link("getAddress", params, request_id)
    }

    /// Get public key for the specified path
    pub fn get_public_key(&self, params: GetPublicKeyParams, request_id: Option<String>) -> TrezorConnectResult<DeepLinkResult> {
        self.generate_deep_link("getPublicKey", params, request_id)
    }

    /// Get account info for the specified parameters
    ///
    /// This method supports three modes of operation:
    /// 1. Using path - provide path and coin
    /// 2. Using descriptor - provide descriptor (public key or address) and coin
    /// 3. Using discovery - provide only coin, BIP-0044 account discovery is performed
    pub fn get_account_info(&self, params: GetAccountInfoParams, request_id: Option<String>) -> TrezorConnectResult<DeepLinkResult> {
        // Validate that at least one valid configuration is present
        if params.path.is_none() && params.descriptor.is_none() {
            // This is discovery mode - just need the coin which is already required
        } else if params.path.is_some() && params.descriptor.is_some() {
            // This is ambiguous - both path and descriptor provided
            return Err(TrezorConnectError::Other {
                error_details: "Both path and descriptor provided. Use either path-based or descriptor-based parameters, not both.".to_string()
            });
        }

        self.generate_deep_link("getAccountInfo", params, request_id)
    }
}

/// Handle a callback URL from Trezor
///
/// This method should be called when your application receives a callback from Trezor,
/// typically from your app's deep link handler.
///
/// # Arguments
/// * `callback_url` - The callback URL that was opened by Trezor
///
/// # Returns
/// * `TrezorConnectResult<TrezorResponseEnum>` - The parsed response from Trezor
pub fn handle_deep_link<S: AsRef<str>>(callback_url: S) -> TrezorConnectResult<TrezorResponsePayload> {
    // Parse the URL
    let parsed_url = Url::parse(callback_url.as_ref())
        .map_err(|e| TrezorConnectError::UrlError {
            error_details: format!("Failed to parse callback URL: {}", e)
        })?;

    // Extract the request ID from the query parameters
    let id = parsed_url.query_pairs()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| TrezorConnectError::Other {
            error_details: "Missing 'id' parameter in callback URL".to_string()
        })?;

    // Extract the response parameter
    let response_param = parsed_url.query_pairs()
        .find(|(key, _)| key == "response")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| TrezorConnectError::Other {
            error_details: "Missing 'response' parameter in callback URL".to_string()
        })?;

    // Parse the response JSON
    let response: TrezorResponse = serde_json::from_str(&response_param)
        .map_err(|e| TrezorConnectError::SerdeError {
            error_details: format!("Failed to parse response JSON: {}", e)
        })?;

    // Check if there was an error
    if !response.success {
        return Err(TrezorConnectError::Other {
            error_details: response.error.unwrap_or_else(|| "Unknown error from Trezor".to_string())
        });
    }

    // Get the payload or return an error
    let payload = response.payload.ok_or_else(|| TrezorConnectError::Other {
        error_details: "Success response but no payload".to_string()
    })?;

    // Extract the method from the URL parameters to help with deserialization
    let method = parsed_url.query_pairs()
        .find(|(key, _)| key == "method")
        .map(|(_, value)| value.to_string());

    // Try to deserialize into the appropriate type based on the method
    match method.as_deref() {
        Some("getFeatures") => {
            let features: FeatureResponse = serde_json::from_value(payload)
                .map_err(|e| TrezorConnectError::SerdeError {
                    error_details: format!("Failed to parse Features response: {}", e)
                })?;
            Ok(TrezorResponsePayload::Features(features))
        },
        Some("getAddress") => {
            let address: AddressResponse = serde_json::from_value(payload)
                .map_err(|e| TrezorConnectError::SerdeError {
                    error_details: format!("Failed to parse Address response: {}", e)
                })?;
            Ok(TrezorResponsePayload::Address(address))
        },
        Some("getPublicKey") => {
            let public_key: PublicKeyResponse = serde_json::from_value(payload)
                .map_err(|e| TrezorConnectError::SerdeError {
                    error_details: format!("Failed to parse PublicKey response: {}", e)
                })?;
            Ok(TrezorResponsePayload::PublicKey(public_key))
        },
        Some("getAccountInfo") => {
            let account_info: AccountInfoResponse = serde_json::from_value(payload)
                .map_err(|e| TrezorConnectError::SerdeError {
                    error_details: format!("Failed to parse AccountInfo response: {}", e)
                })?;
            Ok(TrezorResponsePayload::AccountInfo(account_info))
        },
        _ => {
            Err(TrezorConnectError::Other {
                error_details: format!("Unknown or unsupported method: {:?}", method).to_string()
            })
        }
    }
}