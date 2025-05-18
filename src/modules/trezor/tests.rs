#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::modules::trezor::{handle_deep_link, AccountAddresses, AccountInfoDetails, AccountUtxo, AddressInfo, ComposeAccount, ComposeOutput, ComposeTransactionParams, ComposeTransactionResponse, DefaultAccountType, FeeLevel, GetAccountInfoParams, GetAddressParams, GetPublicKeyParams, RefTransaction, RefTxInput, RefTxOutput, ScriptType, SignMessageParams, SignTransactionParams, TokenFilter, TrezorConnectClient, TrezorConnectError, TrezorEnvironment, TrezorResponsePayload, TxInputType, TxOutputType, VerifyMessageParams};
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

    #[test]
    fn test_sign_transaction_basic() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let inputs = vec![
            TxInputType {
                prev_hash: "b035d89d4543ce5713c553d69431698116a822c57c03ddacf3f04b763d1999ac".to_string(),
                prev_index: 0,
                amount: 3431747,
                sequence: None,
                address_n: Some(vec![
                    44 | 0x80000000u32,
                    0 | 0x80000000u32,
                    2 | 0x80000000u32,
                    1,
                    0,
                ]),
                script_type: Some(ScriptType::SpendAddress),
                multisig: None,
                script_pubkey: None,
                script_sig: None,
                witness: None,
                ownership_proof: None,
                commitment_data: None,
                orig_hash: None,
                orig_index: None,
                coinjoin_flags: None,
            }
        ];

        let outputs = vec![
            TxOutputType {
                address: None,
                address_n: Some(vec![
                    44 | 0x80000000u32,
                    0 | 0x80000000u32,
                    2 | 0x80000000u32,
                    1,
                    1,
                ]),
                amount: 3181747,
                script_type: ScriptType::PayToAddress,
                multisig: None,
                op_return_data: None,
                orig_hash: None,
                orig_index: None,
                payment_req_index: None,
            },
            TxOutputType {
                address: Some("18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2".to_string()),
                address_n: None,
                amount: 200000,
                script_type: ScriptType::PayToAddress,
                multisig: None,
                op_return_data: None,
                orig_hash: None,
                orig_index: None,
                payment_req_index: None,
            }
        ];

        let params = SignTransactionParams {
            coin: "btc".to_string(),
            inputs,
            outputs,
            refTxs: None,
            paymentRequests: None,
            locktime: None,
            version: None,
            expiry: None,
            versionGroupId: None,
            overwintered: None,
            timestamp: None,
            branchId: None,
            push: None,
            amountUnit: None,
            unlockPath: None,
            serialize: None,
            chunkify: None,
            common: None,
        };

        let result = client.sign_transaction(params, Some("sign123".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=signTransaction"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("inputs"));
        assert!(result.url.contains("outputs"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dsign123"));
        assert_eq!(result.request_id, "sign123");
    }

    #[test]
    fn test_sign_transaction_with_reftxs() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let inputs = vec![
            TxInputType {
                prev_hash: "b035d89d4543ce5713c553d69431698116a822c57c03ddacf3f04b763d1999ac".to_string(),
                prev_index: 0,
                amount: 3431747,
                sequence: None,
                address_n: Some(vec![
                    44 | 0x80000000u32,
                    0 | 0x80000000u32,
                    2 | 0x80000000u32,
                    1,
                    0,
                ]),
                script_type: Some(ScriptType::SpendAddress),
                multisig: None,
                script_pubkey: None,
                script_sig: None,
                witness: None,
                ownership_proof: None,
                commitment_data: None,
                orig_hash: None,
                orig_index: None,
                coinjoin_flags: None,
            }
        ];

        let outputs = vec![
            TxOutputType {
                address: Some("18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2".to_string()),
                address_n: None,
                amount: 200000,
                script_type: ScriptType::PayToAddress,
                multisig: None,
                op_return_data: None,
                orig_hash: None,
                orig_index: None,
                payment_req_index: None,
            }
        ];

        let ref_txs = vec![
            RefTransaction {
                hash: "b035d89d4543ce5713c553d69431698116a822c57c03ddacf3f04b763d1999ac".to_string(),
                version: Some(1),
                inputs: vec![
                    RefTxInput {
                        prev_hash: "448946a44f1ef514601ccf9b22cc3e638c69ea3900b67b87517ea673eb0293dc".to_string(),
                        prev_index: 0,
                        script_sig: "47304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022012103f66bbe3c721f119bb4b8a1e6c1832b98f2cf625d9f59242008411dd92aab8d94".to_string(),
                        sequence: 4294967295,
                    }
                ],
                bin_outputs: vec![
                    RefTxOutput {
                        amount: 3431747,
                        script_pubkey: "76a91441352a84436847a7b660d5e76518f6ebb718dedc88ac".to_string(),
                    },
                    RefTxOutput {
                        amount: 10000,
                        script_pubkey: "76a9141403b451c79d34e6a7f6e36806683308085467ac88ac".to_string(),
                    }
                ],
                lock_time: Some(0),
                expiry: None,
                version_group_id: None,
                overwintered: None,
                timestamp: None,
                branch_id: None,
                extra_data: None,
            }
        ];

        let params = SignTransactionParams {
            coin: "btc".to_string(),
            inputs,
            outputs,
            refTxs: Some(ref_txs),
            paymentRequests: None,
            locktime: None,
            version: None,
            expiry: None,
            versionGroupId: None,
            overwintered: None,
            timestamp: None,
            branchId: None,
            push: None,
            amountUnit: None,
            unlockPath: None,
            serialize: None,
            chunkify: None,
            common: None,
        };

        let result = client.sign_transaction(params, Some("sign456".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=signTransaction"));
        assert!(result.url.contains("refTxs"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dsign456"));
        assert_eq!(result.request_id, "sign456");
    }

    #[test]
    fn test_handle_deep_link_signed_transaction() {
        // Create a mock response URL that would come from a signTransaction call
        let signed_tx_json = json!({
        "signatures": [
            "304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022"
        ],
        "serializedTx": "0100000001ac99193d764bf0f3acdd037cc522a8168169316963d553c51357ce43459dd835b0000000006a47304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022012103f66bbe3c721f119bb4b8a1e6c1832b98f2cf625d9f59242008411dd92aab8d94ffffffff02b3893000000000001976a91441352a84436847a7b660d5e76518f6ebb718dedc88ac400d030000000000a914b7536c788d8cfbac33b0d3fb0b9cb4b34bb2bb1087000000"
    });

        let response_json = json!({
        "success": true,
        "payload": signed_tx_json
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=sign123&method=signTransaction&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::SignedTransaction(signed_tx) => {
                assert_eq!(signed_tx.signatures.len(), 1);
                assert_eq!(signed_tx.signatures[0], "304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022");
                assert!(signed_tx.serializedTx.starts_with("0100000001"));
                assert!(signed_tx.txid.is_none()); // No txid since push was not true
            },
            _ => panic!("Expected SignedTransaction payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_signed_transaction_with_txid() {
        // Create a mock response URL with txid (when push=true)
        let signed_tx_json = json!({
        "signatures": [
            "304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022"
        ],
        "serializedTx": "0100000001ac99193d764bf0f3acdd037cc522a8168169316963d553c51357ce43459dd835b0000000006a47304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022012103f66bbe3c721f119bb4b8a1e6c1832b98f2cf625d9f59242008411dd92aab8d94ffffffff02b3893000000000001976a91441352a84436847a7b660d5e76518f6ebb718dedc88ac400d030000000000a914b7536c788d8cfbac33b0d3fb0b9cb4b34bb2bb1087000000",
        "txid": "a1b2c3d4e5f6789012345678901234567890123456789012345678901234567890"
    });

        let response_json = json!({
        "success": true,
        "payload": signed_tx_json
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=sign456&method=signTransaction&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::SignedTransaction(signed_tx) => {
                assert_eq!(signed_tx.signatures.len(), 1);
                assert!(signed_tx.serializedTx.starts_with("0100000001"));
                assert!(signed_tx.txid.is_some());
                assert_eq!(signed_tx.txid.unwrap(), "a1b2c3d4e5f6789012345678901234567890123456789012345678901234567890");
            },
            _ => panic!("Expected SignedTransaction payload, but got something else"),
        }
    }
    #[test]
    fn test_sign_message_basic() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = SignMessageParams {
            path: "m/44'/0'/0'".to_string(),
            coin: None,
            message: "Hello World!".to_string(),
            hex: None,
            no_script_type: None,
            common: None,
        };

        let result = client.sign_message(params, Some("message123".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=signMessage"));
        assert!(result.url.contains("path"));
        assert!(result.url.contains("message"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dmessage123"));
        assert_eq!(result.request_id, "message123");
    }

    #[test]
    fn test_sign_message_with_coin() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = SignMessageParams {
            path: "m/44'/0'/0'".to_string(),
            coin: Some("btc".to_string()),
            message: "Example message".to_string(),
            hex: Some(false),
            no_script_type: Some(false),
            common: None,
        };

        let result = client.sign_message(params, Some("message456".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=signMessage"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("hex"));
        assert!(result.url.contains("no_script_type"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dmessage456"));
        assert_eq!(result.request_id, "message456");
    }

    #[test]
    fn test_sign_message_hex() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = SignMessageParams {
            path: "m/44'/0'/0'".to_string(),
            coin: Some("btc".to_string()),
            message: "48656c6c6f20576f726c6421".to_string(), // "Hello World!" in hex
            hex: Some(true),
            no_script_type: None,
            common: None,
        };

        let result = client.sign_message(params, Some("hex789".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=signMessage"));
        assert!(result.url.contains("hex"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dhex789"));
        assert_eq!(result.request_id, "hex789");
    }

    #[test]
    fn test_handle_deep_link_message_signature() {
        // Create a mock response URL that would come from a signMessage call
        let message_signature_json = json!({
        "address": "1BgGZ9tcN4rm9KBzDn7KprQz87SZ26SAMH",
        "signature": "IDKn5wGhOsGWWkOJ1gD4nXR14VJ/2Rg3A6P9XwRPjZQWjCYJvFNFKKQ5CLjy8oLKlhVIzF7UWNEqVx7lp5Rrffw="
    });

        let response_json = json!({
        "success": true,
        "payload": message_signature_json
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=message123&method=signMessage&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::MessageSignature(msg_sig) => {
                assert_eq!(msg_sig.address, "1BgGZ9tcN4rm9KBzDn7KprQz87SZ26SAMH");
                assert_eq!(msg_sig.signature, "IDKn5wGhOsGWWkOJ1gD4nXR14VJ/2Rg3A6P9XwRPjZQWjCYJvFNFKKQ5CLjy8oLKlhVIzF7UWNEqVx7lp5Rrffw=");
            },
            _ => panic!("Expected MessageSignature payload, but got something else"),
        }
    }
    #[test]
    fn test_verify_message_basic() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = VerifyMessageParams {
            address: "3BD8TL6iShVzizQzvo789SuynEKGpLTms9".to_string(),
            signature: "JO7vL3tOB1qQyfSeIVLvdEw9G1tCvL+lNj78XDAVM4t6UptADs3kXDTO2+2ZeEOLFL4/+wm+BBdSpo3kb3Cnsas=".to_string(),
            message: "example message".to_string(),
            coin: "btc".to_string(),
            hex: None,
            common: None,
        };

        let result = client.verify_message(params, Some("verify123".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=verifyMessage"));
        assert!(result.url.contains("address"));
        assert!(result.url.contains("signature"));
        assert!(result.url.contains("message"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dverify123"));
        assert_eq!(result.request_id, "verify123");
    }

    #[test]
    fn test_verify_message_with_hex() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let params = VerifyMessageParams {
            address: "1BgGZ9tcN4rm9KBzDn7KprQz87SZ26SAMH".to_string(),
            signature: "IDKn5wGhOsGWWkOJ1gD4nXR14VJ/2Rg3A6P9XwRPjZQWjCYJvFNFKKQ5CLjy8oLKlhVIzF7UWNEqVx7lp5Rrffw=".to_string(),
            message: "48656c6c6f20576f726c6421".to_string(), // "Hello World!" in hex
            coin: "btc".to_string(),
            hex: Some(true),
            common: None,
        };

        let result = client.verify_message(params, Some("verify456".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=verifyMessage"));
        assert!(result.url.contains("hex"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dverify456"));
        assert_eq!(result.request_id, "verify456");
    }

    #[test]
    fn test_handle_deep_link_verify_message() {
        // Create a mock response URL that would come from a verifyMessage call
        let verify_message_json = json!({
        "message": "Message verified"
    });

        let response_json = json!({
        "success": true,
        "payload": verify_message_json
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=verify123&method=verifyMessage&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::VerifyMessage(verify_msg) => {
                assert_eq!(verify_msg.message, "Message verified");
            },
            _ => panic!("Expected VerifyMessage payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_verify_message_error() {
        // Create a mock error response URL from verifyMessage call
        let response_json = json!({
        "success": false,
        "error": "Invalid signature"
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=verify456&method=verifyMessage&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url);
        assert!(result.is_err());
        match result {
            Err(TrezorConnectError::Other { error_details }) => {
                assert_eq!(error_details, "Invalid signature");
            },
            _ => panic!("Expected TrezorConnectError::Other but got something else"),
        }
    }
    #[test]
    fn test_compose_transaction_payment() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let outputs = vec![
            ComposeOutput::Regular {
                amount: "200000".to_string(),
                address: "18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2".to_string(),
            }
        ];

        let params = ComposeTransactionParams {
            outputs,
            coin: "btc".to_string(),
            push: Some(true),
            sequence: None,
            account: None,
            fee_levels: None,
            skip_permutation: None,
            common: None,
        };

        let result = client.compose_transaction(params, Some("compose123".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=composeTransaction"));
        assert!(result.url.contains("coin"));
        assert!(result.url.contains("outputs"));
        assert!(result.url.contains("push"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dcompose123"));
        assert_eq!(result.request_id, "compose123");
    }

    #[test]
    fn test_compose_transaction_precompose() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let outputs = vec![
            ComposeOutput::Regular {
                amount: "200000".to_string(),
                address: "tb1q9l0rk0gkgn73d0gc57qn3t3cwvucaj3h8wtrlu".to_string(),
            }
        ];

        let account = ComposeAccount {
            path: "m/84'/0'/0'".to_string(),
            addresses: AccountAddresses {
                used: vec![AddressInfo {
                    address: "bc1qannfxke2tfd4l7vhepehpvt05y83v3qsf6nfkk".to_string(),
                    path: "m/84'/0'/0'/0/0".to_string(),
                    transfers: 1,
                }],
                unused: vec![AddressInfo {
                    address: "".to_string(),
                    path: "m/84'/0'/0'/0/1".to_string(),
                    transfers: 0,
                }],
                change: vec![AddressInfo {
                    address: "bc1qktmhrsmsenepnnfst8x6j27l0uqv7ggrg8x38q".to_string(),
                    path: "m/84'/0'/0'/1/0".to_string(),
                    transfers: 0,
                }],
            },
            utxo: vec![AccountUtxo {
                txid: "86a6e02943dcd057cfbe349f2c2274478a3a1be908eb788606a6950e727a0d36".to_string(),
                vout: 0,
                amount: "300000".to_string(),
                block_height: Some(590093),
                address: "bc1qannfxke2tfd4l7vhepehpvt05y83v3qsf6nfkk".to_string(),
                path: "m/84'/0'/0'/0/0".to_string(),
                confirmations: Some(100),
            }],
        };

        let fee_levels = vec![
            FeeLevel {
                fee_per_unit: "1".to_string(),
                base_fee: None,
                floor_base_fee: None,
            },
            FeeLevel {
                fee_per_unit: "5".to_string(),
                base_fee: None,
                floor_base_fee: None,
            },
            FeeLevel {
                fee_per_unit: "30".to_string(),
                base_fee: None,
                floor_base_fee: None,
            },
        ];

        let params = ComposeTransactionParams {
            outputs,
            coin: "btc".to_string(),
            push: None,
            sequence: None,
            account: Some(account),
            fee_levels: Some(fee_levels),
            skip_permutation: None,
            common: None,
        };

        let result = client.compose_transaction(params, Some("precompose456".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=composeTransaction"));
        assert!(result.url.contains("account"));
        assert!(result.url.contains("fee_levels"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dprecompose456"));
        assert_eq!(result.request_id, "precompose456");
    }

    #[test]
    fn test_compose_transaction_send_max() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let outputs = vec![
            ComposeOutput::SendMax {
                address: "18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2".to_string(),
            }
        ];

        let params = ComposeTransactionParams {
            outputs,
            coin: "btc".to_string(),
            push: Some(false),
            sequence: None,
            account: None,
            fee_levels: None,
            skip_permutation: None,
            common: None,
        };

        let result = client.compose_transaction(params, Some("sendmax789".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=composeTransaction"));
        assert!(result.url.contains("send-max"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dsendmax789"));
        assert_eq!(result.request_id, "sendmax789");
    }

    #[test]
    fn test_compose_transaction_op_return() {
        let client = TrezorConnectClient::new(
            TrezorEnvironment::Local,
            "exampleapp://trezor-callback"
        ).unwrap();

        let outputs = vec![
            ComposeOutput::Regular {
                amount: "100000".to_string(),
                address: "18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2".to_string(),
            },
            ComposeOutput::OpReturn {
                data_hex: "48656c6c6f20576f726c6421".to_string(), // "Hello World!" in hex
            },
        ];

        let params = ComposeTransactionParams {
            outputs,
            coin: "btc".to_string(),
            push: None,
            sequence: None,
            account: None,
            fee_levels: None,
            skip_permutation: None,
            common: None,
        };

        let result = client.compose_transaction(params, Some("opreturn123".to_string())).unwrap();
        assert!(result.url.starts_with("trezorsuitelite://connect/1/"));
        assert!(result.url.contains("method=composeTransaction"));
        assert!(result.url.contains("opreturn"));
        assert!(result.url.contains("dataHex"));
        assert!(result.url.contains("callback=exampleapp%3A%2F%2Ftrezor-callback%3Fid%3Dopreturn123"));
        assert_eq!(result.request_id, "opreturn123");
    }

    #[test]
    fn test_handle_deep_link_compose_transaction_payment() {
        // Create a mock response URL that would come from a composeTransaction call (payment mode)
        let signed_tx_json = json!({
        "signatures": [
            "304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022"
        ],
        "serializedTx": "0100000001ac99193d764bf0f3acdd037cc522a8168169316963d553c51357ce43459dd835b0000000006a47304402202872cb8459eed053dcec0f353c7e293611fe77615862bfadb4d35a5d8807a4cf022015057aa0aaf72ab342b5f8939f86f193ad87b539931911a72e77148a1233e022012103f66bbe3c721f119bb4b8a1e6c1832b98f2cf625d9f59242008411dd92aab8d94ffffffff02b3893000000000001976a91441352a84436847a7b660d5e76518f6ebb718dedc88ac400d030000000000a914b7536c788d8cfbac33b0d3fb0b9cb4b34bb2bb1087000000",
        "txid": "a1b2c3d4e5f6789012345678901234567890123456789012345678901234567890"
    });

        let response_json = json!({
        "success": true,
        "payload": signed_tx_json
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=compose123&method=composeTransaction&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::ComposeTransaction(ComposeTransactionResponse::SignedTransaction(signed_tx)) => {
                assert_eq!(signed_tx.signatures.len(), 1);
                assert!(signed_tx.serializedTx.starts_with("0100000001"));
                assert!(signed_tx.txid.is_some());
                assert_eq!(signed_tx.txid.unwrap(), "a1b2c3d4e5f6789012345678901234567890123456789012345678901234567890");
            },
            _ => panic!("Expected ComposeTransaction SignedTransaction payload, but got something else"),
        }
    }

    #[test]
    fn test_handle_deep_link_compose_transaction_precompose() {
        // Create a mock response URL that would come from a composeTransaction call (precompose mode)
        let precomposed_txs_json = json!([
        {
            "type": "final",
            "totalSpent": "200167",
            "fee": "167",
            "feePerByte": "1",
            "bytes": 167,
            "inputs": [
                {
                    "address_n": [2147483732u32, 2147483648u32, 2147483648u32, 0u32, 0u32],
                    "amount": "300000",
                    "prev_hash": "86a6e02943dcd057cfbe349f2c2274478a3a1be908eb788606a6950e727a0d36",
                    "prev_index": 0,
                    "script_type": "SPENDWITNESS"
                }
            ],
            "outputs": [
                {
                    "address_n": [2147483732u32, 2147483648u32, 2147483649u32, 0u32],
                    "amount": "99833",
                    "script_type": "PAYTOWITNESS"
                },
                {
                    "address": "tb1q9l0rk0gkgn73d0gc57qn3t3cwvucaj3h8wtrlu",
                    "amount": "200000",
                    "script_type": "PAYTOADDRESS"
                }
            ],
            "outputsPermutation": [1, 0]
        },
        {
            "type": "final",
            "totalSpent": "200835",
            "fee": "835",
            "feePerByte": "5",
            "bytes": 167
        },
        {
            "type": "error"
        }
    ]);

        let response_json = json!({
        "success": true,
        "payload": precomposed_txs_json
    });

        let callback_url = format!(
            "exampleapp://trezor-callback?id=precompose456&method=composeTransaction&response={}",
            url::form_urlencoded::byte_serialize(response_json.to_string().as_bytes()).collect::<String>()
        );

        let result = handle_deep_link(callback_url).unwrap();
        match result {
            TrezorResponsePayload::ComposeTransaction(ComposeTransactionResponse::PrecomposedTransactions(precomposed_txs)) => {
                assert_eq!(precomposed_txs.len(), 3);
                assert_eq!(precomposed_txs[0].tx_type, "final");
                assert_eq!(precomposed_txs[0].total_spent, Some("200167".to_string()));
                assert_eq!(precomposed_txs[0].fee, Some("167".to_string()));
                assert_eq!(precomposed_txs[1].tx_type, "final");
                assert_eq!(precomposed_txs[2].tx_type, "error");
            },
            _ => panic!("Expected ComposeTransaction PrecomposedTransactions payload, but got something else"),
        }
    }
}