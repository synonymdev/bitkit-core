#[cfg(test)]
mod tests {
    use crate::{DecodingError, Scanner};

    #[tokio::test]
    async fn test_lightning_invoice_decode() {
        let invoice = "lightning:lnbc543210n1pnjdrvfpp5s720f4z6wzvjwpdnrlpffgct375l46yu9c6cpe7gdvvdfay47cnsdqqcqzzsxqrrsssp53uty4kfw8k3wmw4ga802udavz7e64tc7dmaz2cmtkj9srfxaq3ps9p4gqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpqysgqwl2tdhzm9e6mtedt7a4263yw7dqxehdwjnjk23r4g8tuppk6rs994f6scunwsev3w207tjldwkpdt32rcegzphgk05c0lctv8he7smgqyfn5xq".to_string();
        let decoded = Scanner::decode(invoice).await.unwrap();
        match decoded {
            Scanner::Lightning { invoice } => {
                assert_eq!(invoice.is_expired, true);
                assert_eq!(invoice.amount_satoshis, 54321);
                assert!(invoice.payee_node_id.is_some());
                assert!(invoice.get_network().is_some());
            }
            _ => assert!(false, "Should be a Lightning invoice"),
        }
    }

    #[tokio::test]
    async fn test_onchain_invoice_decode() {
        let invoice = "bitcoin:bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq?amount=0.00001&label=Test&message=Test%20Payment&custom=value".to_string();
        let decoded = Scanner::decode(invoice).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                assert_eq!(invoice.amount_satoshis, 1000);
                assert!(invoice.label.is_some());
                assert!(invoice.message.is_some());

                let params = invoice.params.as_ref().unwrap();
                assert_eq!(params.get("amount").unwrap(), "0.00001");
                assert_eq!(params.get("label").unwrap(), "Test");
                assert_eq!(params.get("message").unwrap(), "Test%20Payment");
                assert_eq!(params.get("custom").unwrap(), "value");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_raw_address() {
        let address = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
        let decoded = Scanner::decode(address.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                assert_eq!(invoice.address, address);
                assert_eq!(invoice.amount_satoshis, 0);
                assert!(invoice.params.as_ref().unwrap().is_empty());
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_invalid_lightning_invoice() {
        let invoice = "lnbc1invalid".to_string();
        assert!(matches!(Scanner::decode(invoice).await, Err(DecodingError::InvalidFormat)));
    }

    #[tokio::test]
    async fn test_paykit_deeplink_with_bitkit_scheme() {
        use crate::paykit::create_session_token_from_keypair;

        // Create a valid token
        let token = create_session_token_from_keypair(
            "test_public_key".to_string(),
            "test_secret_key".to_string(),
            None,
            Some(300),
        ).unwrap();

        // Create deeplink with bitkit:// scheme
        let deeplink = format!("bitkit://paykit/session?token={}", token.token);
        let decoded = Scanner::decode(deeplink.clone()).await.unwrap();

        match decoded {
            Scanner::PaykitSession { data } => {
                assert_eq!(data.url, deeplink);
                assert_eq!(data.action, "session");
                assert_eq!(data.token, token.token);
                assert!(data.parameters.is_empty());
            }
            _ => panic!("Should be a PaykitSession"),
        }
    }

    #[tokio::test]
    async fn test_paykit_deeplink_with_pubky_scheme() {
        use crate::paykit::create_session_token_from_keypair;

        // Create a valid token
        let token = create_session_token_from_keypair(
            "test_public_key".to_string(),
            "test_secret_key".to_string(),
            None,
            Some(300),
        ).unwrap();

        // Create deeplink with pubky:// scheme (a known/supported scheme)
        let deeplink = format!("pubky://paykit/connect?token={}&return_url=home", token.token);
        let decoded = Scanner::decode(deeplink.clone()).await.unwrap();

        match decoded {
            Scanner::PaykitSession { data } => {
                assert_eq!(data.url, deeplink);
                assert_eq!(data.action, "connect");
                assert_eq!(data.token, token.token);
                assert_eq!(data.parameters.get("return_url").unwrap(), "home");
            }
            _ => panic!("Should be a PaykitSession"),
        }
    }

    #[tokio::test]
    async fn test_paykit_deeplink_with_paykit_scheme() {
        use crate::paykit::create_session_token_from_keypair;

        // Create a valid token
        let token = create_session_token_from_keypair(
            "test_public_key".to_string(),
            "test_secret_key".to_string(),
            None,
            Some(300),
        ).unwrap();

        // Create deeplink with paykit:// scheme directly
        let deeplink = format!("paykit://session?token={}", token.token);
        let decoded = Scanner::decode(deeplink.clone()).await.unwrap();

        match decoded {
            Scanner::PaykitSession { data } => {
                assert_eq!(data.url, deeplink);
                assert_eq!(data.action, "session");
                assert_eq!(data.token, token.token);
            }
            _ => panic!("Should be a PaykitSession"),
        }
    }

    #[tokio::test]
    async fn test_unknown_scheme_not_recognized_as_paykit() {
        // Unknown schemes should not be recognized as paykit deeplinks
        // This prevents ambiguity with different transports (e.g., iroh://)
        let unknown_scheme = "iroh://paykit/session?token=abc123";
        let result = Scanner::decode(unknown_scheme.to_string()).await;

        // Should fail or not be recognized as PaykitSession
        match result {
            Ok(Scanner::PaykitSession { .. }) => {
                panic!("Unknown scheme should not be recognized as PaykitSession");
            }
            _ => {
                // Expected - either error or different scanner type
            }
        }
    }

    #[tokio::test]
    async fn test_paykit_deeplink_with_https() {
        use crate::paykit::create_session_token_from_keypair;

        // Create a valid token
        let token = create_session_token_from_keypair(
            "test_public_key".to_string(),
            "test_secret_key".to_string(),
            Some("https://homeserver.example".to_string()),
            None,
        ).unwrap();

        // Create HTTPS deeplink
        let deeplink = format!("https://app.example.com/paykit/session?token={}", token.token);
        let decoded = Scanner::decode(deeplink.clone()).await.unwrap();

        match decoded {
            Scanner::PaykitSession { data } => {
                assert_eq!(data.url, deeplink);
                assert_eq!(data.action, "session");
                assert_eq!(data.token, token.token);
            }
            _ => panic!("Should be a PaykitSession"),
        }
    }

    #[tokio::test]
    async fn test_invalid_paykit_deeplink() {
        // Test with missing token (using a known scheme)
        let invalid_deeplink = "bitkit://paykit/session";
        let result = Scanner::decode(invalid_deeplink.to_string()).await;
        assert!(result.is_err(), "Should fail with missing token");

        // Test with completely invalid token (not base64) using a known scheme
        let invalid_token = "bitkit://paykit/session?token=!!!invalid!!!";
        let result = Scanner::decode(invalid_token.to_string()).await;
        // Should parse URL but fail on token validation in actual usage
        // Scanner itself just extracts the data, validation happens later
        match result {
            Ok(Scanner::PaykitSession { data }) => {
                // Scanner extracts it, but token is invalid
                assert!(data.token.contains("!!!"));
            }
            _ => panic!("Should parse as PaykitSession even with invalid token"),
        }
    }

    #[tokio::test]
    async fn test_paykit_deeplink_priority() {
        // Ensure paykit deeplinks are recognized before other formats
        use crate::paykit::create_session_token_from_keypair;

        let token = create_session_token_from_keypair(
            "key".to_string(),
            "secret".to_string(),
            None,
            None,
        ).unwrap();

        // Test that bitkit://paykit/... is handled as paykit, not generic bitkit
        let deeplink = format!("bitkit://paykit/session?token={}", token.token);
        let decoded = Scanner::decode(deeplink.clone()).await.unwrap();

        match decoded {
            Scanner::PaykitSession { .. } => {
                // Success - handled as paykit
            }
            _ => panic!("bitkit://paykit/ should be handled as PaykitSession"),
        }
    }
}