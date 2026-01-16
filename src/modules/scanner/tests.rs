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
    async fn test_uppercase_raw_address() {
        // Test uppercase raw address (bech32 is case-insensitive)
        let address_upper = "BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ";
        let decoded = Scanner::decode(address_upper.to_string()).await;
        match decoded {
            Ok(Scanner::OnChain { invoice }) => {
                // Address should be normalized to lowercase
                assert_eq!(invoice.address, address_upper.to_lowercase());
            },
            Ok(_) => assert!(false, "Should be an OnChain invoice"),
            Err(e) => panic!("Failed to decode uppercase address: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_legacy_address() {
        // Test legacy P2PKH address (case-sensitive, should NOT be lowercased)
        let legacy = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"; // Genesis block address
        let decoded = Scanner::decode(legacy.to_string()).await;
        match decoded {
            Ok(Scanner::OnChain { invoice }) => {
                // Legacy addresses are case-sensitive, should preserve original case
                assert_eq!(invoice.address, legacy);
            },
            Ok(_) => assert!(false, "Should be an OnChain invoice"),
            Err(e) => panic!("Failed to decode legacy address: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_prefix_with_bech32() {
        // Test bitcoin: prefix with bech32 address (should lowercase)
        let invoice = "bitcoin:bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
        let decoded = Scanner::decode(invoice.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                assert_eq!(invoice.address, "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_prefix_with_uppercase_bech32() {
        // Test bitcoin: prefix with uppercase bech32 (should lowercase)
        let invoice = "bitcoin:BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ";
        let decoded = Scanner::decode(invoice.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                assert_eq!(invoice.address, "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_prefix_with_legacy_p2pkh() {
        // Test bitcoin: prefix with legacy P2PKH (should preserve case)
        let invoice = "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
        let decoded = Scanner::decode(invoice.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                assert_eq!(invoice.address, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_prefix_with_legacy_p2sh() {
        // Test bitcoin: prefix with legacy P2SH (should preserve case)
        let invoice = "bitcoin:3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy";
        let decoded = Scanner::decode(invoice.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                assert_eq!(invoice.address, "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_prefix_uppercase_with_legacy() {
        // Test BITCOIN: prefix (uppercase) with legacy address (should preserve address case)
        let invoice = "BITCOIN:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
        let decoded = Scanner::decode(invoice.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                // Prefix gets lowercased, but address case is preserved
                assert_eq!(invoice.address, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_prefix_with_query_params() {
        // Test bitcoin: prefix with bech32 address and query params
        let invoice = "bitcoin:BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ?amount=0.00001&label=Test";
        let decoded = Scanner::decode(invoice.to_string()).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                // Address should be lowercased, query params preserved
                assert_eq!(invoice.address, "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq");
                assert_eq!(invoice.amount_satoshis, 1000);
                assert_eq!(invoice.label.as_ref().unwrap(), "Test");
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
    async fn test_uppercase_lightning_prefix() {
        // Test uppercase LIGHTNING: prefix (common in QR codes)
        let invoice = "LIGHTNING:LNBC543210N1PNJDRVFPP5S720F4Z6WZVJWPDNRLPFFGCT375L46YU9C6CPE7GDVVDFAY47CNSDQQCQZZSXQRRSSSP53UTY4KFW8K3WMW4GA802UDAVZ7E64TC7DMAZ2CMTKJ9SRFXAQ3PS9P4GQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQPQYSGQWL2TDHZM9E6MTEDT7A4263YW7DQXEHDWJNJK23R4G8TUPPK6RS994F6SCUNWSEV3W207TJLDWKPDT32RCEGZPHGK05C0LCTV8HE7SMGQYFN5XQ".to_string();
        let decoded = Scanner::decode(invoice).await.unwrap();
        match decoded {
            Scanner::Lightning { invoice } => {
                assert_eq!(invoice.is_expired, true);
                assert_eq!(invoice.amount_satoshis, 54321);
                assert!(invoice.payee_node_id.is_some());
            }
            _ => assert!(false, "Should be a Lightning invoice"),
        }
    }

    #[tokio::test]
    async fn test_uppercase_bitcoin_uri() {
        // Test uppercase BITCOIN: prefix with uppercase address (common in QR codes)
        let invoice = "BITCOIN:BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ?amount=0.00001&label=Test".to_string();
        let decoded = Scanner::decode(invoice).await.unwrap();
        match decoded {
            Scanner::OnChain { invoice } => {
                // Address should be normalized to lowercase
                assert_eq!(invoice.address, "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq");
                assert_eq!(invoice.amount_satoshis, 1000);
                // Query params should preserve original case
                assert_eq!(invoice.label.as_ref().unwrap(), "Test");
            },
            _ => assert!(false, "Should be an OnChain invoice"),
        }
    }
}