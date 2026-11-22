#[cfg(test)]
mod tests {
    #![allow(clippy::print_stdout)]

    use crate::modules::blocktank::liquidity::{
        calculate_channel_liquidity_options, get_default_lsp_balance, ChannelLiquidityParams,
        DefaultLspBalanceParams,
    };
    use crate::modules::blocktank::{BlocktankDB, BlocktankError};
    use rust_blocktank_client::*;

    #[tokio::test]
    async fn test_upsert_info() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

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
            let row = conn
                .query_row(
                    "SELECT version, is_current FROM info WHERE version = 1",
                    [],
                    |row| Ok((row.get::<_, u32>(0)?, row.get::<_, bool>(1)?)),
                )
                .unwrap();
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
            let rows: Vec<(u32, bool)> = conn
                .prepare("SELECT version, is_current FROM info ORDER BY version")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();

            assert_eq!(rows.len(), 2, "Should have two versions");
            assert_eq!(rows[0], (1, false), "Version 1 should not be current");
            assert_eq!(rows[1], (2, true), "Version 2 should be current");

            // Verify JSON serialization
            let node_data: String = conn
                .query_row("SELECT nodes FROM info WHERE version = 2", [], |row| {
                    row.get(0)
                })
                .unwrap();

            let nodes: Vec<ILspNode> = serde_json::from_str(&node_data).unwrap();
            assert_eq!(nodes[0].alias, "updated_node");
        } // Lock is dropped here
    }

    #[tokio::test]
    async fn test_fetch_and_store_info_error_handling() {
        // Initialize with invalid URL to test error handling
        let db = BlocktankDB::new(":memory:", Some("http://invalid-url"))
            .await
            .unwrap();

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
            let count: u32 = conn
                .query_row("SELECT COUNT(*) FROM info", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0, "No data should be stored when fetch fails");
        }
    }

    #[tokio::test]
    async fn test_get_info() {
        // Initialize in-memory database
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

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
        assert_eq!(
            stored_info.options.min_channel_size_sat,
            test_info.options.min_channel_size_sat
        );
        assert_eq!(stored_info.versions.http, test_info.versions.http);
        assert_eq!(stored_info.onchain.network, test_info.onchain.network);
        assert_eq!(
            stored_info.onchain.fee_rates.fast,
            test_info.onchain.fee_rates.fast
        );
    }

    #[tokio::test]
    async fn test_upsert_order() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Get current timestamp in ISO 8601 format
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        let test_order = IBtOrder {
            id: "test_order_1".to_string(),
            state: BtOrderState::Created,
            state2: Some(BtOrderState2::Created),
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            lsp_balance_sat: 10000,
            client_balance_sat: 5000,
            zero_conf: false,
            zero_reserve: false,
            client_node_id: Some("client123".to_string()),
            channel_expiry_weeks: 2,
            channel_expires_at: future.to_rfc3339(), // Changed from integer to ISO string
            order_expires_at: future.to_rfc3339(),   // Changed from integer to ISO string
            channel: None,
            lsp_node: Some(ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }),
            lnurl: None,
            payment: Some(IBtPayment {
                state: BtPaymentState::Created,
                state2: Some(BtPaymentState2::Created),
                paid_sat: 0,
                bolt11_invoice: Some(IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future.to_rfc3339(), // Changed from integer to ISO string
                    updated_at: now.to_rfc3339(),    // Changed from integer to ISO string
                }),
                onchain: Some(IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                }),
                is_manually_paid: None,
                manual_refunds: None,
            }),
            coupon_code: None,
            source: None,
            discount: None,
            updated_at: now.to_rfc3339(), // Changed from integer to ISO string
            created_at: now.to_rfc3339(), // Changed from integer to ISO string
        };

        // Test initial insert
        let result = db.upsert_order(&test_order).await;
        assert!(result.is_ok(), "Failed to insert order: {:?}", result.err());

        // Verify the insert
        {
            let conn = db.conn.lock().await;
            let row = conn
                .query_row(
                    "SELECT id, state, fee_sat FROM orders WHERE id = ?1",
                    [&test_order.id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u64>(2)?,
                        ))
                    },
                )
                .unwrap();

            assert_eq!(row.0, test_order.id);
            assert_eq!(row.1, format!("{:?}", test_order.state));
            assert_eq!(row.2, test_order.fee_sat);

            // Verify JSON serialization
            let lsp_node_json: String = conn
                .query_row(
                    "SELECT lsp_node_data FROM orders WHERE id = ?1",
                    [&test_order.id],
                    |row| row.get(0),
                )
                .unwrap();

            let stored_lsp_node: ILspNode = serde_json::from_str(&lsp_node_json).unwrap();
            assert_eq!(
                stored_lsp_node.alias,
                test_order.lsp_node.as_ref().unwrap().alias.clone()
            );
            assert_eq!(
                stored_lsp_node.pubkey,
                test_order.lsp_node.as_ref().unwrap().pubkey.clone()
            );
        }

        // Test update
        let mut updated_order = test_order.clone();
        updated_order.fee_sat = 2000;
        updated_order.state = BtOrderState::Open;
        updated_order.updated_at = chrono::Utc::now().to_rfc3339(); // Update timestamp

        let result = db.upsert_order(&updated_order).await;
        assert!(result.is_ok(), "Failed to update order: {:?}", result.err());

        // Verify the update
        {
            let conn = db.conn.lock().await;
            let row = conn
                .query_row(
                    "SELECT state, fee_sat FROM orders WHERE id = ?1",
                    [&updated_order.id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
                )
                .unwrap();

            assert_eq!(row.0, format!("{:?}", updated_order.state));
            assert_eq!(row.1, updated_order.fee_sat);
        }
    }

    #[tokio::test]
    async fn test_upsert_orders() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Create multiple test orders
        let mut orders = Vec::new();
        for i in 1..=5 {
            let mut order = create_test_order(&format!("bulk_order_{}", i));
            order.fee_sat = 1000 * i as u64;
            order.lsp_balance_sat = 10000 * i as u64;
            orders.push(order);
        }

        // Test bulk insert
        let result = db.upsert_orders(&orders).await;
        assert!(
            result.is_ok(),
            "Failed to bulk insert orders: {:?}",
            result.err()
        );

        // Verify all orders were inserted
        let stored_orders = db.get_orders(None, None).await.unwrap();
        assert_eq!(stored_orders.len(), 5, "Should have inserted 5 orders");

        // Verify each order's data is correct
        for i in 1..=5 {
            let order_id = format!("bulk_order_{}", i);
            let stored_order = stored_orders
                .iter()
                .find(|o| o.id == order_id)
                .expect(&format!("Order {} not found", order_id));

            assert_eq!(stored_order.fee_sat, 1000 * i as u64);
            assert_eq!(stored_order.lsp_balance_sat, 10000 * i as u64);
            assert_eq!(stored_order.state, BtOrderState::Created);

            // Verify JSON fields are stored correctly
            assert!(stored_order.lsp_node.is_some());
            assert_eq!(stored_order.lsp_node.as_ref().unwrap().alias, "test_node");
            assert!(stored_order.payment.is_some());
        }

        // Test bulk update - modify some orders
        let mut updated_orders = orders.clone();
        updated_orders[0].fee_sat = 9999; // Update first order
        updated_orders[1].state = BtOrderState::Open; // Update second order
        updated_orders[1].state2 = Some(BtOrderState2::Executed);
        updated_orders[2].lsp_balance_sat = 99999; // Update third order
        updated_orders[2].updated_at = chrono::Utc::now().to_rfc3339();

        // Bulk update
        let result = db.upsert_orders(&updated_orders).await;
        assert!(
            result.is_ok(),
            "Failed to bulk update orders: {:?}",
            result.err()
        );

        // Verify updates were applied
        let stored_orders_after = db.get_orders(None, None).await.unwrap();
        assert_eq!(stored_orders_after.len(), 5, "Should still have 5 orders");

        let updated_order_1 = stored_orders_after
            .iter()
            .find(|o| o.id == "bulk_order_1")
            .expect("Order 1 not found");
        assert_eq!(updated_order_1.fee_sat, 9999);

        let updated_order_2 = stored_orders_after
            .iter()
            .find(|o| o.id == "bulk_order_2")
            .expect("Order 2 not found");
        assert_eq!(updated_order_2.state, BtOrderState::Open);
        assert_eq!(updated_order_2.state2, Some(BtOrderState2::Executed));

        let updated_order_3 = stored_orders_after
            .iter()
            .find(|o| o.id == "bulk_order_3")
            .expect("Order 3 not found");
        assert_eq!(updated_order_3.lsp_balance_sat, 99999);

        // Verify other orders weren't affected
        let updated_order_4 = stored_orders_after
            .iter()
            .find(|o| o.id == "bulk_order_4")
            .expect("Order 4 not found");
        assert_eq!(updated_order_4.fee_sat, 4000);
        assert_eq!(updated_order_4.lsp_balance_sat, 40000);
    }

    #[tokio::test]
    async fn test_upsert_orders_empty() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Test with empty vector
        let result = db.upsert_orders(&[]).await;
        assert!(result.is_ok(), "Should handle empty vector gracefully");

        // Verify no orders were inserted
        let stored_orders = db.get_orders(None, None).await.unwrap();
        assert_eq!(stored_orders.len(), 0);
    }

    #[tokio::test]
    async fn test_upsert_orders_large_batch() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Create a larger batch of orders to test performance
        let mut orders = Vec::new();
        for i in 1..=50 {
            let mut order = create_test_order(&format!("large_batch_order_{}", i));
            order.fee_sat = 500 * i as u64;
            orders.push(order);
        }

        // Bulk insert
        let start = std::time::Instant::now();
        let result = db.upsert_orders(&orders).await;
        let bulk_duration = start.elapsed();
        assert!(
            result.is_ok(),
            "Failed to bulk insert orders: {:?}",
            result.err()
        );

        // Verify all orders were inserted
        let stored_orders = db.get_orders(None, None).await.unwrap();
        assert_eq!(stored_orders.len(), 50, "Should have inserted 50 orders");

        // Verify a sample of orders
        for i in (1..=50).step_by(10) {
            let order_id = format!("large_batch_order_{}", i);
            let stored_order = stored_orders
                .iter()
                .find(|o| o.id == order_id)
                .expect(&format!("Order {} not found", order_id));
            assert_eq!(stored_order.fee_sat, 500 * i as u64);
        }

        println!("Bulk insert of 50 orders took: {:?}", bulk_duration);
    }

    #[tokio::test]
    async fn test_get_orders() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Get current timestamp and future timestamp as integers
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);
        let now_unix = now.timestamp();
        let future_unix = future.timestamp();

        // Create multiple test orders with different states
        let test_order1 = IBtOrder {
            id: "test_order_1".to_string(),
            state: BtOrderState::Created,
            state2: Some(BtOrderState2::Created),
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
            lsp_node: Some(ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }),
            lnurl: None,
            payment: Some(IBtPayment {
                state: BtPaymentState::Created,
                state2: Some(BtPaymentState2::Created),
                paid_sat: 0,
                bolt11_invoice: Some(IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future_unix.to_string(),
                    updated_at: now_unix.to_string(),
                }),
                onchain: Some(IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                }),
                is_manually_paid: None,
                manual_refunds: None,
            }),
            coupon_code: None,
            source: None,
            discount: None,
            updated_at: now_unix.to_string(),
            created_at: now_unix.to_string(),
        };

        let mut test_order2 = test_order1.clone();
        test_order2.id = "test_order_2".to_string();
        test_order2.state = BtOrderState::Open;
        test_order2.state2 = Some(BtOrderState2::Executed);
        test_order2.fee_sat = 2000;

        let mut test_order3 = test_order1.clone();
        test_order3.id = "test_order_3".to_string();
        test_order3.state = BtOrderState::Closed;
        test_order3.state2 = Some(BtOrderState2::Paid);
        test_order3.fee_sat = 3000;

        // Insert test orders
        db.upsert_order(&test_order1).await.unwrap();
        db.upsert_order(&test_order2).await.unwrap();
        db.upsert_order(&test_order3).await.unwrap();

        // Test 1: Get all orders
        let all_orders = db.get_orders(None, None).await.unwrap();
        assert_eq!(all_orders.len(), 3, "Should retrieve all 3 orders");

        // Test 2: Get specific orders by ID
        let specific_orders = db
            .get_orders(
                Some(&vec![test_order1.id.clone(), test_order2.id.clone()]),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            specific_orders.len(),
            2,
            "Should retrieve 2 specific orders"
        );
        assert!(specific_orders.iter().any(|o| o.id == test_order1.id));
        assert!(specific_orders.iter().any(|o| o.id == test_order2.id));

        // Test 3: Filter by state
        let paid_orders = db
            .get_orders(None, Some(BtOrderState2::Paid))
            .await
            .unwrap();
        assert_eq!(paid_orders.len(), 1, "Should retrieve 1 paid order");
        assert_eq!(paid_orders[0].id, test_order3.id);

        // Test 4: Verify complex fields deserialization
        let order = &all_orders[0];
        assert!(!order
            .lsp_node
            .as_ref()
            .unwrap()
            .connection_strings
            .is_empty());
        assert_eq!(
            order.payment.as_ref().unwrap().state,
            test_order1.payment.as_ref().unwrap().state
        );
        assert_eq!(
            order
                .payment
                .as_ref()
                .unwrap()
                .bolt11_invoice
                .as_ref()
                .unwrap()
                .state,
            test_order1
                .payment
                .as_ref()
                .unwrap()
                .bolt11_invoice
                .as_ref()
                .unwrap()
                .state
        );

        // Test 5: Test with non-existent order IDs
        let non_existent = db
            .get_orders(Some(&vec!["non_existent_id".to_string()]), None)
            .await
            .unwrap();
        assert_eq!(
            non_existent.len(),
            0,
            "Should return empty vector for non-existent IDs"
        );

        // Test 6: Test with invalid state filter
        let executed_orders = db
            .get_orders(None, Some(BtOrderState2::Executed))
            .await
            .unwrap();
        assert_eq!(executed_orders.len(), 1, "Should retrieve 1 executed order");
        assert_eq!(executed_orders[0].id, test_order2.id);
    }

    #[tokio::test]
    async fn test_get_active_orders() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Get current timestamp and future timestamp
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        // Create test orders with different states
        let test_order1 = IBtOrder {
            id: "test_order_1".to_string(),
            state: BtOrderState::Created,
            state2: Some(BtOrderState2::Created), // This should be included in active orders
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
            lsp_node: Some(ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }),
            lnurl: None,
            payment: Some(IBtPayment {
                state: BtPaymentState::Created,
                state2: Some(BtPaymentState2::Created),
                paid_sat: 0,
                bolt11_invoice: Some(IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future.to_rfc3339(),
                    updated_at: now.to_rfc3339(),
                }),
                onchain: Some(IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                }),
                is_manually_paid: None,
                manual_refunds: None,
            }),
            coupon_code: None,
            source: None,
            discount: None,
            updated_at: now.to_rfc3339(),
            created_at: now.to_rfc3339(),
        };

        let mut test_order2 = test_order1.clone();
        test_order2.id = "test_order_2".to_string();
        test_order2.state = BtOrderState::Open;
        test_order2.state2 = Some(BtOrderState2::Paid); // This should be included in active orders
        test_order2.fee_sat = 2000;

        let mut test_order3 = test_order1.clone();
        test_order3.id = "test_order_3".to_string();
        test_order3.state = BtOrderState::Closed;
        test_order3.state2 = Some(BtOrderState2::Expired); // This should NOT be included
        test_order3.fee_sat = 3000;

        let mut test_order4 = test_order1.clone();
        test_order4.id = "test_order_4".to_string();
        test_order4.state = BtOrderState::Closed;
        test_order4.state2 = Some(BtOrderState2::Executed); // This should NOT be included
        test_order4.fee_sat = 4000;

        // Insert all test orders
        db.upsert_order(&test_order1).await.unwrap();
        db.upsert_order(&test_order2).await.unwrap();
        db.upsert_order(&test_order3).await.unwrap();
        db.upsert_order(&test_order4).await.unwrap();

        // Test get_active_orders
        let active_orders = db.get_active_orders().await.unwrap();

        // Verify we only got orders in Created or Paid state
        assert_eq!(
            active_orders.len(),
            2,
            "Should only retrieve orders in Created or Paid state"
        );

        // Verify the specific orders we got back
        let order_ids: Vec<String> = active_orders.iter().map(|o| o.id.clone()).collect();

        assert!(
            order_ids.contains(&test_order1.id),
            "Should contain the Created order"
        );
        assert!(
            order_ids.contains(&test_order2.id),
            "Should contain the Paid order"
        );

        // Verify the orders are in the correct state
        for order in active_orders {
            assert!(
                matches!(
                    order.state2,
                    Some(BtOrderState2::Created) | Some(BtOrderState2::Paid)
                ),
                "Order {} should be in Created or Paid state, but was in {:?} state",
                order.id,
                order.state2
            );

            // Verify other fields are correctly loaded
            if order.id == test_order1.id {
                assert_eq!(order.state2, Some(BtOrderState2::Created));
                assert_eq!(order.fee_sat, 1000);
            } else if order.id == test_order2.id {
                assert_eq!(order.state2, Some(BtOrderState2::Paid));
                assert_eq!(order.fee_sat, 2000);
            }
        }
    }

    #[tokio::test]
    async fn test_upsert_cjit_entry() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

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
        assert!(
            result.is_ok(),
            "Failed to insert CJIT entry: {:?}",
            result.err()
        );

        // Verify the insert
        {
            let conn = db.conn.lock().await;
            let row = conn
                .query_row(
                    "SELECT id, state, fee_sat FROM cjit_entries WHERE id = ?1",
                    [&test_entry.id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u64>(2)?,
                        ))
                    },
                )
                .unwrap();

            assert_eq!(row.0, test_entry.id);
            assert_eq!(row.1, format!("{:?}", test_entry.state));
            assert_eq!(row.2, test_entry.fee_sat);

            // Verify JSON serialization
            let lsp_node_json: String = conn
                .query_row(
                    "SELECT lsp_node_data FROM cjit_entries WHERE id = ?1",
                    [&test_entry.id],
                    |row| row.get(0),
                )
                .unwrap();

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
        assert!(
            result.is_ok(),
            "Failed to update CJIT entry: {:?}",
            result.err()
        );

        // Verify the update
        {
            let conn = db.conn.lock().await;
            let row = conn
                .query_row(
                    "SELECT state, fee_sat FROM cjit_entries WHERE id = ?1",
                    [&updated_entry.id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
                )
                .unwrap();

            assert_eq!(row.0, format!("{:?}", updated_entry.state));
            assert_eq!(row.1, updated_entry.fee_sat);
        }
    }

    #[tokio::test]
    async fn test_upsert_cjit_entries() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Create multiple test CJIT entries
        let mut entries = Vec::new();
        for i in 1..=5 {
            let mut entry = create_test_cjit_entry(&format!("bulk_cjit_{}", i));
            entry.fee_sat = 1000 * i as u64;
            entry.channel_size_sat = 10000 * i as u64;
            entries.push(entry);
        }

        // Test bulk insert
        let result = db.upsert_cjit_entries(&entries).await;
        assert!(
            result.is_ok(),
            "Failed to bulk insert CJIT entries: {:?}",
            result.err()
        );

        // Verify all entries were inserted
        let stored_entries = db.get_cjit_entries(None, None).await.unwrap();
        assert_eq!(stored_entries.len(), 5, "Should have inserted 5 entries");

        // Verify each entry's data is correct
        for i in 1..=5 {
            let entry_id = format!("bulk_cjit_{}", i);
            let stored_entry = stored_entries
                .iter()
                .find(|e| e.id == entry_id)
                .expect(&format!("Entry {} not found", entry_id));

            assert_eq!(stored_entry.fee_sat, 1000 * i as u64);
            assert_eq!(stored_entry.channel_size_sat, 10000 * i as u64);
            assert_eq!(stored_entry.state, CJitStateEnum::Created);

            // Verify JSON fields are stored correctly
            assert_eq!(stored_entry.lsp_node.alias, "test_node");
            assert_eq!(stored_entry.invoice.state, BtBolt11InvoiceState::Pending);
        }

        // Test bulk update - modify some entries
        let mut updated_entries = entries.clone();
        updated_entries[0].fee_sat = 9999; // Update first entry
        updated_entries[1].state = CJitStateEnum::Completed; // Update second entry
        updated_entries[2].channel_size_sat = 99999; // Update third entry
        updated_entries[2].updated_at = chrono::Utc::now().to_rfc3339();

        // Bulk update
        let result = db.upsert_cjit_entries(&updated_entries).await;
        assert!(
            result.is_ok(),
            "Failed to bulk update CJIT entries: {:?}",
            result.err()
        );

        // Verify updates were applied
        let stored_entries_after = db.get_cjit_entries(None, None).await.unwrap();
        assert_eq!(stored_entries_after.len(), 5, "Should still have 5 entries");

        let updated_entry_1 = stored_entries_after
            .iter()
            .find(|e| e.id == "bulk_cjit_1")
            .expect("Entry 1 not found");
        assert_eq!(updated_entry_1.fee_sat, 9999);

        let updated_entry_2 = stored_entries_after
            .iter()
            .find(|e| e.id == "bulk_cjit_2")
            .expect("Entry 2 not found");
        assert_eq!(updated_entry_2.state, CJitStateEnum::Completed);

        let updated_entry_3 = stored_entries_after
            .iter()
            .find(|e| e.id == "bulk_cjit_3")
            .expect("Entry 3 not found");
        assert_eq!(updated_entry_3.channel_size_sat, 99999);

        // Verify other entries weren't affected
        let updated_entry_4 = stored_entries_after
            .iter()
            .find(|e| e.id == "bulk_cjit_4")
            .expect("Entry 4 not found");
        assert_eq!(updated_entry_4.fee_sat, 4000);
        assert_eq!(updated_entry_4.channel_size_sat, 40000);
    }

    #[tokio::test]
    async fn test_upsert_cjit_entries_empty() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Test with empty vector
        let result = db.upsert_cjit_entries(&[]).await;
        assert!(result.is_ok(), "Should handle empty vector gracefully");

        // Verify no entries were inserted
        let stored_entries = db.get_cjit_entries(None, None).await.unwrap();
        assert_eq!(stored_entries.len(), 0);
    }

    #[tokio::test]
    async fn test_get_cjit_entries() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

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
        let specific_entries = db
            .get_cjit_entries(
                Some(&vec![test_entry1.id.clone(), test_entry2.id.clone()]),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            specific_entries.len(),
            2,
            "Should retrieve 2 specific entries"
        );
        assert!(specific_entries.iter().any(|e| e.id == test_entry1.id));
        assert!(specific_entries.iter().any(|e| e.id == test_entry2.id));

        // Test 3: Filter by state
        let created_entries = db
            .get_cjit_entries(None, Some(CJitStateEnum::Created))
            .await
            .unwrap();
        assert_eq!(created_entries.len(), 1, "Should retrieve 1 created entry");
        assert_eq!(created_entries[0].id, test_entry1.id);

        // Test 4: Verify complex fields deserialization
        let entry = &all_entries[0];
        assert!(!entry.lsp_node.connection_strings.is_empty());
        assert_eq!(entry.invoice.state, test_entry1.invoice.state);

        // Test 5: Test with non-existent entry IDs
        let non_existent = db
            .get_cjit_entries(Some(&vec!["non_existent_id".to_string()]), None)
            .await
            .unwrap();
        assert_eq!(
            non_existent.len(),
            0,
            "Should return empty vector for non-existent IDs"
        );

        // Test 6: Test with completed state filter
        let completed_entries = db
            .get_cjit_entries(None, Some(CJitStateEnum::Completed))
            .await
            .unwrap();
        assert_eq!(
            completed_entries.len(),
            1,
            "Should retrieve 1 completed entry"
        );
        assert_eq!(completed_entries[0].id, test_entry2.id);

        // Test 7: Verify all fields are correctly loaded for a specific entry
        let specific_entry = &created_entries[0];
        assert_eq!(specific_entry.fee_sat, test_entry1.fee_sat);
        assert_eq!(specific_entry.network_fee_sat, test_entry1.network_fee_sat);
        assert_eq!(specific_entry.service_fee_sat, test_entry1.service_fee_sat);
        assert_eq!(
            specific_entry.channel_size_sat,
            test_entry1.channel_size_sat
        );
        assert_eq!(
            specific_entry.channel_expiry_weeks,
            test_entry1.channel_expiry_weeks
        );
        assert_eq!(specific_entry.node_id, test_entry1.node_id);
        assert_eq!(specific_entry.coupon_code, test_entry1.coupon_code);
        assert_eq!(specific_entry.source, test_entry1.source);
    }

    #[tokio::test]
    async fn test_get_active_cjit_entries() {
        let db = BlocktankDB::new(":memory:", None).await.unwrap();

        // Get current timestamp and future timestamp
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(30);

        // Create test entries with different states
        let test_entry1 = ICJitEntry {
            id: "test_cjit_1".to_string(),
            state: CJitStateEnum::Created, // This should be included in active entries
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
        test_entry2.state = CJitStateEnum::Failed; // This should be included in active entries
        test_entry2.fee_sat = 2000;
        test_entry2.coupon_code = "TEST2".to_string();
        test_entry2.channel_open_error = Some("Connection failed".to_string());

        let mut test_entry3 = test_entry1.clone();
        test_entry3.id = "test_cjit_3".to_string();
        test_entry3.state = CJitStateEnum::Completed; // This should NOT be included
        test_entry3.fee_sat = 3000;
        test_entry3.coupon_code = "TEST3".to_string();

        let mut test_entry4 = test_entry1.clone();
        test_entry4.id = "test_cjit_4".to_string();
        test_entry4.state = CJitStateEnum::Expired; // This should NOT be included
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
        assert_eq!(
            active_entries.len(),
            2,
            "Should only retrieve entries in Created or Failed state"
        );

        // Verify the specific entries we got back
        let entry_ids: Vec<String> = active_entries.iter().map(|e| e.id.clone()).collect();

        assert!(
            entry_ids.contains(&test_entry1.id),
            "Should contain the Created entry"
        );
        assert!(
            entry_ids.contains(&test_entry2.id),
            "Should contain the Failed entry"
        );

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
                assert_eq!(
                    entry.channel_open_error,
                    Some("Connection failed".to_string())
                );
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
        conn.execute("UPDATE cjit_entries SET state = 'Completed'", [])
            .unwrap();
        drop(conn); // Release the lock

        // Now test get_active_cjit_entries with no active entries
        let empty_result = db.get_active_cjit_entries().await.unwrap();
        assert_eq!(
            empty_result.len(),
            0,
            "Should return empty vec when no active entries exist"
        );
    }

    #[tokio::test]
    async fn test_remove_all_orders() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let db_path = format!("{}/test_blocktank.db", temp_dir.path().display());

        let db = BlocktankDB::new(&db_path, None)
            .await
            .expect("Failed to create BlocktankDB");

        // Create and insert test orders
        let order1 = create_test_order("order_1");
        let order2 = create_test_order("order_2");
        let order3 = create_test_order("order_3");

        db.upsert_order(&order1)
            .await
            .expect("Failed to insert order1");
        db.upsert_order(&order2)
            .await
            .expect("Failed to insert order2");
        db.upsert_order(&order3)
            .await
            .expect("Failed to insert order3");

        // Verify orders exist
        let orders = db
            .get_orders(None, None)
            .await
            .expect("Failed to get orders");
        assert_eq!(orders.len(), 3);

        // Remove all orders
        db.remove_all_orders()
            .await
            .expect("Failed to remove all orders");

        // Verify all orders are deleted
        let orders_after = db
            .get_orders(None, None)
            .await
            .expect("Failed to get orders");
        assert_eq!(orders_after.len(), 0);

        // Verify we can still insert new orders after wipe
        let new_order = create_test_order("new_order");
        db.upsert_order(&new_order)
            .await
            .expect("Failed to insert new order");
        let orders_new = db
            .get_orders(None, None)
            .await
            .expect("Failed to get orders");
        assert_eq!(orders_new.len(), 1);
    }

    #[tokio::test]
    async fn test_remove_all_cjit_entries() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let db_path = format!("{}/test_blocktank.db", temp_dir.path().display());

        let db = BlocktankDB::new(&db_path, None)
            .await
            .expect("Failed to create BlocktankDB");

        // Create and insert test CJIT entries
        let entry1 = create_test_cjit_entry("cjit_1");
        let entry2 = create_test_cjit_entry("cjit_2");
        let entry3 = create_test_cjit_entry("cjit_3");

        db.upsert_cjit_entry(&entry1)
            .await
            .expect("Failed to insert entry1");
        db.upsert_cjit_entry(&entry2)
            .await
            .expect("Failed to insert entry2");
        db.upsert_cjit_entry(&entry3)
            .await
            .expect("Failed to insert entry3");

        // Verify entries exist
        let entries = db
            .get_cjit_entries(None, None)
            .await
            .expect("Failed to get entries");
        assert_eq!(entries.len(), 3);

        // Remove all CJIT entries
        db.remove_all_cjit_entries()
            .await
            .expect("Failed to remove all CJIT entries");

        // Verify all entries are deleted
        let entries_after = db
            .get_cjit_entries(None, None)
            .await
            .expect("Failed to get entries");
        assert_eq!(entries_after.len(), 0);

        // Verify we can still insert new entries after wipe
        let new_entry = create_test_cjit_entry("new_cjit");
        db.upsert_cjit_entry(&new_entry)
            .await
            .expect("Failed to insert new entry");
        let entries_new = db
            .get_cjit_entries(None, None)
            .await
            .expect("Failed to get entries");
        assert_eq!(entries_new.len(), 1);
    }

    #[tokio::test]
    async fn test_wipe_all() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let db_path = format!("{}/test_blocktank.db", temp_dir.path().display());

        let db = BlocktankDB::new(&db_path, None)
            .await
            .expect("Failed to create BlocktankDB");

        // Insert various types of data
        let order1 = create_test_order("order_1");
        let order2 = create_test_order("order_2");
        db.upsert_order(&order1)
            .await
            .expect("Failed to insert order");
        db.upsert_order(&order2)
            .await
            .expect("Failed to insert order");

        let entry1 = create_test_cjit_entry("cjit_1");
        let entry2 = create_test_cjit_entry("cjit_2");
        db.upsert_cjit_entry(&entry1)
            .await
            .expect("Failed to insert entry");
        db.upsert_cjit_entry(&entry2)
            .await
            .expect("Failed to insert entry");

        let info = create_test_info();
        db.upsert_info(&info).await.expect("Failed to insert info");

        // Verify all data exists
        let orders = db
            .get_orders(None, None)
            .await
            .expect("Failed to get orders");
        assert_eq!(orders.len(), 2);
        let entries = db
            .get_cjit_entries(None, None)
            .await
            .expect("Failed to get entries");
        assert_eq!(entries.len(), 2);
        let info_check = db.get_info().await.expect("Failed to get info");
        assert!(info_check.is_some());

        // Wipe all data
        db.wipe_all().await.expect("Failed to wipe all");

        // Verify everything is deleted
        let orders_after = db
            .get_orders(None, None)
            .await
            .expect("Failed to get orders");
        assert_eq!(orders_after.len(), 0);
        let entries_after = db
            .get_cjit_entries(None, None)
            .await
            .expect("Failed to get entries");
        assert_eq!(entries_after.len(), 0);
        let info_after = db.get_info().await.expect("Failed to get info");
        assert!(info_after.is_none());

        // Verify we can still insert new data after wipe
        let new_order = create_test_order("new_order");
        db.upsert_order(&new_order)
            .await
            .expect("Failed to insert new order");
        let orders_new = db
            .get_orders(None, None)
            .await
            .expect("Failed to get orders");
        assert_eq!(orders_new.len(), 1);
    }

    // Helper function to create test orders
    fn create_test_order(id: &str) -> IBtOrder {
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(7);

        IBtOrder {
            id: id.to_string(),
            state: BtOrderState::Created,
            state2: Some(BtOrderState2::Created),
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            lsp_balance_sat: 100000,
            client_balance_sat: 50000,
            zero_conf: true,
            zero_reserve: false,
            client_node_id: Some("test_node_id".to_string()),
            channel_expiry_weeks: 4,
            channel_expires_at: future.to_rfc3339(),
            order_expires_at: future.to_rfc3339(),
            channel: None,
            lsp_node: Some(ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }),
            lnurl: None,
            payment: Some(IBtPayment {
                state: BtPaymentState::Created,
                state2: Some(BtPaymentState2::Created),
                paid_sat: 0,
                bolt11_invoice: Some(IBtBolt11Invoice {
                    request: "lnbc...".to_string(),
                    state: BtBolt11InvoiceState::Pending,
                    expires_at: future.to_rfc3339(),
                    updated_at: now.to_rfc3339(),
                }),
                onchain: Some(IBtOnchainTransactions {
                    address: "bc1...".to_string(),
                    confirmed_sat: 0,
                    required_confirmations: 3,
                    transactions: vec![],
                }),
                is_manually_paid: None,
                manual_refunds: None,
            }),
            coupon_code: None,
            source: Some("test".to_string()),
            discount: None,
            updated_at: now.to_rfc3339(),
            created_at: now.to_rfc3339(),
        }
    }

    // Helper function to create test CJIT entries
    fn create_test_cjit_entry(id: &str) -> ICJitEntry {
        let now = chrono::Utc::now();
        let future = now + chrono::Duration::days(1);

        ICJitEntry {
            id: id.to_string(),
            state: CJitStateEnum::Created,
            fee_sat: 1000,
            network_fee_sat: 500,
            service_fee_sat: 500,
            channel_size_sat: 150000,
            channel_expiry_weeks: 4,
            channel_open_error: None,
            node_id: "test_node_id".to_string(),
            coupon_code: "test_coupon".to_string(),
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
        }
    }

    // Helper function to create test info
    fn create_test_info() -> IBtInfo {
        IBtInfo {
            version: 1,
            nodes: vec![ILspNode {
                alias: "test_node".to_string(),
                pubkey: "test_pubkey".to_string(),
                connection_strings: vec!["test_connection".to_string()],
                readonly: None,
            }],
            options: IBtInfoOptions {
                min_channel_size_sat: 10000,
                max_channel_size_sat: 10000000,
                min_expiry_weeks: 1,
                max_expiry_weeks: 52,
                min_payment_confirmations: 1,
                min_high_risk_payment_confirmations: 3,
                max_0_conf_client_balance_sat: 100000,
                max_client_balance_sat: 1000000,
            },
            versions: IBtInfoVersions {
                http: "1.0.0".to_string(),
                btc: "2.0.0".to_string(),
                ln2: "3.0.0".to_string(),
            },
            onchain: IBtInfoOnchain {
                network: BitcoinNetworkEnum::Mainnet,
                fee_rates: FeeRates {
                    fast: 10,
                    mid: 5,
                    slow: 2,
                },
            },
        }
    }

    // Test rate: 1000 sats per EUR (for easy math)
    const TEST_SATS_PER_EUR: u64 = 1000;

    #[test]
    fn test_default_lsp_balance_small_client_balance() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 100_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 1_020_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // 0-225€: fill to 450€, so LSP = 450k - 100k = 350k
        assert_eq!(options.default_lsp_balance_sat, 350_000);
    }

    #[test]
    fn test_default_lsp_balance_medium_client_balance() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 300_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 1_020_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // 225-495€: 1:1 ratio
        assert_eq!(options.default_lsp_balance_sat, 300_000);
    }

    #[test]
    fn test_default_lsp_balance_large_client_balance() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 600_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // 495€+: max out, max_channel = 970k * 0.98 = 950.6k, max_lsp = 950.6k - 600k = 350.6k
        assert_eq!(options.default_lsp_balance_sat, 350_600);
    }

    #[test]
    fn test_min_lsp_balance_ldk_reserve() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 1_000_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 2_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // LDK reserve: 2.5% of 1M = 25k
        assert_eq!(options.min_lsp_balance_sat, 25_000);
    }

    #[test]
    fn test_min_lsp_balance_channel_minimum() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 50_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 200_000,
            max_channel_size_sat: 1_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // Channel minimum: 200k - 50k = 150k needed
        assert_eq!(options.min_lsp_balance_sat, 150_000);
    }

    #[test]
    fn test_max_client_balance() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 0,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 1_020_408,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // max_channel = 1_020_408 * 0.98 = 999,999.84 ≈ 1M
        // max_client = 97.5% of ~1M = 975k
        assert_eq!(options.max_client_balance_sat, 975_000);
    }

    #[test]
    fn test_full_calculation_with_existing_channels() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 200_000,
            existing_channels_total_sat: 300_000,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 1_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };

        let options = calculate_channel_liquidity_options(params);

        // max_channel = 1M * 0.98 = 980k, then 980k - 300k existing = 680k
        // default = 450k - 200k = 250k (below threshold_1)
        // min = max(200k * 0.025, 100k - 200k) = max(5k, 0) = 5k
        // max_lsp = 680k - 200k = 480k
        // max_client = 680k * 0.975 = 663k

        assert_eq!(options.default_lsp_balance_sat, 250_000);
        assert_eq!(options.min_lsp_balance_sat, 5_000);
        assert_eq!(options.max_lsp_balance_sat, 480_000);
        assert_eq!(options.max_client_balance_sat, 663_000);
    }

    #[test]
    fn test_boundary_at_threshold_1() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 225_000, // exactly 225€
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 1_020_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // 225 is NOT > 225, so fills to 450: LSP = 450 - 225 = 225
        assert_eq!(options.default_lsp_balance_sat, 225_000);
    }

    #[test]
    fn test_boundary_at_threshold_2() {
        let params = ChannelLiquidityParams {
            client_balance_sat: 495_000, // exactly 495€
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        let options = calculate_channel_liquidity_options(params);
        // At exactly 495€ with > (not >=): uses 1:1 ratio = 495k
        // But capped by max_lsp = 950.6k - 495k = 455.6k
        assert_eq!(options.default_lsp_balance_sat, 455_600);
    }

    #[test]
    fn test_policy_example_1_client_200() {
        // Policy example: Client = 200€, LSP = 250€ (fill to 450)
        let params = ChannelLiquidityParams {
            client_balance_sat: 200_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };

        let options = calculate_channel_liquidity_options(params);
        assert_eq!(options.default_lsp_balance_sat, 250_000);
    }

    #[test]
    fn test_policy_example_2_client_300() {
        // Policy example: Client = 300€, LSP = 300€ (1:1 ratio)
        let params = ChannelLiquidityParams {
            client_balance_sat: 300_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };

        let options = calculate_channel_liquidity_options(params);
        assert_eq!(options.default_lsp_balance_sat, 300_000);
    }

    #[test]
    fn test_policy_example_3_client_600() {
        // Policy example: Client = 600€, LSP = 350€ (max out to 950)
        let params = ChannelLiquidityParams {
            client_balance_sat: 600_000,
            existing_channels_total_sat: 0,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };

        let options = calculate_channel_liquidity_options(params);
        // max_channel = 970k * 0.98 = 950.6k, max_lsp = 950.6k - 600k = 350.6k
        assert_eq!(options.default_lsp_balance_sat, 350_600);
    }

    #[test]
    fn test_two_channels_scenario() {
        // Policy: User can open 2 channels of 450€ each (total 900€)
        let params = ChannelLiquidityParams {
            client_balance_sat: 100_000,
            existing_channels_total_sat: 450_000,
            min_channel_size_sat: 100_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };

        let options = calculate_channel_liquidity_options(params);

        // max_channel = 970k * 0.98 - 450k = 500.6k remaining
        // default = 450k - 100k = 350k (fill to 450)
        // max_lsp = 500.6k - 100k = 400.6k
        assert_eq!(options.default_lsp_balance_sat, 350_000);
        assert_eq!(options.max_lsp_balance_sat, 400_600);
    }

    #[test]
    fn test_existing_channels_near_limit() {
        // User already has 900€ in channels, very limited remaining
        let params = ChannelLiquidityParams {
            client_balance_sat: 30_000,
            existing_channels_total_sat: 900_000,
            min_channel_size_sat: 50_000,
            max_channel_size_sat: 970_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };

        let options = calculate_channel_liquidity_options(params);

        // max_channel = 970k * 0.98 - 900k = 50.6k remaining
        // default would be 450k - 30k = 420k, but capped to max_lsp
        // max_lsp = 50.6k - 30k = 20.6k
        assert_eq!(options.default_lsp_balance_sat, 20_600);
        assert_eq!(options.max_lsp_balance_sat, 20_600);
    }

    // Simple default LSP balance tests (for CJIT channels)

    #[test]
    fn test_simple_default_lsp_balance_small_amount() {
        let params = DefaultLspBalanceParams {
            client_balance_sat: 100_000,
            max_channel_size_sat: 1_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        // default_target = 450€ = 450_000 sats
        // result = 450_000 - 100_000 = 350_000
        assert_eq!(get_default_lsp_balance(params), 350_000);
    }

    #[test]
    fn test_simple_default_lsp_balance_medium_amount() {
        let params = DefaultLspBalanceParams {
            client_balance_sat: 300_000, // 300€, between 225€ and 495€
            max_channel_size_sat: 1_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        // 1:1 ratio
        assert_eq!(get_default_lsp_balance(params), 300_000);
    }

    #[test]
    fn test_simple_default_lsp_balance_large_amount() {
        let params = DefaultLspBalanceParams {
            client_balance_sat: 600_000, // 600€, above 495€
            max_channel_size_sat: 1_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        // use max
        assert_eq!(get_default_lsp_balance(params), 1_000_000);
    }

    #[test]
    fn test_simple_default_lsp_balance_capped_by_max() {
        let params = DefaultLspBalanceParams {
            client_balance_sat: 100_000,
            max_channel_size_sat: 200_000, // Small max
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        // default would be 350_000 but capped at 200_000
        assert_eq!(get_default_lsp_balance(params), 200_000);
    }

    #[test]
    fn test_simple_default_lsp_balance_zero_client() {
        // Use case: calculating minimum CJIT sats
        let params = DefaultLspBalanceParams {
            client_balance_sat: 0,
            max_channel_size_sat: 1_000_000,
            sats_per_eur: TEST_SATS_PER_EUR,
        };
        // default_target = 450€ = 450_000 sats
        assert_eq!(get_default_lsp_balance(params), 450_000);
    }
}
