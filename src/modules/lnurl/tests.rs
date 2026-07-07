#[cfg(test)]
mod tests {
    use crate::lnurl::implementation::{
        build_lnurl_pay_callback_url, create_channel_request_url, create_withdraw_callback_url,
        get_lnurl_invoice_for_pay_data, lnurl_auth, validate_lnurl_pay_invoice,
    };
    use crate::lnurl::{ChannelRequestParams, LnurlAuthParams, LnurlError, WithdrawCallbackParams};
    use crate::LnurlPayData;
    use bitcoin::hashes::{sha256, Hash as _};
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
    use lnurl::get_derivation_path;

    const TEST_MNEMONIC: &str = "stable inch effort skull suggest circle charge lemon amazing clean giant quantum party grow visa best rule icon gown disagree win drop smile love";
    const TEST_METADATA: &str = "[[\"text/plain\",\"test payment\"]]";
    const TEST_AMOUNT_MSATS: u64 = 12_345_000;

    fn create_test_invoice(amount_msats: Option<u64>, metadata: &str, hashed: bool) -> String {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[0xab; 32]).unwrap();

        let mut builder = InvoiceBuilder::new(Currency::Bitcoin)
            .payment_hash(sha256::Hash::from_byte_array([1u8; 32]))
            .payment_secret(PaymentSecret([2u8; 32]))
            .current_timestamp()
            .min_final_cltv_expiry_delta(144);

        if let Some(amount_msats) = amount_msats {
            builder = builder.amount_milli_satoshis(amount_msats);
        }

        let builder = if hashed {
            builder.description_hash(sha256::Hash::hash(metadata.as_bytes()))
        } else {
            builder.description(metadata.to_string())
        };

        builder
            .build_signed(|hash| secp.sign_ecdsa_recoverable(hash, &secret_key))
            .unwrap()
            .to_string()
    }

    fn test_pay_data() -> LnurlPayData {
        LnurlPayData {
            uri: "lnurl1test".to_string(),
            callback: "https://example.com/callback?existing=1".to_string(),
            min_sendable: 1_000,
            max_sendable: 20_000_000,
            metadata_str: TEST_METADATA.to_string(),
            comment_allowed: Some(100),
            allows_nostr: false,
            nostr_pubkey: None,
        }
    }

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
    fn test_lnurl_pay_callback_url_preserves_existing_params() {
        let url = build_lnurl_pay_callback_url(
            "https://example.com/callback?existing=param",
            TEST_AMOUNT_MSATS,
            Some("hello"),
        )
        .unwrap();

        assert_eq!(url.scheme(), "https");
        assert!(url.as_str().contains("existing=param"));
        assert!(url.as_str().contains("amount=12345000"));
        assert!(url.as_str().contains("comment=hello"));
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_exact_match() {
        let invoice = create_test_invoice(Some(TEST_AMOUNT_MSATS), TEST_METADATA, false);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_larger_mismatch() {
        let invoice = create_test_invoice(Some(TEST_AMOUNT_MSATS + 1_000), TEST_METADATA, false);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(matches!(
            result,
            Err(LnurlError::AmountMismatch {
                requested_msats: TEST_AMOUNT_MSATS,
                invoice_msats
            }) if invoice_msats == TEST_AMOUNT_MSATS + 1_000
        ));
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_smaller_mismatch() {
        let invoice = create_test_invoice(Some(TEST_AMOUNT_MSATS - 1_000), TEST_METADATA, false);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(matches!(
            result,
            Err(LnurlError::AmountMismatch {
                requested_msats: TEST_AMOUNT_MSATS,
                invoice_msats
            }) if invoice_msats == TEST_AMOUNT_MSATS - 1_000
        ));
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_amountless() {
        let invoice = create_test_invoice(None, TEST_METADATA, false);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(matches!(
            result,
            Err(LnurlError::AmountMismatch {
                requested_msats: TEST_AMOUNT_MSATS,
                invoice_msats: 0
            })
        ));
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_malformed() {
        let result = validate_lnurl_pay_invoice("lnbc1malformed", TEST_AMOUNT_MSATS);

        assert!(matches!(result, Err(LnurlError::InvalidResponse)));
    }

    #[tokio::test]
    async fn test_get_lnurl_invoice_for_pay_data_amount_outside_range() {
        let data = test_pay_data();

        let result = get_lnurl_invoice_for_pay_data(data, 999, None).await;

        assert!(matches!(result, Err(LnurlError::InvalidAmount { .. })));
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_matching_amount_with_hash_description() {
        let invoice = create_test_invoice(Some(TEST_AMOUNT_MSATS), TEST_METADATA, true);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_matching_amount_with_text_description() {
        let invoice = create_test_invoice(Some(TEST_AMOUNT_MSATS), "test payment", false);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_lnurl_pay_invoice_matching_amount_with_different_description() {
        let invoice = create_test_invoice(Some(TEST_AMOUNT_MSATS), "other metadata", false);

        let result = validate_lnurl_pay_invoice(&invoice, TEST_AMOUNT_MSATS);

        assert!(result.is_ok());
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
