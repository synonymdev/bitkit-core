//const STAGING_SERVER: &str = "https://api1.blocktank.to/api";
//const STAGING_SERVER: &str = "https://api.stag.blocktank.to/api";
const STAGING_SERVER: &str = "https://api.stag.blocktank.to/blocktank/api/v2";

#[cfg(test)]
mod tests {
    use rust_blocktank_client::*;
    use crate::modules::blocktank::{BlocktankDB, BlocktankError};
    use super::*;

    #[tokio::test]
    async fn test_upsert_info() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Create test data
        let test_info = IBtInfo {
            version: 1,
            nodes: vec![ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }],
            options: IBtInfoOptions {
                min_channel_size_sat: 20000,
                max_channel_size_sat: 5000000,
                min_expiry_weeks: 2,
                max_expiry_weeks: 52,
                min_payment_confirmations: 1,
                min_high_risk_payment_confirmations: 6,
                max_0_conf_client_balance_sat: 100000,
                max_client_balance_sat: 5000000,
            },
            versions: IBtInfoVersions {
                http: "1.0".to_string(),
                btc: "0.21.0".to_string(),
                ln2: "0.10.0".to_string(),
            },
            onchain: IBtInfoOnchain {
                network: BitcoinNetworkEnum::Testnet,
                fee_rates: FeeRates {
                    fast: 10,
                    mid: 5,
                    slow: 1,
                },
            },
        };

        // Test initial insert
        let result = db.upsert_info(&test_info).await;
        assert!(result.is_ok(), "Failed to insert info: {:?}", result.err());

        // Verify the insert
        {
            let conn = db.conn.lock().await;
            let row = conn.query_row(
                "SELECT version, is_current FROM info WHERE version = 1",
                [],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, bool>(1)?))
            ).unwrap();
            assert_eq!(row, (1, true));
        } // Lock is dropped here

        // Create updated test data
        let mut updated_info = test_info.clone();
        updated_info.version = 2;
        updated_info.nodes[0].alias = "updated_node".to_string();

        // Test update
        let result = db.upsert_info(&updated_info).await;
        assert!(result.is_ok(), "Failed to update info: {:?}", result.err());

        // Verify the update and JSON serialization
        {
            let conn = db.conn.lock().await;

            // Check version statuses
            let rows: Vec<(u32, bool)> = conn.prepare("SELECT version, is_current FROM info ORDER BY version")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();

            assert_eq!(rows.len(), 2, "Should have two versions");
            assert_eq!(rows[0], (1, false), "Version 1 should not be current");
            assert_eq!(rows[1], (2, true), "Version 2 should be current");

            // Verify JSON serialization
            let node_data: String = conn.query_row(
                "SELECT nodes FROM info WHERE version = 2",
                [],
                |row| row.get(0)
            ).unwrap();

            let nodes: Vec<ILspNode> = serde_json::from_str(&node_data).unwrap();
            assert_eq!(nodes[0].alias, "updated_node");
        } // Lock is dropped here
    }

    #[tokio::test]
    async fn test_fetch_and_store_info() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Test fetch and store
        let result = db.fetch_and_store_info().await;
        assert!(result.is_ok(), "Failed to fetch and store info: {:?}", result.err());

        let info = result.unwrap();

        // Verify the stored info matches what was returned
        {
            let conn = db.conn.lock().await;

            // Verify version is stored
            let row = conn.query_row(
                "SELECT version, is_current FROM info WHERE version = ?1",
                [info.version],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, bool>(1)?))
            ).unwrap();
            assert_eq!(row.0, info.version);
            assert_eq!(row.1, true);

            // Verify JSON data
            let nodes_json: String = conn.query_row(
                "SELECT nodes FROM info WHERE version = ?1",
                [info.version],
                |row| row.get(0)
            ).unwrap();

            let stored_nodes: Vec<ILspNode> = serde_json::from_str(&nodes_json).unwrap();
            assert_eq!(stored_nodes.len(), info.nodes.len());

            // Compare first node's data if any exist
            if !info.nodes.is_empty() {
                assert_eq!(stored_nodes[0].alias, info.nodes[0].alias);
                assert_eq!(stored_nodes[0].pubkey, info.nodes[0].pubkey);
            }
        }
    }

    #[tokio::test]
    async fn test_fetch_and_store_info_error_handling() {
        // Initialize with invalid URL to test error handling
        let db = BlocktankDB::new(":memory:", Some("http://invalid-url")).await.unwrap();

        // Test fetch and store with invalid URL
        let result = db.fetch_and_store_info().await;
        assert!(result.is_err(), "Expected error for invalid URL");

        match result {
            Err(BlocktankError::DataError { error_details }) => {
                assert!(error_details.contains("Failed to fetch info from Blocktank"));
            }
            _ => panic!("Expected DataError"),
        }

        // Verify no data was stored
        {
            let conn = db.conn.lock().await;
            let count: u32 = conn.query_row(
                "SELECT COUNT(*) FROM info",
                [],
                |row| row.get(0)
            ).unwrap();
            assert_eq!(count, 0, "No data should be stored when fetch fails");
        }
    }

    #[tokio::test]
    async fn test_get_info() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Should return None when no info exists
        let empty_result = db.get_info().await.unwrap();
        assert!(empty_result.is_none());

        // Create and store test info
        let test_info = IBtInfo {
            version: 1,
            nodes: vec![ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }],
            options: IBtInfoOptions {
                min_channel_size_sat: 20000,
                max_channel_size_sat: 5000000,
                min_expiry_weeks: 2,
                max_expiry_weeks: 52,
                min_payment_confirmations: 1,
                min_high_risk_payment_confirmations: 6,
                max_0_conf_client_balance_sat: 100000,
                max_client_balance_sat: 5000000,
            },
            versions: IBtInfoVersions {
                http: "1.0".to_string(),
                btc: "0.21.0".to_string(),
                ln2: "0.10.0".to_string(),
            },
            onchain: IBtInfoOnchain {
                network: BitcoinNetworkEnum::Testnet,
                fee_rates: FeeRates {
                    fast: 10,
                    mid: 5,
                    slow: 1,
                },
            },
        };

        // Store the info
        db.upsert_info(&test_info).await.unwrap();

        // Retrieve and verify
        let stored_info = db.get_info().await.unwrap().unwrap();
        assert_eq!(stored_info.version, test_info.version);
        assert_eq!(stored_info.nodes[0].alias, test_info.nodes[0].alias);
        assert_eq!(stored_info.nodes[0].pubkey, test_info.nodes[0].pubkey);
        assert_eq!(stored_info.options.min_channel_size_sat, test_info.options.min_channel_size_sat);
        assert_eq!(stored_info.versions.http, test_info.versions.http);
        assert_eq!(stored_info.onchain.network, test_info.onchain.network);
        assert_eq!(stored_info.onchain.fee_rates.fast, test_info.onchain.fee_rates.fast);
    }


    #[tokio::test]
    async fn test_upsert_order() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Get current timestamp in ISO 8601 format
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        let test_order = IBtOrder {
            id: "test_order_1".to_string(),
            state: BtOrderState::Created,
            state2: BtOrderState2::Created,
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            lsp_balance_sat: 10000,
            client_balance_sat: 5000,
            zero_conf: false,
            zero_reserve: false,
            client_node_id: Some("client123".to_string()),
            channel_expiry_weeks: 2,
            channel_expires_at: future.to_rfc3339(),  // Changed from integer to ISO string
            order_expires_at: future.to_rfc3339(),    // Changed from integer to ISO string
            channel: None,
            lsp_node: ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            },
            lnurl: None,
            payment: IBtPayment {
                state: BtPaymentState::Created,
                state2: BtPaymentState2::Created,
                paid_sat: 0,
                bolt11_invoice: IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future.to_rfc3339(),   // Changed from integer to ISO string
                    updated_at: now.to_rfc3339(),      // Changed from integer to ISO string
                },
                onchain: IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                },
                is_manually_paid: None,
                manual_refunds: None,
            },
            coupon_code: None,
            source: None,
            discount: None,
            updated_at: now.to_rfc3339(),     // Changed from integer to ISO string
            created_at: now.to_rfc3339(),     // Changed from integer to ISO string
        };

        // Test initial insert
        let result = db.upsert_order(&test_order).await;
        assert!(result.is_ok(), "Failed to insert order: {:?}", result.err());

        // Verify the insert
        {
            let conn = db.conn.lock().await;
            let row = conn.query_row(
                "SELECT id, state, fee_sat FROM orders WHERE id = ?1",
                [&test_order.id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, u64>(2)?))
            ).unwrap();

            assert_eq!(row.0, test_order.id);
            assert_eq!(row.1, format!("{:?}", test_order.state));
            assert_eq!(row.2, test_order.fee_sat);

            // Verify JSON serialization
            let lsp_node_json: String = conn.query_row(
                "SELECT lsp_node_data FROM orders WHERE id = ?1",
                [&test_order.id],
                |row| row.get(0)
            ).unwrap();

            let stored_lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).unwrap();
            assert_eq!(stored_lsp_node.alias, test_order.lsp_node.alias);
            assert_eq!(stored_lsp_node.pubkey, test_order.lsp_node.pubkey);
        }

        // Test update
        let mut updated_order = test_order.clone();
        updated_order.fee_sat = 2000;
        updated_order.state = BtOrderState::Open;
        updated_order.updated_at = chrono::Utc::now().to_rfc3339();  // Update timestamp

        let result = db.upsert_order(&updated_order).await;
        assert!(result.is_ok(), "Failed to update order: {:?}", result.err());

        // Verify the update
        {
            let conn = db.conn.lock().await;
            let row = conn.query_row(
                "SELECT state, fee_sat FROM orders WHERE id = ?1",
                [&updated_order.id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            ).unwrap();

            assert_eq!(row.0, format!("{:?}", updated_order.state));
            assert_eq!(row.1, updated_order.fee_sat);
        }
    }

    #[tokio::test]
    async fn test_create_and_store_order() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        let timestamp = chrono::Utc::now().to_rfc3339();

        let options = CreateOrderOptions {
            coupon_code: "".to_string(),
            zero_conf: true,
            zero_reserve: false,
            announce_channel: false,
            ..Default::default()
        };

        let result = db.create_and_store_order(100000, 4, Some(options)).await;
        assert!(result.is_ok(), "Failed to create and store order: {:?}", result.err());

        let order = result.unwrap();
        assert_eq!(order.lsp_balance_sat, 100000);
        assert_eq!(order.client_balance_sat, 0);

    }

    #[tokio::test]
    async fn test_refresh_orders() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Create actual orders through the API
        let options = CreateOrderOptions {
            client_balance_sat: 0,
            coupon_code: "".to_string(),
            zero_conf: true,
            zero_reserve: false,
            announce_channel: false,
            ..Default::default()
        };

        // Create two real orders
        println!("Creating first test order...");
        let order1 = db.create_and_store_order(100000, 4, Some(options.clone())).await
            .expect("Failed to create first order");

        println!("First order created with ID: {}", order1.id);

        // Add a small delay between order creations
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        println!("Creating second test order...");
        let order2 = db.create_and_store_order(150000, 4, Some(options.clone())).await
            .expect("Failed to create second order");

        println!("Second order created with ID: {}", order2.id);

        // Add a small delay before refreshing
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Test refreshing orders
        let order_ids = vec![order1.id.clone(), order2.id.clone()];
        println!("Attempting to refresh orders with IDs: {:?}", order_ids);

        let result = db.refresh_orders(&order_ids).await;

        match result {
            Ok(refreshed_orders) => {
                assert_eq!(refreshed_orders.len(), 2, "Should have refreshed 2 orders");

                // Verify each refreshed order
                let mut found_order1 = false;
                let mut found_order2 = false;

                for order in &refreshed_orders {
                    println!("Checking refreshed order: {}", order.id);

                    if order.id == order1.id {
                        found_order1 = true;
                        assert_eq!(order.lsp_balance_sat, 100000);
                        assert_eq!(order.client_balance_sat, 0);
                    } else if order.id == order2.id {
                        found_order2 = true;
                        assert_eq!(order.lsp_balance_sat, 150000);
                        assert_eq!(order.client_balance_sat, 0);
                    }

                    // Verify database state
                    let conn = db.conn.lock().await;
                    let row = conn.query_row(
                        "SELECT id, state, fee_sat FROM orders WHERE id = ?1",
                        [&order.id],
                        |row| Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u64>(2)?
                        ))
                    ).unwrap();

                    assert_eq!(row.0, order.id);
                    assert_eq!(row.1, format!("{:?}", order.state));
                    assert_eq!(row.2, order.fee_sat);
                }

                assert!(found_order1, "First order not found in refreshed orders");
                assert!(found_order2, "Second order not found in refreshed orders");
            },
            Err(e) => panic!("Failed to refresh orders: {:?}", e),
        }

        // Test error handling with invalid order IDs
        let invalid_ids = vec!["invalid_id_1".to_string()];
        let error_result = db.refresh_orders(&invalid_ids).await;
        assert!(error_result.is_err(), "Expected error for invalid order IDs");

        match error_result {
            Err(BlocktankError::DataError { error_details }) => {
                assert!(error_details.contains("Failed to fetch orders"));
            },
            _ => panic!("Expected DataError"),
        }
    }

    #[tokio::test]
    async fn test_get_orders() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Get current timestamp and future timestamp as integers
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);
        let now_unix = now.timestamp();
        let future_unix = future.timestamp();

        // Create multiple test orders with different states
        let test_order1 = IBtOrder {
            id: "test_order_1".to_string(),
            state: BtOrderState::Created,
            state2: BtOrderState2::Created,
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            lsp_balance_sat: 10000,
            client_balance_sat: 5000,
            zero_conf: false,
            zero_reserve: false,
            client_node_id: Some("client123".to_string()),
            channel_expiry_weeks: 2,
            channel_expires_at: future_unix.to_string(),
            order_expires_at: future_unix.to_string(),
            channel: None,
            lsp_node: ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            },
            lnurl: None,
            payment: IBtPayment {
                state: BtPaymentState::Created,
                state2: BtPaymentState2::Created,
                paid_sat: 0,
                bolt11_invoice: IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future_unix.to_string(),
                    updated_at: now_unix.to_string(),
                },
                onchain: IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                },
                is_manually_paid: None,
                manual_refunds: None,
            },
            coupon_code: None,
            source: None,
            discount: None,
            updated_at: now_unix.to_string(),
            created_at: now_unix.to_string(),
        };

        let mut test_order2 = test_order1.clone();
        test_order2.id = "test_order_2".to_string();
        test_order2.state = BtOrderState::Open;
        test_order2.state2 = BtOrderState2::Executed;
        test_order2.fee_sat = 2000;

        let mut test_order3 = test_order1.clone();
        test_order3.id = "test_order_3".to_string();
        test_order3.state = BtOrderState::Closed;
        test_order3.state2 = BtOrderState2::Paid;
        test_order3.fee_sat = 3000;

        // Insert test orders
        db.upsert_order(&test_order1).await.unwrap();
        db.upsert_order(&test_order2).await.unwrap();
        db.upsert_order(&test_order3).await.unwrap();

        // Test 1: Get all orders
        let all_orders = db.get_orders(None, None).await.unwrap();
        assert_eq!(all_orders.len(), 3, "Should retrieve all 3 orders");

        // Test 2: Get specific orders by ID
        let specific_orders = db.get_orders(
            Some(&vec![test_order1.id.clone(), test_order2.id.clone()]),
            None
        ).await.unwrap();
        assert_eq!(specific_orders.len(), 2, "Should retrieve 2 specific orders");
        assert!(specific_orders.iter().any(|o| o.id == test_order1.id));
        assert!(specific_orders.iter().any(|o| o.id == test_order2.id));

        // Test 3: Filter by state
        let paid_orders = db.get_orders(None, Some(BtOrderState2::Paid)).await.unwrap();
        assert_eq!(paid_orders.len(), 1, "Should retrieve 1 paid order");
        assert_eq!(paid_orders[0].id, test_order3.id);

        // Test 4: Verify complex fields deserialization
        let order = &all_orders[0];
        assert!(!order.lsp_node.connection_strings.is_empty());
        assert_eq!(order.payment.state, test_order1.payment.state);
        assert_eq!(order.payment.bolt11_invoice.state, test_order1.payment.bolt11_invoice.state);

        // Test 5: Test with non-existent order IDs
        let non_existent = db.get_orders(
            Some(&vec!["non_existent_id".to_string()]),
            None
        ).await.unwrap();
        assert_eq!(non_existent.len(), 0, "Should return empty vector for non-existent IDs");

        // Test 6: Test with invalid state filter
        let executed_orders = db.get_orders(None, Some(BtOrderState2::Executed)).await.unwrap();
        assert_eq!(executed_orders.len(), 1, "Should retrieve 1 executed order");
        assert_eq!(executed_orders[0].id, test_order2.id);
    }

    #[tokio::test]
    async fn test_get_active_orders() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Get current timestamp and future timestamp
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        // Create test orders with different states
        let test_order1 = IBtOrder {
            id: "test_order_1".to_string(),
            state: BtOrderState::Created,
            state2: BtOrderState2::Created,  // This should be included in active orders
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            lsp_balance_sat: 10000,
            client_balance_sat: 5000,
            zero_conf: false,
            zero_reserve: false,
            client_node_id: Some("client123".to_string()),
            channel_expiry_weeks: 2,
            channel_expires_at: future.to_rfc3339(),
            order_expires_at: future.to_rfc3339(),
            channel: None,
            lsp_node: ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            },
            lnurl: None,
            payment: IBtPayment {
                state: BtPaymentState::Created,
                state2: BtPaymentState2::Created,
                paid_sat: 0,
                bolt11_invoice: IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future.to_rfc3339(),
                    updated_at: now.to_rfc3339(),
                },
                onchain: IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                },
                is_manually_paid: None,
                manual_refunds: None,
            },
            coupon_code: None,
            source: None,
            discount: None,
            updated_at: now.to_rfc3339(),
            created_at: now.to_rfc3339(),
        };

        let mut test_order2 = test_order1.clone();
        test_order2.id = "test_order_2".to_string();
        test_order2.state = BtOrderState::Open;
        test_order2.state2 = BtOrderState2::Paid;  // This should be included in active orders
        test_order2.fee_sat = 2000;

        let mut test_order3 = test_order1.clone();
        test_order3.id = "test_order_3".to_string();
        test_order3.state = BtOrderState::Closed;
        test_order3.state2 = BtOrderState2::Expired;  // This should NOT be included
        test_order3.fee_sat = 3000;

        let mut test_order4 = test_order1.clone();
        test_order4.id = "test_order_4".to_string();
        test_order4.state = BtOrderState::Closed;
        test_order4.state2 = BtOrderState2::Executed;  // This should NOT be included
        test_order4.fee_sat = 4000;

        // Insert all test orders
        db.upsert_order(&test_order1).await.unwrap();
        db.upsert_order(&test_order2).await.unwrap();
        db.upsert_order(&test_order3).await.unwrap();
        db.upsert_order(&test_order4).await.unwrap();

        // Test get_active_orders
        let active_orders = db.get_active_orders().await.unwrap();

        // Verify we only got orders in Created or Paid state
        assert_eq!(active_orders.len(), 2, "Should only retrieve orders in Created or Paid state");

        // Verify the specific orders we got back
        let order_ids: Vec<String> = active_orders.iter()
            .map(|o| o.id.clone())
            .collect();

        assert!(order_ids.contains(&test_order1.id), "Should contain the Created order");
        assert!(order_ids.contains(&test_order2.id), "Should contain the Paid order");

        // Verify the orders are in the correct state
        for order in active_orders {
            assert!(
                matches!(order.state2, BtOrderState2::Created | BtOrderState2::Paid),
                "Order {} should be in Created or Paid state, but was in {:?} state",
                order.id,
                order.state2
            );

            // Verify other fields are correctly loaded
            if order.id == test_order1.id {
                assert_eq!(order.state2, BtOrderState2::Created);
                assert_eq!(order.fee_sat, 1000);
            } else if order.id == test_order2.id {
                assert_eq!(order.state2, BtOrderState2::Paid);
                assert_eq!(order.fee_sat, 2000);
            }
        }
    }

    #[tokio::test]
    async fn test_refresh_active_orders() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Create test orders with different states
        let options = CreateOrderOptions {
            client_balance_sat: 0,
            coupon_code: "".to_string(),
            zero_conf: true,
            zero_reserve: false,
            announce_channel: false,
            ..Default::default()
        };

        // Create orders that should be active (Created and Paid states)
        println!("Creating first test order (Created state)...");
        let order1 = db.create_and_store_order(100000, 4, Some(options.clone())).await
            .expect("Failed to create first order");
        println!("First order created with ID: {}", order1.id);

        // Add a small delay between order creations
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        println!("Creating second test order (will be set to Paid state)...");
        let order2 = db.create_and_store_order(150000, 4, Some(options.clone())).await
            .expect("Failed to create second order");
        println!("Second order created with ID: {}", order2.id);

        // Add a small delay before refreshing
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Test refreshing active orders
        println!("Testing refresh_active_orders...");
        let result = db.refresh_active_orders().await;

        match result {
            Ok(refreshed_orders) => {
                assert_eq!(refreshed_orders.len(), 2, "Should have refreshed 2 active orders");

                // Verify each refreshed order
                let mut found_order1 = false;
                let mut found_order2 = false;

                for order in &refreshed_orders {
                    println!("Checking refreshed order: {}", order.id);

                    if order.id == order1.id {
                        found_order1 = true;
                        assert_eq!(order.lsp_balance_sat, 100000);
                        assert_eq!(order.client_balance_sat, 0);
                        assert!(matches!(order.state2, BtOrderState2::Created));
                    } else if order.id == order2.id {
                        found_order2 = true;
                        assert_eq!(order.lsp_balance_sat, 150000);
                        assert_eq!(order.client_balance_sat, 0);
                    }

                    // Verify order is in an active state
                    assert!(
                        matches!(order.state2, BtOrderState2::Created | BtOrderState2::Paid),
                        "Order should be in Created or Paid state"
                    );
                }

                assert!(found_order1, "First order not found in refreshed orders");
                assert!(found_order2, "Second order not found in refreshed orders");

                // Verify database state
                let conn = db.conn.lock().await;
                for order in &refreshed_orders {
                    let row = conn.query_row(
                        "SELECT id, state2, fee_sat FROM orders WHERE id = ?1",
                        [&order.id],
                        |row| Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u64>(2)?
                        ))
                    ).unwrap();

                    assert_eq!(row.0, order.id);
                    assert!(
                        row.1 == "Created" || row.1 == "Paid",
                        "Database order state should be Created or Paid"
                    );
                }
            },
            Err(e) => panic!("Failed to refresh active orders: {:?}", e),
        }

        // Test with no active orders
        // First, expire all orders to make them inactive
        let conn = db.conn.lock().await;
        conn.execute(
            "UPDATE orders SET state2 = 'Expired'",
            [],
        ).unwrap();
        drop(conn);  // Release the lock

        // Now test refresh_active_orders with no active orders
        let empty_result = db.refresh_active_orders().await.unwrap();
        assert_eq!(empty_result.len(), 0, "Should return empty vec when no active orders exist");
    }

    #[tokio::test]
    async fn test_get_min_zero_conf_tx_fee() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // First create an order to get a valid order ID
        let options = CreateOrderOptions {
            client_balance_sat: 0,
            coupon_code: "".to_string(),
            zero_conf: true,
            zero_reserve: false,
            announce_channel: false,
            ..Default::default()
        };

        println!("Creating test order...");
        let order = db.create_and_store_order(100000, 4, Some(options)).await
            .expect("Failed to create order");

        println!("Order created with ID: {}", order.id);

        // Test getting min zero conf fee
        let result = db.get_min_zero_conf_tx_fee(order.id).await;
        assert!(result.is_ok(), "Failed to get min zero conf fee: {:?}", result.err());

        let fee_window = result.unwrap();
        assert!(fee_window.sat_per_vbyte > 0.0, "sat_per_vbyte should be greater than 0");
        assert!(!fee_window.validity_ends_at.is_empty(), "validity_ends_at should not be empty");

        // Test with invalid order ID
        let error_result = db.get_min_zero_conf_tx_fee("invalid_id".to_string()).await;
        assert!(error_result.is_err(), "Expected error for invalid order ID");

        match error_result {
            Err(BlocktankError::DataError { error_details }) => {
                assert!(error_details.contains("Failed to get minimum zero-conf transaction fee"));
            },
            _ => panic!("Expected DataError"),
        }
    }

    #[tokio::test]
    async fn test_estimate_order_fee() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        let options = Some(CreateOrderOptions {
            client_balance_sat: 0,
            coupon_code: "".to_string(),
            zero_conf: true,
            zero_reserve: false,
            announce_channel: false,
            ..Default::default()
        });

        // Test valid estimation
        let result = db.estimate_order_fee(100000, 4, options.clone()).await;
        assert!(result.is_ok(), "Failed to estimate order fee: {:?}", result.err());

        let fee_estimate = result.unwrap();
        assert!(fee_estimate.fee_sat > 0, "Fee estimate should be greater than 0");
        assert!(fee_estimate.min_0_conf_tx_fee.sat_per_vbyte > 0.0, "min_0_conf_tx_fee.sat_per_vbyte should be greater than 0");

        // Test with invalid parameters
        let error_result = db.estimate_order_fee(0, 0, None).await;
        assert!(error_result.is_err(), "Expected error for invalid parameters");

        match error_result {
            Err(BlocktankError::DataError { error_details }) => {
                assert!(error_details.contains("Failed to estimate order fee"));
            },
            _ => panic!("Expected DataError"),
        }
    }

    #[tokio::test]
    async fn test_estimate_order_fee_full() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        let options = Some(CreateOrderOptions {
            client_balance_sat: 20000,
            coupon_code: "".to_string(),
            zero_conf: true,
            zero_reserve: false,
            announce_channel: false,
            ..Default::default()
        });

        // Test valid full estimation
        let result = db.estimate_order_fee_full(100000, 4, options.clone()).await;
        assert!(result.is_ok(), "Failed to estimate full order fee: {:?}", result.err());

        let fee_estimate = result.unwrap();
        assert!(fee_estimate.fee_sat > 0, "Fee estimate should be greater than 0");
        assert!(fee_estimate.network_fee_sat > 0, "Network fee should be greater than 0");
        assert!(fee_estimate.service_fee_sat > 0, "Service fee should be greater than 0");
        assert!(fee_estimate.min_0_conf_tx_fee.sat_per_vbyte > 0.0, "min_0_conf_tx_fee.sat_per_vbyte should be greater than 0");

        // Test with invalid parameters
        let error_result = db.estimate_order_fee_full(0, 0, None).await;
        assert!(error_result.is_err(), "Expected error for invalid parameters");

        match error_result {
            Err(BlocktankError::DataError { error_details }) => {
                assert!(error_details.contains("Failed to estimate full order fee"));
            },
            _ => panic!("Expected DataError"),
        }
    }

    #[tokio::test]
    async fn test_upsert_cjit_entry() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Get current timestamp in ISO 8601 format
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        let test_entry = ICJitEntry {
            id: "test_cjit_1".to_string(),
            state: CJitStateEnum::Created,
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            channel_size_sat: 100000,
            channel_expiry_weeks: 4,
            channel_open_error: None,
            node_id: "node123".to_string(),
            coupon_code: "TEST".to_string(),
            source: Some("test".to_string()),
            expires_at: future.to_rfc3339(),
            invoice: IBtBolt11Invoice {
                request: "lnbc...".to_string(),
                state: BtBolt11InvoiceState::Pending,
                expires_at: future.to_rfc3339(),
                updated_at: now.to_rfc3339(),
            },
            channel: None,
            lsp_node: ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            },
            discount: None,
            updated_at: now.to_rfc3339(),
            created_at: now.to_rfc3339(),
        };

        // Test initial insert
        let result = db.upsert_cjit_entry(&test_entry).await;
        assert!(result.is_ok(), "Failed to insert CJIT entry: {:?}", result.err());

        // Verify the insert
        {
            let conn = db.conn.lock().await;
            let row = conn.query_row(
                "SELECT id, state, fee_sat FROM cjit_entries WHERE id = ?1",
                [&test_entry.id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, u64>(2)?))
            ).unwrap();

            assert_eq!(row.0, test_entry.id);
            assert_eq!(row.1, format!("{:?}", test_entry.state));
            assert_eq!(row.2, test_entry.fee_sat);

            // Verify JSON serialization
            let lsp_node_json: String = conn.query_row(
                "SELECT lsp_node_data FROM cjit_entries WHERE id = ?1",
                [&test_entry.id],
                |row| row.get(0)
            ).unwrap();

            let stored_lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).unwrap();
            assert_eq!(stored_lsp_node.alias, test_entry.lsp_node.alias);
            assert_eq!(stored_lsp_node.pubkey, test_entry.lsp_node.pubkey);
        }

        // Test update
        let mut updated_entry = test_entry.clone();
        updated_entry.fee_sat = 2000;
        updated_entry.state = CJitStateEnum::Completed;
        updated_entry.updated_at = chrono::Utc::now().to_rfc3339();

        let result = db.upsert_cjit_entry(&updated_entry).await;
        assert!(result.is_ok(), "Failed to update CJIT entry: {:?}", result.err());

        // Verify the update
        {
            let conn = db.conn.lock().await;
            let row = conn.query_row(
                "SELECT state, fee_sat FROM cjit_entries WHERE id = ?1",
                [&updated_entry.id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            ).unwrap();

            assert_eq!(row.0, format!("{:?}", updated_entry.state));
            assert_eq!(row.1, updated_entry.fee_sat);
        }
    }

    #[tokio::test]
    async fn test_get_cjit_entries() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Get current timestamp and future timestamp
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        // Create multiple test entries with different states
        let test_entry1 = ICJitEntry {
            id: "test_cjit_1".to_string(),
            state: CJitStateEnum::Created,
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            channel_size_sat: 100000,
            channel_expiry_weeks: 4,
            channel_open_error: None,
            node_id: "node123".to_string(),
            coupon_code: "TEST1".to_string(),
            source: Some("test".to_string()),
            expires_at: future.to_rfc3339(),
            invoice: IBtBolt11Invoice {
                request: "lnbc...".to_string(),
                state: BtBolt11InvoiceState::Pending,
                expires_at: future.to_rfc3339(),
                updated_at: now.to_rfc3339(),
            },
            channel: None,
            lsp_node: ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            },
            discount: None,
            updated_at: now.to_rfc3339(),
            created_at: now.to_rfc3339(),
        };

        let mut test_entry2 = test_entry1.clone();
        test_entry2.id = "test_cjit_2".to_string();
        test_entry2.state = CJitStateEnum::Completed;
        test_entry2.fee_sat = 2000;
        test_entry2.coupon_code = "TEST2".to_string();

        let mut test_entry3 = test_entry1.clone();
        test_entry3.id = "test_cjit_3".to_string();
        test_entry3.state = CJitStateEnum::Failed;
        test_entry3.fee_sat = 3000;
        test_entry3.coupon_code = "TEST3".to_string();

        // Insert test entries
        db.upsert_cjit_entry(&test_entry1).await.unwrap();
        db.upsert_cjit_entry(&test_entry2).await.unwrap();
        db.upsert_cjit_entry(&test_entry3).await.unwrap();

        // Test 1: Get all entries
        let all_entries = db.get_cjit_entries(None, None).await.unwrap();
        assert_eq!(all_entries.len(), 3, "Should retrieve all 3 entries");

        // Test 2: Get specific entries by ID
        let specific_entries = db.get_cjit_entries(
            Some(&vec![test_entry1.id.clone(), test_entry2.id.clone()]),
            None
        ).await.unwrap();
        assert_eq!(specific_entries.len(), 2, "Should retrieve 2 specific entries");
        assert!(specific_entries.iter().any(|e| e.id == test_entry1.id));
        assert!(specific_entries.iter().any(|e| e.id == test_entry2.id));

        // Test 3: Filter by state
        let created_entries = db.get_cjit_entries(None, Some(CJitStateEnum::Created)).await.unwrap();
        assert_eq!(created_entries.len(), 1, "Should retrieve 1 created entry");
        assert_eq!(created_entries[0].id, test_entry1.id);

        // Test 4: Verify complex fields deserialization
        let entry = &all_entries[0];
        assert!(!entry.lsp_node.connection_strings.is_empty());
        assert_eq!(entry.invoice.state, test_entry1.invoice.state);

        // Test 5: Test with non-existent entry IDs
        let non_existent = db.get_cjit_entries(
            Some(&vec!["non_existent_id".to_string()]),
            None
        ).await.unwrap();
        assert_eq!(non_existent.len(), 0, "Should return empty vector for non-existent IDs");

        // Test 6: Test with completed state filter
        let completed_entries = db.get_cjit_entries(None, Some(CJitStateEnum::Completed)).await.unwrap();
        assert_eq!(completed_entries.len(), 1, "Should retrieve 1 completed entry");
        assert_eq!(completed_entries[0].id, test_entry2.id);

        // Test 7: Verify all fields are correctly loaded for a specific entry
        let specific_entry = &created_entries[0];
        assert_eq!(specific_entry.fee_sat, test_entry1.fee_sat);
        assert_eq!(specific_entry.network_fee_sat, test_entry1.network_fee_sat);
        assert_eq!(specific_entry.service_fee_sat, test_entry1.service_fee_sat);
        assert_eq!(specific_entry.channel_size_sat, test_entry1.channel_size_sat);
        assert_eq!(specific_entry.channel_expiry_weeks, test_entry1.channel_expiry_weeks);
        assert_eq!(specific_entry.node_id, test_entry1.node_id);
        assert_eq!(specific_entry.coupon_code, test_entry1.coupon_code);
        assert_eq!(specific_entry.source, test_entry1.source);
    }

    #[tokio::test]
    async fn test_cjit_entry_integration() {
        // Initialize database with staging server
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Create a test CJIT entry
        let options = Some(CreateCjitOptions {
            source: Some("integration_test".to_string()),
            discount_code: None,
        });

        println!("Creating CJIT entry...");
        let channel_size_sat = 100000; // 100k sats
        let invoice_sat = 20000; // 20k sats
        let invoice_description = "Integration test CJIT entry";
        let node_id = "03c8533232c155c41c42e5a8f8487b192dd36f1d354b86ef461cc82e67e3388839"; // Example node ID
        let channel_expiry_weeks = 4;

        let result = db.create_cjit_entry(
            channel_size_sat,
            invoice_sat,
            invoice_description,
            node_id,
            channel_expiry_weeks,
            options.clone()
        ).await;

        // Verify creation was successful
        assert!(result.is_ok(), "Failed to create CJIT entry: {:?}", result.err());
        let cjit_entry = result.unwrap();

        println!("Created CJIT entry with ID: {}", cjit_entry.id);

        // Verify the entry properties
        assert_eq!(cjit_entry.channel_size_sat, channel_size_sat);
        assert_eq!(cjit_entry.channel_expiry_weeks, channel_expiry_weeks);
        assert_eq!(cjit_entry.node_id, node_id);
        assert_eq!(cjit_entry.state, CJitStateEnum::Created);
        assert!(cjit_entry.fee_sat > 0);
        assert!(cjit_entry.network_fee_sat > 0);
        assert!(cjit_entry.service_fee_sat > 0);

        // Verify the invoice
        assert_eq!(cjit_entry.invoice.state, BtBolt11InvoiceState::Pending);
        assert!(!cjit_entry.invoice.request.is_empty());

        // Verify source was set correctly
        assert_eq!(cjit_entry.source, Some("integration_test".to_string()));

        // Test fetching the created entry
        let fetched_entries = db.get_cjit_entries(
            Some(&vec![cjit_entry.id.clone()]),
            None
        ).await.unwrap();

        assert_eq!(fetched_entries.len(), 1, "Should retrieve exactly one entry");
        let fetched_entry = &fetched_entries[0];

        // Verify fetched entry matches created entry
        assert_eq!(fetched_entry.id, cjit_entry.id);
        assert_eq!(fetched_entry.channel_size_sat, channel_size_sat);
        assert_eq!(fetched_entry.channel_expiry_weeks, channel_expiry_weeks);
        assert_eq!(fetched_entry.node_id, node_id);
        assert_eq!(fetched_entry.state, CJitStateEnum::Created);

        // Test filtering by state
        let created_entries = db.get_cjit_entries(None, Some(CJitStateEnum::Created)).await.unwrap();
        assert!(created_entries.iter().any(|e| e.id == cjit_entry.id),
                "Should find the entry when filtering by Created state");

        // Verify no entries in other states
        let completed_entries = db.get_cjit_entries(None, Some(CJitStateEnum::Completed)).await.unwrap();
        assert!(!completed_entries.iter().any(|e| e.id == cjit_entry.id),
                "Should not find the entry when filtering by Completed state");

        println!("CJIT entry integration test completed successfully");
    }

    #[tokio::test]
    async fn test_cjit_entry_invalid_params() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Test with invalid channel size (too small)
        let result = db.create_cjit_entry(
            1000, // Very small channel size
            100,
            "Test invalid channel size",
            "03c8533232c155c41c42e5a8f8487b192dd36f1d354b86ef461cc82e67e3388839",
            4,
            None
        ).await;
        assert!(result.is_err(), "Should fail with too small channel size");

        // Test with invalid node ID
        let result = db.create_cjit_entry(
            100000,
            1000,
            "Test invalid node ID",
            "invalid_node_id",
            4,
            None
        ).await;
        assert!(result.is_err(), "Should fail with invalid node ID");

        // Test with invalid expiry weeks (too long)
        let result = db.create_cjit_entry(
            100000,
            1000,
            "Test invalid expiry",
            "03c8533232c155c41c42e5a8f8487b192dd36f1d354b86ef461cc82e67e3388839",
            100, // Too many weeks
            None
        ).await;
        assert!(result.is_err(), "Should fail with too many expiry weeks");

        println!("CJIT entry invalid parameters test completed successfully");
    }

    #[tokio::test]
    async fn test_get_active_cjit_entries() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        // Get current timestamp and future timestamp
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        // Create test entries with different states
        let test_entry1 = ICJitEntry {
            id: "test_cjit_1".to_string(),
            state: CJitStateEnum::Created,  // This should be included in active entries
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            channel_size_sat: 100000,
            channel_expiry_weeks: 4,
            channel_open_error: None,
            node_id: "node123".to_string(),
            coupon_code: "TEST1".to_string(),
            source: Some("test".to_string()),
            expires_at: future.to_rfc3339(),
            invoice: IBtBolt11Invoice {
                request: "lnbc...".to_string(),
                state: BtBolt11InvoiceState::Pending,
                expires_at: future.to_rfc3339(),
                updated_at: now.to_rfc3339(),
            },
            channel: None,
            lsp_node: ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            },
            discount: None,
            updated_at: now.to_rfc3339(),
            created_at: now.to_rfc3339(),
        };

        let mut test_entry2 = test_entry1.clone();
        test_entry2.id = "test_cjit_2".to_string();
        test_entry2.state = CJitStateEnum::Failed;  // This should be included in active entries
        test_entry2.fee_sat = 2000;
        test_entry2.coupon_code = "TEST2".to_string();
        test_entry2.channel_open_error = Some("Connection failed".to_string());

        let mut test_entry3 = test_entry1.clone();
        test_entry3.id = "test_cjit_3".to_string();
        test_entry3.state = CJitStateEnum::Completed;  // This should NOT be included
        test_entry3.fee_sat = 3000;
        test_entry3.coupon_code = "TEST3".to_string();

        let mut test_entry4 = test_entry1.clone();
        test_entry4.id = "test_cjit_4".to_string();
        test_entry4.state = CJitStateEnum::Expired;  // This should NOT be included
        test_entry4.fee_sat = 4000;
        test_entry4.coupon_code = "TEST4".to_string();

        // Insert all test entries
        db.upsert_cjit_entry(&test_entry1).await.unwrap();
        db.upsert_cjit_entry(&test_entry2).await.unwrap();
        db.upsert_cjit_entry(&test_entry3).await.unwrap();
        db.upsert_cjit_entry(&test_entry4).await.unwrap();

        // Test get_active_cjit_entries
        let active_entries = db.get_active_cjit_entries().await.unwrap();

        // Verify we only got entries in Created or Failed state
        assert_eq!(active_entries.len(), 2, "Should only retrieve entries in Created or Failed state");

        // Verify the specific entries we got back
        let entry_ids: Vec<String> = active_entries.iter()
            .map(|e| e.id.clone())
            .collect();

        assert!(entry_ids.contains(&test_entry1.id), "Should contain the Created entry");
        assert!(entry_ids.contains(&test_entry2.id), "Should contain the Failed entry");

        // Verify the entries are in the correct state
        for entry in active_entries {
            assert!(
                matches!(entry.state, CJitStateEnum::Created | CJitStateEnum::Failed),
                "Entry {} should be in Created or Failed state, but was in {:?} state",
                entry.id,
                entry.state
            );

            // Verify other fields are correctly loaded
            if entry.id == test_entry1.id {
                assert_eq!(entry.state, CJitStateEnum::Created);
                assert_eq!(entry.fee_sat, 1000);
                assert!(entry.channel_open_error.is_none());
            } else if entry.id == test_entry2.id {
                assert_eq!(entry.state, CJitStateEnum::Failed);
                assert_eq!(entry.fee_sat, 2000);
                assert_eq!(entry.channel_open_error, Some("Connection failed".to_string()));
            }

            // Verify LSP node data is loaded correctly
            assert_eq!(entry.lsp_node.alias, "test_node");
            assert_eq!(entry.lsp_node.pubkey, "test_pubkey");
            assert_eq!(entry.lsp_node.connection_strings.len(), 1);
            assert_eq!(entry.lsp_node.connection_strings[0], "test_connection");
        }

        // Test with no active entries
        // First, mark all entries as completed to make them inactive
        let conn = db.conn.lock().await;
        conn.execute(
            "UPDATE cjit_entries SET state = 'Completed'",
            [],
        ).unwrap();
        drop(conn);  // Release the lock

        // Now test get_active_cjit_entries with no active entries
        let empty_result = db.get_active_cjit_entries().await.unwrap();
        assert_eq!(empty_result.len(), 0, "Should return empty vec when no active entries exist");
    }

    #[tokio::test]
    async fn test_refresh_active_cjit_entries() {
        // Initialize database with staging server
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();

        let options = Some(CreateCjitOptions {
            source: Some("integration_test".to_string()),
            discount_code: None,
        });

        // Create two test entries that will start in Created state
        println!("Creating first test CJIT entry...");
        let entry1 = db.create_cjit_entry(
            100000,         // channel_size_sat
            20000,          // invoice_sat
            "Test CJIT 1",  // description
            "03c8533232c155c41c42e5a8f8487b192dd36f1d354b86ef461cc82e67e3388839", // node_id
            4,              // channel_expiry_weeks
            options.clone()
        ).await.expect("Failed to create first CJIT entry");
        println!("First CJIT entry created with ID: {}", entry1.id);

        // Add a small delay between entry creations
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        println!("Creating second test CJIT entry...");
        let entry2 = db.create_cjit_entry(
            150000,         // channel_size_sat
            30000,          // invoice_sat
            "Test CJIT 2",  // description
            "03c8533232c155c41c42e5a8f8487b192dd36f1d354b86ef461cc82e67e3388839", // node_id
            4,              // channel_expiry_weeks
            options.clone()
        ).await.expect("Failed to create second CJIT entry");
        println!("Second CJIT entry created with ID: {}", entry2.id);

        // Test refreshing active CJIT entries
        println!("Testing refresh_active_cjit_entries...");
        let result = db.refresh_active_cjit_entries().await;

        match result {
            Ok(refreshed_entries) => {
                // Verify we got entries back
                assert!(!refreshed_entries.is_empty(), "Should have received refreshed entries");

                // All refreshed entries should be in an active state
                for entry in &refreshed_entries {
                    println!("Checking refreshed entry: {}", entry.id);
                    assert!(
                        matches!(entry.state, CJitStateEnum::Created | CJitStateEnum::Failed),
                        "Entry {} should be in Created or Failed state, but was in {:?} state",
                        entry.id,
                        entry.state
                    );

                    // Verify the basic entry properties are preserved
                    if entry.id == entry1.id {
                        assert_eq!(entry.channel_size_sat, 100000);
                    } else if entry.id == entry2.id {
                        assert_eq!(entry.channel_size_sat, 150000);
                    }

                    // Verify LSP node data is loaded correctly
                    assert!(!entry.lsp_node.pubkey.is_empty());
                    assert!(!entry.lsp_node.connection_strings.is_empty());
                }

                // Verify database state
                let conn = db.conn.lock().await;
                for entry in &refreshed_entries {
                    let row = conn.query_row(
                        "SELECT id FROM cjit_entries WHERE id = ?1",
                        [&entry.id],
                        |row| Ok(row.get::<_, String>(0)?)
                    ).unwrap();

                    assert_eq!(row, entry.id);
                }
            },
            Err(e) => panic!("Failed to refresh active CJIT entries: {:?}", e),
        }

        // Test with no active entries by manually marking all entries as completed in the database
        {
            let conn = db.conn.lock().await;
            conn.execute(
                "UPDATE cjit_entries SET state = 'Completed'",
                [],
            ).unwrap();
        }

        // Now test refresh_active_cjit_entries with no active entries
        let empty_result = db.refresh_active_cjit_entries().await.unwrap();
        assert_eq!(empty_result.len(), 0, "Should return empty vec when no active entries exist");
    }

    #[tokio::test]
    async fn test_regtest_mine() {
        let db = BlocktankDB::new(":memory:", Some(STAGING_SERVER)).await.unwrap();
        // Test mining 1 block
        let result = db.regtest_mine(Some(1)).await;
        assert!(result.is_ok(), "Failed to mine 1 block: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_regtest_deposit() {
        let client = BlocktankClient::new(Some(STAGING_SERVER))
            .expect("Failed to create BlocktankClient");

        let test_address = "bcrt1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";

        // Test deposit with specific amount
        let result = client.regtest_deposit(test_address, Some(50000)).await;

        match result {
            Ok(txid) => {
                println!("Successfully deposited to address, txid: {}", txid);
                assert!(!txid.is_empty(), "Transaction ID should not be empty");
            },
            Err(err) => {
                if err.to_string().contains("not in regtest mode") ||
                    err.to_string().contains("Bad Request") {
                    println!("Skipping test_regtest_deposit: Not in regtest mode or not supported in this environment");
                    return;
                } else {
                    panic!("API call to regtest_deposit failed with unexpected error: {:?}", err);
                }
            }
        }
    }
}