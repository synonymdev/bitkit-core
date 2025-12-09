#[cfg(test)]
mod tests {
    use crate::paykit::*;

    // ===== Error Tests =====

    #[test]
    fn test_error_conversions() {
        // Test Transport error
        let transport_err = PaykitError::Transport("Connection failed".to_string());
        assert_eq!(
            format!("{}", transport_err),
            "Transport error: Connection failed"
        );

        // Test InvalidPublicKey error
        let pk_err = PaykitError::InvalidPublicKey("Invalid format".to_string());
        assert_eq!(
            format!("{}", pk_err),
            "Invalid public key: Invalid format"
        );

        // Test SessionError
        let session_err = PaykitError::SessionError("Session expired".to_string());
        assert_eq!(
            format!("{}", session_err),
            "Session error: Session expired"
        );
    }

    // ===== Type Tests =====

    #[test]
    fn test_method_id_constants() {
        assert_eq!(MethodId::LIGHTNING, "lightning");
        assert_eq!(MethodId::ONCHAIN, "onchain");
        assert_eq!(MethodId::BOLT11, "bolt11");
        assert_eq!(MethodId::BOLT12, "bolt12");
        assert_eq!(MethodId::LNURL, "lnurl");
    }

    #[test]
    fn test_method_id_creation() {
        let method1 = MethodId::lightning();
        assert_eq!(method1.id, "lightning");

        let method2 = MethodId::new("custom");
        assert_eq!(method2.id, "custom");

        let method3 = MethodId::new(String::from("string_method"));
        assert_eq!(method3.id, "string_method");
    }

    #[test]
    fn test_public_key_creation() {
        let key1 = PublicKey::new("test_key");
        assert_eq!(key1.key, "test_key");

        let key2 = PublicKey::new(String::from("string_key"));
        assert_eq!(key2.key, "string_key");
    }

    #[test]
    fn test_public_key_validation_edge_cases() {
        // Test minimum valid length (32 chars)
        let min_valid = PublicKey::new("a".repeat(32));
        assert!(min_valid.validate().is_ok());

        // Test maximum valid length (256 chars)
        let max_valid = PublicKey::new("a".repeat(256));
        assert!(max_valid.validate().is_ok());

        // Test just below minimum
        let below_min = PublicKey::new("a".repeat(31));
        assert!(below_min.validate().is_err());

        // Test just above maximum
        let above_max = PublicKey::new("a".repeat(257));
        assert!(above_max.validate().is_err());
    }

    // ===== Transport Tests =====

    #[test]
    fn test_unauthenticated_transport_creation() {
        // This test will fail in CI without a Pubky network, but demonstrates the API
        let result = PubkyUnauthenticatedTransport::new();
        // We expect this to either succeed or fail with a transport error, not unimplemented
        match result {
            Ok(_) => println!("Successfully created unauthenticated transport"),
            Err(PaykitError::Transport(..)) => {
                println!("Transport error (expected in test environment)")
            }
            Err(e) => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_authenticated_transport_creation() {
        // Test that direct creation returns appropriate error
        let result = PubkyAuthenticatedTransport::new();

        assert!(result.is_err());
        match result {
            Err(PaykitError::SessionError(message)) => {
                assert!(message.contains("Direct session creation not supported"));
            }
            _ => panic!("Expected SessionError"),
        }
    }

    #[test]
    fn test_error_display_formatting() {
        let errors = vec![
            PaykitError::Unimplemented("Feature X".to_string()),
            PaykitError::Transport("Connection timeout".to_string()),
            PaykitError::InvalidPublicKey("Invalid hex format".to_string()),
            PaykitError::InvalidMethodId("Empty method ID".to_string()),
            PaykitError::InvalidEndpointData("Invalid JSON".to_string()),
            PaykitError::SessionError("Not authenticated".to_string()),
        ];

        for error in errors {
            let formatted = format!("{}", error);
            assert!(!formatted.is_empty());
            println!("Error format: {}", formatted);
        }
    }

    #[test]
    fn test_public_key_string_conversions() {
        let original = "test_public_key_string_123456789012345678901234567890";

        // Test FromStr
        let from_str: Result<PublicKey, _> = original.parse();
        assert!(from_str.is_ok());
        let key = from_str.unwrap();
        assert_eq!(key.key, original);

        // Test Display
        let displayed = format!("{}", key);
        assert_eq!(displayed, original);

        // Round trip
        let round_trip: Result<PublicKey, _> = displayed.parse();
        assert!(round_trip.is_ok());
        assert_eq!(round_trip.unwrap().key, original);
    }
}
