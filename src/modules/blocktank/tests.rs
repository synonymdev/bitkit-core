#[cfg(test)]
mod tests {
    use rust_blocktank_client::{BitcoinNetworkEnum, FeeRates, IBtInfo, IBtInfoOnchain, IBtInfoOptions, IBtInfoVersions, ILspNode};
    use super::*;
    use crate::modules::blocktank::{BlocktankDB, BlocktankError};

    const STAGING_SERVER: &str = "https://api1.blocktank.to/api";

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
                |row| Ok((row.get::<_, i32>(0)?, row.get::<_, bool>(1)?))
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
            let rows: Vec<(i32, bool)> = conn.prepare("SELECT version, is_current FROM info ORDER BY version")
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
                |row| Ok((row.get::<_, i32>(0)?, row.get::<_, bool>(1)?))
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
            let count: i32 = conn.query_row(
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
        let db = BlocktankDB::new(":memory:", Some("http://test-blocktank.com")).await.unwrap();

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
}