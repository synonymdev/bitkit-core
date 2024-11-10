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
}