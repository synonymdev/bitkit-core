#[cfg(test)]
mod tests {
    use crate::lnurl::implementation::{
        create_channel_request_url, create_withdraw_callback_url, lnurl_auth,
    };
    use crate::lnurl::{ChannelRequestParams, LnurlAuthParams, LnurlError, WithdrawCallbackParams};
    use lnurl::get_derivation_path;

    const TEST_MNEMONIC: &str = "stable inch effort skull suggest circle charge lemon amazing clean giant quantum party grow visa best rule icon gown disagree win drop smile love";

    #[test]
    fn test_create_channel_request_url() {
        let params = ChannelRequestParams {
            k1: "test_k1_value".to_string(),
            callback: "https://example.com/callback".to_string(),
            local_node_id: "03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
                .to_string(),
            is_private: true,
            cancel: false,
        };

        let result = create_channel_request_url(params).unwrap();

        assert!(result.contains("k1=test_k1_value"));
        assert!(result.contains(
            "remoteid=03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
        ));
        assert!(result.contains("private=1"));
        assert!(result.contains("cancel=0"));
        assert!(result.starts_with("https://example.com/callback?"));
    }

    #[test]
    fn test_create_channel_request_url_with_existing_params() {
        let params = ChannelRequestParams {
            k1: "test_k1_value".to_string(),
            callback: "https://example.com/callback?existing=param".to_string(),
            local_node_id: "03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
                .to_string(),
            is_private: false,
            cancel: true,
        };

        let result = create_channel_request_url(params).unwrap();

        assert!(result.contains("existing=param"));
        assert!(result.contains("k1=test_k1_value"));
        assert!(result.contains(
            "remoteid=03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
        ));
        assert!(result.contains("private=0"));
        assert!(result.contains("cancel=1"));
    }

    #[test]
    fn test_create_channel_request_url_with_existing_k1() {
        // Test case where the callback URL already contains k1 parameter
        let params = ChannelRequestParams {
            k1: "new_k1_value".to_string(),
            callback: "https://example.com/callback?k1=existing_k1_value&foo=bar".to_string(),
            local_node_id: "03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
                .to_string(),
            is_private: false,
            cancel: true,
        };

        let result = create_channel_request_url(params).unwrap();

        // Check that we have exactly one k1 parameter (the new one)
        let k1_count = result.matches("k1=").count();
        assert_eq!(
            k1_count, 1,
            "URL should have exactly 1 k1 parameter after fix"
        );

        // The URL should contain only the new k1 value
        assert!(
            !result.contains("k1=existing_k1_value"),
            "Old k1 value should be replaced"
        );
        assert!(
            result.contains("k1=new_k1_value"),
            "New k1 value should be present"
        );

        // Other parameters should be preserved
        assert!(
            result.contains("foo=bar"),
            "Other query parameters should be preserved"
        );
    }

    #[test]
    fn test_create_withdraw_callback_url() {
        let params = WithdrawCallbackParams {
            k1: "test_k1_value".to_string(),
            callback: "https://example.com/withdraw".to_string(),
            payment_request: "lnbc1230n1pjqqqqqqpp5abcdef...".to_string(),
        };

        let result = create_withdraw_callback_url(params).unwrap();

        assert!(result.contains("k1=test_k1_value"));
        assert!(result.contains("pr=lnbc1230n1pjqqqqqqpp5abcdef..."));
        assert!(result.starts_with("https://example.com/withdraw?"));
    }

    #[test]
    fn test_create_withdraw_callback_url_with_existing_params() {
        let params = WithdrawCallbackParams {
            k1: "test_k1_value".to_string(),
            callback: "https://example.com/withdraw?existing=param".to_string(),
            payment_request: "lnbc1230n1pjqqqqqqpp5abcdef...".to_string(),
        };

        let result = create_withdraw_callback_url(params).unwrap();

        assert!(result.contains("existing=param"));
        assert!(result.contains("k1=test_k1_value"));
        assert!(result.contains("pr=lnbc1230n1pjqqqqqqpp5abcdef..."));
    }

    #[test]
    fn test_invalid_callback_url() {
        let params = ChannelRequestParams {
            k1: "test_k1_value".to_string(),
            callback: "invalid_url".to_string(),
            local_node_id: "03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
                .to_string(),
            is_private: true,
            cancel: false,
        };

        let result = create_channel_request_url(params);
        assert!(result.is_err());
        assert!(matches!(result, Err(LnurlError::InvalidAddress)));
    }

    #[test]
    fn test_get_derivation_path() {
        use url::Url;

        // Test with a simple domain
        let hashing_key: [u8; 32] = [
            0x7d, 0x41, 0x7a, 0x6a, 0x5e, 0x9a, 0x6a, 0x4a, 0x87, 0x9a, 0xea, 0xba, 0x11, 0xa1,
            0x18, 0x38, 0x76, 0x4c, 0x8f, 0xa2, 0xb9, 0x59, 0xc2, 0x42, 0xd4, 0x3d, 0xea, 0x68,
            0x2b, 0x3e, 0x40, 0x9b,
        ];
        let url = Url::parse("https://site.com").unwrap();

        let path = get_derivation_path(hashing_key, &url).unwrap();

        // Based on the test vector, the expected path should be:
        // 138'/1588488367/511787106'/38110259/1988853114'
        let expected_path = "138'/1588488367/511787106'/38110259/1988853114'";
        assert_eq!(path.to_string(), expected_path);

        // Test that same inputs produce same path
        let path2 = get_derivation_path(hashing_key, &url).unwrap();
        assert_eq!(path.to_string(), path2.to_string());
    }

    #[test]
    fn test_create_channel_request_url_matches_reference() {
        let params = ChannelRequestParams {
            k1: "test_k1_value".to_string(),
            callback: "https://example.com/callback".to_string(),
            local_node_id: "03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
                .to_string(),
            is_private: true,
            cancel: false,
        };

        let result = create_channel_request_url(params).unwrap();

        let expected_parts = [
            "https://example.com/callback?",
            "k1=test_k1_value",
            "remoteid=03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234",
            "private=1",
            "cancel=0",
        ];

        for part in expected_parts {
            assert!(result.contains(part), "Result should contain: {}", part);
        }
    }

    #[test]
    fn test_create_withdraw_callback_url_matches_reference() {
        let params = WithdrawCallbackParams {
            k1: "test_k1_value".to_string(),
            callback: "https://example.com/withdraw".to_string(),
            payment_request: "lnbc1230n1pjqqqqqqpp5abcdef...".to_string(),
        };

        let result = create_withdraw_callback_url(params).unwrap();

        let expected_parts = [
            "https://example.com/withdraw?",
            "k1=test_k1_value",
            "pr=lnbc1230n1pjqqqqqqpp5abcdef...",
        ];

        for part in expected_parts {
            assert!(result.contains(part), "Result should contain: {}", part);
        }
    }

    #[tokio::test]
    async fn test_lnurl_auth_invalid_domain() {
        let params = LnurlAuthParams {
            domain: "invalid domain with spaces".to_string(),
            k1: "abcdef1234567890".to_string(),
            callback: "https://example.com/auth".to_string(),
            hashing_key: [0u8; 32],
        };

        let result = lnurl_auth(params).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(LnurlError::InvalidAddress)));
    }

    #[tokio::test]
    async fn test_lnurl_auth_invalid_k1() {
        let params = LnurlAuthParams {
            domain: "example.com".to_string(),
            k1: "invalid_hex".to_string(),
            callback: "https://example.com/auth".to_string(),
            hashing_key: [1u8; 32],
        };

        let result = lnurl_auth(params).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(LnurlError::AuthenticationFailed)));
    }

    #[tokio::test]
    async fn test_lnurl_auth_callback_encoded() {
        let params = LnurlAuthParams {
            domain: "example.com".to_string(),
            k1: "03cb12a5ac8930403c5f8bd9e38dd1e1c07f93a1379d139658fac53183232e19".to_string(),
            callback: "lnurl1dp68gup69uhkcmmrv9kxsmmnwsarxvpsxqhkzat5dq3xqhlx".to_string(),
            hashing_key: [1u8; 32],
        };

        let result = lnurl_auth(params).await;

        assert!(result.is_err());
        assert!(matches!(result, Err(LnurlError::RequestFailed)));
    }

    #[tokio::test]
    async fn test_lnurl_auth_callback_decoded() {
        let params = LnurlAuthParams {
            domain: "example.com".to_string(),
            k1: "03cb12a5ac8930403c5f8bd9e38dd1e1c07f93a1379d139658fac53183232e19".to_string(),
            callback: "https://example.com/auth".to_string(),
            hashing_key: [1u8; 32],
        };

        let result = lnurl_auth(params).await;

        assert!(result.is_err());
        // Error should not be InvalidAddress
        assert!(matches!(result, Err(LnurlError::RequestFailed)));
    }

    #[test]
    fn test_channel_request_params_creation() {
        let params = ChannelRequestParams {
            k1: "test_k1".to_string(),
            callback: "https://example.com".to_string(),
            local_node_id: "03abc123".to_string(),
            is_private: true,
            cancel: false,
        };

        assert_eq!(params.k1, "test_k1");
        assert_eq!(params.callback, "https://example.com");
        assert_eq!(params.local_node_id, "03abc123");
        assert!(params.is_private);
        assert!(!params.cancel);
    }

    #[test]
    fn test_withdraw_callback_params_creation() {
        let params = WithdrawCallbackParams {
            k1: "test_k1".to_string(),
            callback: "https://example.com".to_string(),
            payment_request: "lnbc123...".to_string(),
        };

        assert_eq!(params.k1, "test_k1");
        assert_eq!(params.callback, "https://example.com");
        assert_eq!(params.payment_request, "lnbc123...");
    }

    #[test]
    fn test_lnurl_auth_params_creation() {
        let hashing_key = [42u8; 32];
        let params = LnurlAuthParams {
            domain: "example.com".to_string(),
            k1: "abcdef123456".to_string(),
            callback: "https://example.com/auth".to_string(),
            hashing_key,
        };

        assert_eq!(params.domain, "example.com");
        assert_eq!(params.k1, "abcdef123456");
        assert_eq!(params.callback, "https://example.com/auth");
        assert_eq!(params.hashing_key, hashing_key);
    }

    #[test]
    fn test_url_parameter_encoding() {
        let params = ChannelRequestParams {
            k1: "special+chars&test=value".to_string(),
            callback: "https://example.com/callback".to_string(),
            local_node_id: "03abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"
                .to_string(),
            is_private: false,
            cancel: true,
        };

        let result = create_channel_request_url(params).unwrap();

        assert!(result.contains("cancel=1"));
        assert!(result.contains("private=0"));
        assert!(result.contains("k1="));
        assert!(result.contains("remoteid="));
    }

    #[test]
    fn test_create_withdraw_callback_url_with_existing_k1() {
        // Test case where callback URL already contains k1 parameter
        let params = WithdrawCallbackParams {
            k1: "new_k1_value".to_string(),
            callback: "https://example.com/withdraw?k1=existing_k1_value&foo=bar".to_string(),
            payment_request: "lnbc1230n1pjqqqqqqpp5abcdef...".to_string(),
        };

        let result = create_withdraw_callback_url(params).unwrap();

        // Check that we have exactly one k1 parameter (the new one)
        let k1_count = result.matches("k1=").count();
        assert_eq!(
            k1_count, 1,
            "URL should have exactly 1 k1 parameter after fix"
        );

        // The URL should contain only the new k1 value
        assert!(
            !result.contains("k1=existing_k1_value"),
            "Old k1 value should be replaced"
        );
        assert!(
            result.contains("k1=new_k1_value"),
            "New k1 value should be present"
        );

        // Other parameters should be preserved
        assert!(
            result.contains("foo=bar"),
            "Other query parameters should be preserved"
        );
        assert!(
            result.contains("pr=lnbc1230n1pjqqqqqqpp5abcdef..."),
            "Payment request should be added"
        );
    }
}
