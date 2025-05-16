#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::modules::trezor::{handle_deep_link, AccountInfoDetails, DefaultAccountType, GetAccountInfoParams, GetAddressParams, GetPublicKeyParams, TokenFilter, TrezorConnectClient, TrezorConnectError, TrezorEnvironment, TrezorResponsePayload};
    use super::*;

    #[test]
    fn test_get_features_with_id() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let result = client.get_features(Some("123".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getFeatures"));
        // The ID is now in the callback URL, not the main URL query params
        assert!(result.url.contains("callback=exampleapp"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3D123"));
        assert_eq!(result.request_id, "123");
    }

    #[test]
    fn test_get_features_auto_id() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let result = client.get_features(None).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getFeatures"));
        assert!(result.url.contains("callback=exampleapp"));
        // The ID is now in the callback URL, and we just check it contains a callback with id param
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3D"));
        assert!(!result.request_id.is_empty());
        // UUID validation (ensuring it's in the proper format)
        assert_eq!(result.request_id.len(), 36);
    }

    #[test]
    fn test_get_address() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = GetAddressParams {
            path: "m/44'/0'/0'/0/0".to_string(),
            address: None,
            showOnTrezor: Some(true),
            chunkify: None,
            useEventListener: None,
            coin: Some("btc".to_string()),
            crossChain: None,
            multisig: None,
            scriptType: None,
            unlockPath: None,
            common: None,
        };

        let result = client.get_address(params, Some("456".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getAddress"));
        assert!(result.url.contains("path"));
        // Check for encoded callback URL with ID
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3D456"));
        assert_eq!(result.request_id, "456");
    }

    #[test]
    fn test_get_public_key() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = GetPublicKeyParams {
            path: "m/49'/0'/0'".to_string(),
            showOnTrezor: Some(true),
            suppressBackupWarning: None,
            chunkify: None,
            coin: Some("btc".to_string()),
            crossChain: None,
            scriptType: None,
            ignoreXpubMagic: None,
            ecdsaCurveName: None,
            unlockPath: None,
            common: None,
        };

        let result = client.get_public_key(params, Some("789".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getPublicKey"));
        assert!(result.url.contains("path"));
        // Check for encoded callback URL with ID
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3D789"));
        assert_eq!(result.request_id, "789");
    }

    #[test]
    fn test_get_account_info_with_path() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = GetAccountInfoParams {
            path: Some("m/49'/0'/0'".to_string()),
            descriptor: None,
            coin: "btc".to_string(),
            details: None,
            tokens: None,
            page: None,
            pageSize: None,
            from: None,
            to: None,
            gap: None,
            contractFilter: None,
            marker: None,
            defaultAccountType: None,
            suppressBackupWarning: None,
            common: None,
        };

        let result = client.get_account_info(params, Some("account1".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getAccountInfo"));
        assert!(result.url.contains("path"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Daccount1"));
        assert_eq!(result.request_id, "account1");
    }

    #[test]
    fn test_get_account_info_with_descriptor() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = GetAccountInfoParams {
            path: None,
            descriptor: Some("xpub6CVKsQYXc9awxgV1tWbG4foDvdcnieK2JkbpPEBKB5WwAPKBZ1mstLbKVB4ov7QzxzjaxNK6EfmNY5Jsk2cG26EVcEkycGW4tchT2dyUhrx".to_string()),
            coin: "btc".to_string(),
            details: None,
            tokens: None,
            page: None,
            pageSize: None,
            from: None,
            to: None,
            gap: None,
            contractFilter: None,
            marker: None,
            defaultAccountType: None,
            suppressBackupWarning: None,
            common: None,
        };

        let result = client.get_account_info(params, Some("account2".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getAccountInfo"));
        assert!(result.url.contains("descriptor"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Daccount2"));
        assert_eq!(result.request_id, "account2");
    }

    #[test]
    fn test_get_account_info_discovery() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = GetAccountInfoParams {
            path: None,
            descriptor: None,
            coin: "btc".to_string(),
            details: None,
            tokens: None,
            page: None,
            pageSize: None,
            from: None,
            to: None,
            gap: None,
            contractFilter: None,
            marker: None,
            defaultAccountType: Some(DefaultAccountType::Segwit),
            suppressBackupWarning: Some(true),
            common: None,
        };

        let result = client.get_account_info(params, Some("account3".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getAccountInfo"));
        assert!(!result.url.contains("path"));
        assert!(!result.url.contains("descriptor"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("defaultAccountType"));
        assert!(result.url.contains("suppressBackupWarning"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Daccount3"));
        assert_eq!(result.request_id, "account3");
    }

    #[test]
    fn test_get_account_info_with_details() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = GetAccountInfoParams {
            path: Some("m/49'/0'/0'".to_string()),
            descriptor: None,
            coin: "btc".to_string(),
            details: Some(AccountInfoDetails::Txs),
            tokens: Some(TokenFilter::Used),
            page: Some(1),
            pageSize: Some(25),
            from: Some(650000),
            to: Some(670000),
            gap: Some(20),
            contractFilter: None,
            marker: None,
            defaultAccountType: None,
            suppressBackupWarning: None,
            common: None,
        };

        let result = client.get_account_info(params, Some("account4".to_string())).unwrap();
        println!("Debug URL: {}", result.url);
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=getAccountInfo"));
        assert!(result.url.contains("%22details%22%3A%22txs%22"));
        assert!(result.url.contains("%22tokens%22%3A%22used%22"));
        assert!(result.url.contains("%22page%22%3A1"));
        assert!(result.url.contains("%22pageSize%22%3A25"));
        assert!(result.url.contains("%22from%22%3A650000"));
        assert!(result.url.contains("%22to%22%3A670000"));
        assert!(result.url.contains("%22gap%22%3A20"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Daccount4"));
        assert_eq!(result.request_id, "account4");
    }

    #[test]
    fn test_get_account_info_invalid_params() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        // Both path and descriptor provided (invalid)
        let params = GetAccountInfoParams {
            path: Some("m/49'/0'/0'".to_string()),
            descriptor: Some("xpub6CVKsQYXc9awxgV1tWbG4foDvdcnieK2JkbpPEBKB5WwAPKBZ1mstLbKVB4ov7QzxzjaxNK6EfmNY5Jsk2cG26EVcEkycGW4tchT2dyUhrx".to_string()),
            coin: "btc".to_string(),
            details: None,
            tokens: None,
            page: None,
            pageSize: None,
            from: None,
            to: None,
            gap: None,
            contractFilter: None,
            marker: None,
            defaultAccountType: None,
            suppressBackupWarning: None,
            common: None,
        };

        let result = client.get_account_info(params, Some("account_invalid".to_string()));
        assert!(result.is_err());
        if let Err(TrezorConnectError::Other { error_details }) = result {
            assert!(error_details.contains("Both path and descriptor provided"));
        } else {
            panic!("Expected TrezorConnectError::Other but got something else");
        }
    }

    #[test]
    fn test_preserves_existing_query() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback?foo=bar"
        ).unwrap();

        let result = client.get_features(Some("xyz".to_string())).unwrap();
        // The callback URL should contain both the original foo=bar and the id
        assert!(result.url.contains("callback=exampleapp"));
        // URL encoded version of ?foo=bar&id=xyz
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Ffoo%3Dbar%26id%3Dxyz"));
        assert_eq!(result.request_id, "xyz");
    }

    #[test]
    fn test_environment_urls() {
        // Test Local environment
        let local_url = TrezorEnvironment::Local.base_url().unwrap();
        assert_eq!(local_url.as_str(), "trezorsuitelite://connect/1/");

        // Test Development environment
        let dev_url = TrezorEnvironment::Development.base_url().unwrap();
        assert_eq!(dev_url.as_str(), "https://dev.suite.sldev.cz/connect/develop/deeplink/1/");

        // Test Production environment (should return an error)
        let prod_result = TrezorEnvironment::Production.base_url();
        assert!(prod_result.is_err());
        if let Err(TrezorConnectError::EnvironmentError { error_details }) = prod_result {
            assert_eq!(error_details, "Production environment is not available");
        } else {
            panic!("Expected EnvironmentError but got something else");
        }
    }

    #[test]
    fn test_client_creation_error() {
        // Test with invalid URL
        let result = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "invalid-url"
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_deep_link_features() {
        // Create a mock response URL that would come from a getFeatures call
        let features_json = json!({
            "vendor": "trezor.io",
            "major_version": 2,
            "minor_version": 7,
            "patch_version": 0,
            "device_id": "DEVICE12345",
            "capabilities": ["Capability_Bitcoin", "Capability_Ethereum"]
        });

        let response_json = json!({
            "success": true,
            "payload": features_json
        });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=test123&method=getFeatures&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::Features(features) => {
                assert_eq!(features.vendor, "trezor.io");
                assert_eq!(features.major_version, 2);
                assert_eq!(features.minor_version, 7);
                assert_eq!(features.patch_version, 0);
                assert_eq!(features.device_id, "DEVICE12345");
                assert!(features.capabilities.is_some());
                assert_eq!(features.capabilities.unwrap().len(), 2);
            },
            _ => panic!("Expected Features payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_address() {
        // Create a mock response URL that would come from a getAddress call
        let address_json = json!({
            "address": "1BgGZ9tcN4rm9KBzDn7KprQz87SZ26SAMH",
            "path": [44, 0, 0, 0, 0],
            "serializedPath": "m/44'/0'/0'/0/0"
        });

        let response_json = json!({
            "success": true,
            "payload": address_json
        });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=addr123&method=getAddress&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::Address(address) => {
                assert_eq!(address.address, "1BgGZ9tcN4rm9KBzDn7KprQz87SZ26SAMH");
                assert_eq!(address.path, vec![44, 0, 0, 0, 0]);
                assert_eq!(address.serializedPath, "m/44'/0'/0'/0/0");
            },
            _ => panic!("Expected Address payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_public_key() {
        // Create a mock response URL that would come from a getPublicKey call
        let pubkey_json = json!({
            "path": [49, 0, 0],
            "serializedPath": "m/49'/0'/0'",
            "xpub": "xpub6BosfCnifzxcFwrSzQiqu2DBVTshkCXacvNsWGYJVVhhawA7d4R5WSWGFNbi8Aw6ZRc1brxMyWMzG3DSSSSoekkudhUd9yLb6qx39T9nMdj",
            "chainCode": "1f66e8e24ed26a59bdd7c586b14e9f3d2114506a03114f62f766d42701c091e1",
            "childNum": 0,
            "publicKey": "03a1af804ac108a8a51782198c2d034b28bf90c8803f5a53f76276fa69a4eae77f",
            // Fixed fingerprint value to be within i32 range
            "fingerprint": 2147483647,
            "depth": 3
        });

        let response_json = json!({
            "success": true,
            "payload": pubkey_json
        });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=pubkey123&method=getPublicKey&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::PublicKey(pubkey) => {
                assert_eq!(pubkey.path, vec![49, 0, 0]);
                assert_eq!(pubkey.serializedPath, "m/49'/0'/0'");
                assert_eq!(pubkey.xpub, "xpub6BosfCnifzxcFwrSzQiqu2DBVTshkCXacvNsWGYJVVhhawA7d4R5WSWGFNbi8Aw6ZRc1brxMyWMzG3DSSSSoekkudhUd9yLb6qx39T9nMdj");
                assert_eq!(pubkey.chainCode, "1f66e8e24ed26a59bdd7c586b14e9f3d2114506a03114f62f766d42701c091e1");
                assert_eq!(pubkey.childNum, 0);
                assert_eq!(pubkey.publicKey, "03a1af804ac108a8a51782198c2d034b28bf90c8803f5a53f76276fa69a4eae77f");
                assert_eq!(pubkey.fingerprint, 2147483647); // Max i32 value
                assert_eq!(pubkey.depth, 3);
            },
            _ => panic!("Expected PublicKey payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_account_info() {
        // Create a mock response URL that would come from a getAccountInfo call
        let account_json = json!({
            "id": 0,
            "path": "m/49'/0'/0'",
            "descriptor": "xpub6CVKsQYXc9awxgV1tWbG4foDvdcnieK2JkbpPEBKB5WwAPKBZ1mstLbKVB4ov7QzxzjaxNK6EfmNY5Jsk2cG26EVcEkycGW4tchT2dyUhrx",
            "balance": "12500000",
            "availableBalance": "12500000"
        });

        let response_json = json!({
            "success": true,
            "payload": account_json
        });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=account123&method=getAccountInfo&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::AccountInfo(account) => {
                assert_eq!(account.id, 0);
                assert_eq!(account.path, "m/49'/0'/0'");
                assert_eq!(account.descriptor, "xpub6CVKsQYXc9awxgV1tWbG4foDvdcnieK2JkbpPEBKB5WwAPKBZ1mstLbKVB4ov7QzxzjaxNK6EfmNY5Jsk2cG26EVcEkycGW4tchT2dyUhrx");
                assert_eq!(account.balance, "12500000");
                assert_eq!(account.availableBalance, "12500000");
            },
            _ => panic!("Expected AccountInfo payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_error_response() {
        // Create a mock error response
        let response_json = json!({
            "success": false,
            "error": "Device disconnected"
        });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=error123&method=getFeatures&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url);
        assert!(result.is_err());
        match result {
            Err(TrezorConnectError::Other { error_details }) => {
                assert_eq!(error_details, "Device disconnected");
            },
            _ => panic!("Expected TrezorConnectError::Other but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_invalid_url() {
        // Use a clearly invalid URL format that will trigger a URL parsing error
        let result = handle_deep_link("not a valid url with spaces and special chars: @#$%^&*()");

        assert!(result.is_err());
        // Check that it's the right error type or message
        match result {
            Err(TrezorConnectError::UrlError { error_details }) => {
                // We're just checking for the error type here, not the specific message
                assert!(error_details.contains("Failed to parse callback URL"));
            },
            Err(other) => {
                panic!("Expected UrlError, but got {:?}", other);
            },
            Ok(_) => {
                panic!("Expected error, but got Ok result");
            }
        }
    }

    #[test]
    fn test_handle_deep_link_missing_id() {
        // Create a response URL missing the ID parameter
        let response_json = json!({
            "success": true,
            "payload": { "data": "some data" }
        });

        let callback_url = format!(
            "exampleapp://trezor-callback?method=getFeatures&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url);
        assert!(result.is_err());
        match result {
            Err(TrezorConnectError::Other { error_details }) => {
                assert!(error_details.contains("Missing 'id' parameter"));
            },
            _ => panic!("Expected Other error, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_missing_response() {
        // Create a response URL missing the response parameter
        let callback_url = "exampleapp://trezor-callback?id=test123&method=getFeatures";

        let result = handle_deep_link(callback_url);
        assert!(result.is_err());
        match result {
            Err(TrezorConnectError::Other { error_details }) => {
                assert!(error_details.contains("Missing 'response' parameter"));
            },
            _ => panic!("Expected Other error, but got something else"),
        }
    }

    // New tests to cover TrezorConnectError functionality

    #[test]
    fn test_serde_error_conversion() {
        // Create a serde_json::Error
        let json_error = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();

        // Convert to TrezorConnectError
        let trezor_error: TrezorConnectError = json_error.into();

        // Check the error is correctly converted
        match trezor_error {
            TrezorConnectError::SerdeError { error_details } => {
                assert!(error_details.contains("expected value"));
            },
            _ => panic!("Expected SerdeError but got something else"),
        }
    }

    #[test]
    fn test_url_error_conversion() {
        // Create a url::ParseError
        let url_error = url::Url::parse("invalid url").unwrap_err();

        // Convert to TrezorConnectError
        let trezor_error: TrezorConnectError = url_error.into();

        // Check the error is correctly converted
        match trezor_error {
            TrezorConnectError::UrlError { error_details } => {
                // The actual error message may vary but should mention the URL was relative without a base
                assert!(error_details.contains("relative URL without a base"));
            },
            _ => panic!("Expected UrlError but got something else"),
        }
    }

        #[test]
        fn test_error_display() {
        // Test the display implementation for TrezorConnectError
        let error = TrezorConnectError::SerdeError {
        error_details: "test serde error".to_string()
        };
        assert_eq!(format!("{}", error), "Serialization error: test serde error");

        let error = TrezorConnectError::UrlError {
        error_details: "test url error".to_string()
        };
        assert_eq!(format!("{}", error), "URL error: test url error");

        let error = TrezorConnectError::EnvironmentError {
        error_details: "test environment error".to_string()
        };
        assert_eq!(format!("{}", error), "Environment error: test environment error");

        let error = TrezorConnectError::Other {
        error_details: "test other error".to_string()
        };
        assert_eq!(format!("{}", error), "Error: test other error");

        let error = TrezorConnectError::ClientError {
        error_details: "test client error".to_string()
        };
        assert_eq!(format!("{}", error), "Unable to create client: test client error");
        }

    #[test]
    fn test_chained_error_conversion() {
        // Test that we can chain error conversions
        let url_error = url::Url::parse("invalid url").unwrap_err();
        let error_str = format!("URL parsing failed: {}", url_error);
        let trezor_error = TrezorConnectError::ClientError {
            error_details: error_str
        };

        // Check that the error message contains the expected fragments
        assert!(format!("{}", trezor_error).contains("URL parsing failed"));
        assert!(format!("{}", trezor_error).contains("relative URL without a base"));
    }
}