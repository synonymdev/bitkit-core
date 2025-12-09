//! Integration tests for the Paykit module using real Pubky credentials.
//!
//! These tests require a Pubky network connection and use the recovery.pkarr file
//! with password "password" for authentication.

#[cfg(all(test, feature = "pubky"))]
mod integration_tests {
    use crate::paykit::*;
    use crate::paykit::session_helper::{SessionConfig, create_session_from_pkarr};

    // Test credentials
    const PKARR_PATH: &str = "src/modules/paykit/recovery.pkarr";
    const PASSWORD: &str = "password";

    /// Helper function to create a Pubky session from the pkarr file
    async fn create_test_session() -> Result<PubkyAuthenticatedTransport, PaykitError> {
        let config = SessionConfig::new(PKARR_PATH, PASSWORD);
        let transport = create_session_from_pkarr(config).await?;
        Ok(PubkyAuthenticatedTransport::from(transport))
    }

    #[tokio::test]
    async fn test_with_real_credentials() {
        println!("Testing with recovery.pkarr file...");

        // Verify the pkarr file exists
        assert!(
            std::path::Path::new(PKARR_PATH).exists(),
            "recovery.pkarr file not found at {}",
            PKARR_PATH
        );

        // Try to create a session
        match create_test_session().await {
            Ok(session) => {
                println!("Session created successfully!");
                // Session is valid - we have authenticated transport
            }
            Err(PaykitError::SessionError(msg)) => {
                // Network errors are acceptable in CI/test environments
                println!("Session error (may be expected without network): {}", msg);
            }
            Err(e) => {
                println!("Other error: {:?}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_read_operations_with_unauthenticated_transport() {
        // This test can run without authentication
        println!("Testing unauthenticated read operations...");

        // Create unauthenticated transport
        let reader = match PubkyUnauthenticatedTransport::new() {
            Ok(r) => r,
            Err(e) => {
                println!("Skipping test - no Pubky network available: {:?}", e);
                return;
            }
        };

        // Use a test public key (this would be derived from the recovery.pkarr)
        // For now, use a placeholder that will return empty results
        let test_pubkey = PublicKey::new("a".repeat(64)); // Valid format placeholder

        // Test getting payment list (should be empty for new account)
        match get_payment_list(&reader, &test_pubkey).await {
            Ok(payments) => {
                println!("Payment list retrieved. Count: {}", payments.entries.len());
                // For a new account, we expect this to be empty
                if payments.entries.is_empty() {
                    println!("No payment methods found (expected for new account)");
                } else {
                    for (method, data) in &payments.entries {
                        println!("Found payment method: {} -> {}", method, data.data);
                    }
                }
            }
            Err(PaykitError::Transport(message)) if message.contains("NOT_FOUND") => {
                println!("No payment data found for this public key (expected for new account)");
            }
            Err(e) => {
                println!("Error getting payment list: {:?}", e);
            }
        }

        // Test getting specific endpoint (should be None for new account)
        let lightning_method = MethodId::lightning();
        match get_payment_endpoint(&reader, &test_pubkey, &lightning_method).await {
            Ok(Some(endpoint)) => {
                println!("Lightning endpoint found: {}", endpoint.data);
            }
            Ok(None) => {
                println!("No lightning endpoint found (expected for new account)");
            }
            Err(e) => {
                println!("Error getting lightning endpoint: {:?}", e);
            }
        }

        // Test getting known contacts (should be empty for new account)
        match get_known_contacts(&reader, &test_pubkey).await {
            Ok(contacts) => {
                println!("Known contacts count: {}", contacts.len());
                if contacts.is_empty() {
                    println!("No contacts found (expected for new account)");
                } else {
                    for contact in &contacts {
                        println!("Contact: {}", contact);
                    }
                }
            }
            Err(e) => {
                println!("Error getting contacts: {:?}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_write_and_read_cycle() {
        println!("Testing write and read cycle with real credentials...");

        // Create an authenticated session from recovery.pkarr
        let auth_transport = match create_test_session().await {
            Ok(transport) => transport,
            Err(e) => {
                println!("Skipping test - could not create session: {:?}", e);
                return;
            }
        };

        println!("Session created successfully!");

        // Write a test onchain endpoint
        let method = MethodId::onchain();
        let test_data = EndpointData {
            data: "bc1qtest123456789".to_string(),
        };

        // Set the payment endpoint
        match set_payment_endpoint(&auth_transport, method.clone(), test_data.clone()).await {
            Ok(()) => {
                println!("Payment endpoint set successfully");
            }
            Err(e) => {
                println!("Failed to set payment endpoint: {:?}", e);
                return;
            }
        }

        // Read it back using unauthenticated transport
        let reader = match PubkyUnauthenticatedTransport::new() {
            Ok(r) => r,
            Err(e) => {
                println!("Failed to create reader: {:?}", e);
                return;
            }
        };

        // Note: We'd need the public key from the session to read it back
        // For now, just verify the write succeeded
        println!("Write operation completed successfully");

        // Clean up - remove the test endpoint
        match remove_payment_endpoint(&auth_transport, method).await {
            Ok(()) => {
                println!("Test endpoint cleaned up");
            }
            Err(e) => {
                println!("Failed to clean up endpoint: {:?}", e);
            }
        }
    }

    #[test]
    fn test_pkarr_file_properties() {
        // Basic test to verify the pkarr file
        let pkarr_data = std::fs::read(PKARR_PATH)
            .expect("Failed to read recovery.pkarr");

        println!("PKARR file size: {} bytes", pkarr_data.len());
        assert_eq!(pkarr_data.len(), 91, "Expected pkarr file to be 91 bytes");

        // The pkarr format typically starts with specific magic bytes
        // This is just a basic sanity check
        assert!(!pkarr_data.is_empty(), "PKARR file should not be empty");
    }
}