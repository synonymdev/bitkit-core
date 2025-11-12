#[cfg(test)]
mod tests {
    use crate::activity::{ActivityDB, OnchainActivity, LightningActivity, PaymentType, PaymentState, Activity, ActivityFilter, ActivityType, SortDirection, ClosedChannelDetails, ActivityTags, PreActivityMetadata};
    use std::fs;
    use rand::random;

    fn setup() -> (ActivityDB, String) {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        let db = ActivityDB::new(&db_path).unwrap();
        (db, db_path)
    }

    fn cleanup(db_path: &str) {
        fs::remove_file(db_path).ok();
    }

    fn create_test_onchain_activity() -> OnchainActivity {
        OnchainActivity {
            id: "test_onchain_1".to_string(),
            tx_type: PaymentType::Sent,
            tx_id: "txid123".to_string(),
            value: 50000,
            fee: 500,
            fee_rate: 1,
            address: "bc1q...".to_string(),
            confirmed: true,
            timestamp: 1234567890,
            is_boosted: false,
            boost_tx_ids: vec![],
            is_transfer: false,
            does_exist: true,
            confirm_timestamp: Some(1234568890),
            channel_id: None,
            transfer_tx_id: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn create_test_lightning_activity() -> LightningActivity {
        LightningActivity {
            id: "test_lightning_1".to_string(),
            tx_type: PaymentType::Received,
            status: PaymentState::Succeeded,
            value: 10000,
            fee: Some(1),
            invoice: "lightning:abc".to_string(),
            message: "Test payment".to_string(),
            timestamp: 1234567890,
            preimage: Some("preimage123".to_string()),
            created_at: None,
            updated_at: None,
        }
    }

    fn create_test_closed_channel() -> ClosedChannelDetails {
        ClosedChannelDetails {
            channel_id: "channel123".to_string(),
            counterparty_node_id: "03abc123...".to_string(),
            funding_txo_txid: "funding_tx_id_123".to_string(),
            funding_txo_index: 0,
            channel_value_sats: 1000000,
            closed_at: 1234567890,
            outbound_capacity_msat: 500000000,
            inbound_capacity_msat: 500000000,
            counterparty_unspendable_punishment_reserve: 10000000,
            unspendable_punishment_reserve: 10000000,
            forwarding_fee_proportional_millionths: 1,
            forwarding_fee_base_msat: 10,
            channel_name: "Test Channel".to_string(),
            channel_closure_reason: "CooperativeClose".to_string(),
        }
    }

    fn create_test_pre_activity_metadata(payment_id: String, _payment_type: ActivityType, tags: Vec<String>) -> PreActivityMetadata {
        PreActivityMetadata {
            payment_id,
            tags,
            payment_hash: None,
            tx_id: None,
            address: None,
            is_receive: false,
            created_at: 0,
        }
    }

    #[test]
    fn test_db_initialization() {
        let (db, db_path) = setup();
        assert!(db.conn.is_autocommit(), "Database should be in autocommit mode");
        cleanup(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_onchain_activity() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        assert!(db.insert_onchain_activity(&activity).is_ok());

        let activities = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, None, None).unwrap();
        assert_eq!(activities.len(), 1);
        if let Activity::Onchain(retrieved) = &activities[0] {
            assert_eq!(retrieved.id, activity.id);
            assert_eq!(retrieved.value, activity.value);
            assert_eq!(retrieved.fee, activity.fee);
            assert!(retrieved.created_at.is_some());
            assert!(retrieved.updated_at.is_some());
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_lightning_activity() {
        let (mut db, db_path) = setup();
        let activity = create_test_lightning_activity();
        assert!(db.insert_lightning_activity(&activity).is_ok());

        let activities = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, None, None).unwrap();
        assert_eq!(activities.len(), 1);
        if let Activity::Lightning(retrieved) = &activities[0] {
            assert_eq!(retrieved.id, activity.id);
            assert_eq!(retrieved.value, activity.value);
            assert_eq!(retrieved.message, activity.message);
            assert!(retrieved.created_at.is_some());
            assert!(retrieved.updated_at.is_some());
        } else {
            panic!("Expected Lightning activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_activities() {
        let (mut db, db_path) = setup();
        let onchain = create_test_onchain_activity();
        let lightning = create_test_lightning_activity();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        let all_activities = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, None).unwrap();
        assert_eq!(all_activities.len(), 2);

        // Check ordering by timestamp descending (they have the same timestamp in this test)
        // The order should not matter if they have identical timestamps, but both should appear.
        assert!(all_activities.iter().any(|a| a.get_id() == onchain.id));
        assert!(all_activities.iter().any(|a| a.get_id() == lightning.id));

        cleanup(&db_path);
    }

    #[test]
    fn test_activity_timestamps() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, None, None).unwrap();
        if let Activity::Onchain(activity) = &retrieved[0] {
            assert!(activity.created_at.is_some());
            assert!(activity.updated_at.is_some());
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_concurrent_access() {
        let (mut db, db_path) = setup();
        let mut db_clone = ActivityDB::new(&db_path).unwrap();

        let activity1 = create_test_onchain_activity();
        let mut activity2 = create_test_lightning_activity();
        activity2.id = "test_lightning_concurrent".to_string();

        db.insert_onchain_activity(&activity1).unwrap();
        db_clone.insert_lightning_activity(&activity2).unwrap();

        let all_activities = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, None).unwrap();
        assert_eq!(all_activities.len(), 2);

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_activities_ordering() {
        let (mut db, db_path) = setup();
        let mut onchain1 = create_test_onchain_activity();
        onchain1.timestamp = 1000;
        let mut onchain2 = create_test_onchain_activity();
        onchain2.id = "test_onchain_2".to_string();
        onchain2.timestamp = 2000;
        let mut lightning = create_test_lightning_activity();
        lightning.timestamp = 1500;

        db.insert_onchain_activity(&onchain1).unwrap();
        db.insert_onchain_activity(&onchain2).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        let activities = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, None).unwrap();
        let timestamps: Vec<u64> = activities.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(timestamps, vec![2000, 1500, 1000]);

        cleanup(&db_path);
    }

    #[test]
    fn test_limits_on_activities() {
        let (mut db, db_path) = setup();

        // Insert multiple activities
        for i in 0..5 {
            let mut onchain = create_test_onchain_activity();
            onchain.id = format!("test_onchain_{}", i);
            onchain.timestamp = 1234567890 + i as u64;
            db.insert_onchain_activity(&onchain).unwrap();

            let mut lightning = create_test_lightning_activity();
            lightning.id = format!("test_lightning_{}", i);
            lightning.timestamp = 1234567890 + i as u64;
            db.insert_lightning_activity(&lightning).unwrap();
        }

        // Test limits with different filters
        let all = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, Some(3), None).unwrap();
        assert_eq!(all.len(), 3);

        let onchain = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, Some(2), None).unwrap();
        assert_eq!(onchain.len(), 2);

        let lightning = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, Some(4), None).unwrap();
        assert_eq!(lightning.len(), 4);

        // Test without limits
        let all = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, None).unwrap();
        assert_eq!(all.len(), 10);

        cleanup(&db_path);
    }

    #[test]
    fn test_zero_limit() {
        let (mut db, db_path) = setup();
        db.insert_onchain_activity(&create_test_onchain_activity()).unwrap();
        db.insert_lightning_activity(&create_test_lightning_activity()).unwrap();

        let all = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, Some(0), None).unwrap();
        assert_eq!(all.len(), 0);

        let onchain = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, Some(0), None).unwrap();
        assert_eq!(onchain.len(), 0);

        let lightning = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, Some(0), None).unwrap();
        assert_eq!(lightning.len(), 0);

        cleanup(&db_path);
    }

    #[test]
    fn test_tags_add_retrieve() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = vec!["payment".to_string(), "coffee".to_string()];
        db.add_tags(&activity.id, &tags).unwrap();
        let retrieved_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(retrieved_tags.len(), 2);
        assert!(retrieved_tags.contains(&"payment".to_string()));
        assert!(retrieved_tags.contains(&"coffee".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_tags_remove() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = vec!["payment".to_string(), "coffee".to_string()];
        db.add_tags(&activity.id, &tags).unwrap();

        db.remove_tags(&activity.id, &vec!["payment".to_string()]).unwrap();
        let remaining_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(remaining_tags.len(), 1);
        assert_eq!(remaining_tags[0], "coffee");

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activities_by_tag() {
        let (mut db, db_path) = setup();
        let onchain = create_test_onchain_activity();
        let mut lightning = create_test_lightning_activity();
        lightning.id = "test_lightning_tagged".to_string();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        db.add_tags(&onchain.id, &["payment".to_string()]).unwrap();
        db.add_tags(&lightning.id, &["payment".to_string()]).unwrap();

        let activities = db.get_activities_by_tag("payment", None, None).unwrap();
        assert_eq!(activities.len(), 2);

        let limited = db.get_activities_by_tag("payment", Some(1), None).unwrap();
        assert_eq!(limited.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_tags_on_nonexistent_activity() {
        let (mut db, db_path) = setup();
        let tags = vec!["test".to_string()];
        assert!(db.add_tags("nonexistent", &tags).is_err());
        cleanup(&db_path);
    }

    #[test]
    fn test_duplicate_tags() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = vec!["test".to_string(), "test".to_string()];
        db.add_tags(&activity.id, &tags).unwrap();

        let retrieved_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(retrieved_tags.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_get_tags_empty() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = db.get_tags(&activity.id).unwrap();
        assert!(tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_activity_removes_tags() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        db.add_tags(&activity.id, &["test".to_string()]).unwrap();
        db.delete_activity_by_id(&activity.id).unwrap();

        let tags = db.get_tags(&activity.id).unwrap();
        assert!(tags.is_empty(), "Tags should be removed after activity deletion");

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activities_by_nonexistent_tag() {
        let (db, db_path) = setup();
        let activities = db.get_activities_by_tag("nonexistent", None, None).unwrap();
        assert!(activities.is_empty());
        cleanup(&db_path);
    }

    #[test]
    fn test_operations_after_deletion() {
        let (mut db, db_path) = setup();

        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();
        db.delete_activity_by_id(&activity.id).unwrap();

        // These operations should fail or return empty results after deletion
        assert!(db.get_activity_by_id(&activity.id).unwrap().is_none());
        assert!(db.update_onchain_activity_by_id(&activity.id, &activity).is_err());
        assert!(db.add_tags(&activity.id, &["test".to_string()]).is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_large_numeric_values() {
        let (mut db, db_path) = setup();

        // Use a large but safe value instead of i64::MAX
        let mut activity = create_test_onchain_activity();
        let safe_max = 1_000_000_000_000;
        activity.value = safe_max;
        activity.fee = safe_max - 1;
        activity.fee_rate = safe_max - 2;
        activity.timestamp = safe_max - 3;
        activity.confirm_timestamp = Some(safe_max - 1);

        let result = db.insert_onchain_activity(&activity);
        assert!(result.is_ok(), "Failed to insert activity: {:?}", result.err());

        let retrieved = db.get_activity_by_id(&activity.id).unwrap().unwrap();
        if let Activity::Onchain(retrieved) = retrieved {
            assert_eq!(retrieved.value, safe_max);
            assert_eq!(retrieved.fee, safe_max - 1);
            assert_eq!(retrieved.fee_rate, safe_max - 2);
            assert_eq!(retrieved.timestamp, safe_max - 3);
            assert_eq!(retrieved.confirm_timestamp, Some(safe_max - 1));
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_zero_values() {
        let (mut db, db_path) = setup();

        // Test zero value handling
        let mut activity = create_test_onchain_activity();
        activity.value = 0;
        activity.fee = 0;
        activity.fee_rate = 0;

        assert!(db.insert_onchain_activity(&activity).is_ok());

        let retrieved = db.get_activity_by_id(&activity.id).unwrap().unwrap();
        if let Activity::Onchain(retrieved) = retrieved {
            assert_eq!(retrieved.value, 0);
            assert_eq!(retrieved.fee, 0);
            assert_eq!(retrieved.fee_rate, 0);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_lightning_optional_fee() {
        let (mut db, db_path) = setup();

        // Test None fee
        let mut activity = create_test_lightning_activity();
        activity.fee = None;
        assert!(db.insert_lightning_activity(&activity).is_ok());

        // Test Some(0) fee
        activity.id = "test_lightning_2".to_string();
        activity.fee = Some(0);
        assert!(db.insert_lightning_activity(&activity).is_ok());

        // Test Some(max) fee - use i64::MAX instead of u64::MAX
        activity.id = "test_lightning_3".to_string();
        activity.fee = Some(i64::MAX as u64);
        assert!(db.insert_lightning_activity(&activity).is_ok());

        let activities = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, None, None).unwrap();
        assert_eq!(activities.len(), 3);

        for act in activities {
            if let Activity::Lightning(lightning) = act {
                match lightning.id.as_str() {
                    "test_lightning_1" => assert_eq!(lightning.fee, None),
                    "test_lightning_2" => assert_eq!(lightning.fee, Some(0)),
                    "test_lightning_3" => assert_eq!(lightning.fee, Some(i64::MAX as u64)),
                    _ => panic!("Unexpected activity ID"),
                }
            } else {
                panic!("Expected Lightning activity");
            }
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_timestamp_conversions() {
        let (mut db, db_path) = setup();

        // Test various timestamp scenarios
        let mut activity = create_test_onchain_activity();
        activity.timestamp = 0;
        activity.confirm_timestamp = Some(0);
        assert!(db.insert_onchain_activity(&activity).is_err()); // Should fail due to timestamp > 0 constraint

        activity.timestamp = 1;
        activity.confirm_timestamp = Some(0);
        assert!(db.insert_onchain_activity(&activity).is_err()); // Should fail due to confirm_timestamp >= timestamp constraint

        activity.timestamp = 1000;
        activity.confirm_timestamp = Some(2000);
        assert!(db.insert_onchain_activity(&activity).is_ok());

        let retrieved = db.get_activity_by_id(&activity.id).unwrap().unwrap();
        if let Activity::Onchain(retrieved) = retrieved {
            assert_eq!(retrieved.timestamp, 1000);
            assert_eq!(retrieved.confirm_timestamp, Some(2000));
            assert!(retrieved.created_at.unwrap() > 0);
            assert!(retrieved.updated_at.unwrap() > 0);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_value_update() {
        let (mut db, db_path) = setup();

        let mut activity = create_test_onchain_activity();
        activity.value = 1000;
        assert!(db.insert_onchain_activity(&activity).is_ok());

        std::thread::sleep(std::time::Duration::from_millis(1));

        // Use a large but safe value
        activity.value = 1_000_000_000_000;
        assert!(db.update_onchain_activity_by_id(&activity.id, &activity).is_ok());

        let retrieved = db.get_activity_by_id(&activity.id).unwrap().unwrap();
        if let Activity::Onchain(retrieved) = retrieved {
            assert_eq!(retrieved.value, 1_000_000_000_000);
            assert!(retrieved.created_at.is_some());
            assert!(retrieved.updated_at.is_some());
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_onchain_activity_insert_then_update() {
        let (mut db, db_path) = setup();

        // Create initial activity
        let mut onchain = create_test_onchain_activity();
        let activity = Activity::Onchain(onchain.clone());

        // Test insert path
        assert!(db.upsert_activity(&activity).is_ok());

        let retrieved = db.get_activity_by_id(&onchain.id).unwrap().unwrap();
        if let Activity::Onchain(retrieved) = retrieved {
            assert_eq!(retrieved.value, onchain.value);
            assert!(retrieved.created_at.is_some());
            let first_update = retrieved.updated_at;

            // Test update path
            std::thread::sleep(std::time::Duration::from_secs(1));
            onchain.value = 100_000;
            let updated = Activity::Onchain(onchain);
            assert!(db.upsert_activity(&updated).is_ok());

            // Verify update
            let retrieved = db.get_activity_by_id(&updated.get_id()).unwrap().unwrap();
            if let Activity::Onchain(retrieved) = retrieved {
                assert_eq!(retrieved.value, 100_000);
                assert!(retrieved.updated_at > first_update);
            }
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_lightning_activity_with_status_change() {
        let (mut db, db_path) = setup();

        // Create initial pending activity
        let mut lightning = create_test_lightning_activity();
        lightning.status = PaymentState::Pending;
        let activity = Activity::Lightning(lightning.clone());

        // Test insert
        assert!(db.upsert_activity(&activity).is_ok());

        let retrieved = db.get_activity_by_id(&lightning.id).unwrap().unwrap();
        if let Activity::Lightning(retrieved) = retrieved {
            assert_eq!(retrieved.status, PaymentState::Pending);

            // Update to succeeded
            std::thread::sleep(std::time::Duration::from_millis(1));
            lightning.status = PaymentState::Succeeded;
            let updated = Activity::Lightning(lightning);
            assert!(db.upsert_activity(&updated).is_ok());

            // Verify status change
            let retrieved = db.get_activity_by_id(&updated.get_id()).unwrap().unwrap();
            if let Activity::Lightning(retrieved) = retrieved {
                assert_eq!(retrieved.status, PaymentState::Succeeded);
                assert!(retrieved.created_at.is_some());
                assert!(retrieved.updated_at.is_some());
            }
        } else {
            panic!("Expected Lightning activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_activity_invalid_id() {
        let (mut db, db_path) = setup();
        let mut activity = create_test_onchain_activity();
        activity.id = "".to_string();
        let activity = Activity::Onchain(activity);
        assert!(db.upsert_activity(&activity).is_err());
        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_activity_timestamps() {
        let (mut db, db_path) = setup();

        let mut onchain = create_test_onchain_activity();
        let activity = Activity::Onchain(onchain.clone());
        assert!(db.upsert_activity(&activity).is_ok());

        let initial = db.get_activity_by_id(&onchain.id).unwrap().unwrap();
        if let Activity::Onchain(initial) = initial {
            let created_at = initial.created_at.unwrap();

            // Update and verify created_at stays the same
            std::thread::sleep(std::time::Duration::from_secs(1));
            onchain.value = 100_000;
            let updated = Activity::Onchain(onchain);
            assert!(db.upsert_activity(&updated).is_ok());

            let retrieved = db.get_activity_by_id(&updated.get_id()).unwrap().unwrap();
            if let Activity::Onchain(retrieved) = retrieved {
                assert_eq!(retrieved.created_at.unwrap(), created_at);
                assert!(retrieved.updated_at.unwrap() > initial.updated_at.unwrap());
            }
        }
        cleanup(&db_path);
    }

    #[test]
    fn test_sort_direction_activities() {
        let (mut db, db_path) = setup();

        // Insert activities with different timestamps
        let mut activities = Vec::new();
        for i in 0..3 {
            let mut onchain = create_test_onchain_activity();
            onchain.id = format!("test_onchain_{}", i);
            onchain.timestamp = 1000 + i as u64;
            activities.push(onchain);
        }

        // Insert in random order
        for activity in activities.iter() {
            db.insert_onchain_activity(activity).unwrap();
        }

        // Test ascending order
        let asc_results = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, Some(SortDirection::Asc)).unwrap();
        let asc_timestamps: Vec<u64> = asc_results.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 1001, 1002]);

        // Test descending order
        let desc_results = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, Some(SortDirection::Desc)).unwrap();
        let desc_timestamps: Vec<u64> = desc_results.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(desc_timestamps, vec![1002, 1001, 1000]);

        cleanup(&db_path);
    }

    #[test]
    fn test_sort_direction_with_tags() {
        let (mut db, db_path) = setup();

        // Create activities with different timestamps and same tag
        let mut onchain1 = create_test_onchain_activity();
        onchain1.timestamp = 1000;
        let mut onchain2 = create_test_onchain_activity();
        onchain2.id = "test_onchain_2".to_string();
        onchain2.timestamp = 2000;

        db.insert_onchain_activity(&onchain1).unwrap();
        db.insert_onchain_activity(&onchain2).unwrap();

        // Add same tag to both
        let tag = "test_tag".to_string();
        db.add_tags(&onchain1.id, &[tag.clone()]).unwrap();
        db.add_tags(&onchain2.id, &[tag.clone()]).unwrap();

        // Test ascending order
        let asc_activities = db.get_activities_by_tag(&tag, None, Some(SortDirection::Asc)).unwrap();
        let asc_timestamps: Vec<u64> = asc_activities.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 2000]);

        // Test descending order
        let desc_activities = db.get_activities_by_tag(&tag, None, Some(SortDirection::Desc)).unwrap();
        let desc_timestamps: Vec<u64> = desc_activities.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(desc_timestamps, vec![2000, 1000]);

        cleanup(&db_path);
    }

    #[test]
    fn test_sort_direction_with_limit() {
        let (mut db, db_path) = setup();

        // Insert 5 activities with sequential timestamps
        for i in 0..5 {
            let mut onchain = create_test_onchain_activity();
            onchain.id = format!("test_onchain_{}", i);
            onchain.timestamp = 1000 + i as u64;
            db.insert_onchain_activity(&onchain).unwrap();
        }

        // Test ascending order with limit
        let asc_limited = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, Some(3), Some(SortDirection::Asc)).unwrap();
        let asc_timestamps: Vec<u64> = asc_limited.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 1001, 1002]);

        // Test descending order with limit
        let desc_limited = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, Some(3), Some(SortDirection::Desc)).unwrap();
        let desc_timestamps: Vec<u64> = desc_limited.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(desc_timestamps, vec![1004, 1003, 1002]);

        cleanup(&db_path);
    }

    #[test]
    fn test_sort_direction_mixed_types() {
        let (mut db, db_path) = setup();

        // Create mix of onchain and lightning activities with different timestamps
        let mut onchain = create_test_onchain_activity();
        onchain.timestamp = 1000;

        let mut lightning = create_test_lightning_activity();
        lightning.timestamp = 2000;

        let mut onchain2 = create_test_onchain_activity();
        onchain2.id = "test_onchain_2".to_string();
        onchain2.timestamp = 3000;

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();
        db.insert_onchain_activity(&onchain2).unwrap();

        // Test ascending order
        let asc_results = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, Some(SortDirection::Asc)).unwrap();
        let asc_timestamps: Vec<u64> = asc_results.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 2000, 3000]);

        // Verify correct activity types are maintained in order
        assert!(matches!(asc_results[0], Activity::Onchain(_)));
        assert!(matches!(asc_results[1], Activity::Lightning(_)));
        assert!(matches!(asc_results[2], Activity::Onchain(_)));

        cleanup(&db_path);
    }

    #[test]
    fn test_default_sort_direction() {
        let (mut db, db_path) = setup();

        // Insert activities with different timestamps
        let mut onchain1 = create_test_onchain_activity();
        onchain1.timestamp = 1000;
        let mut onchain2 = create_test_onchain_activity();
        onchain2.id = "test_onchain_2".to_string();
        onchain2.timestamp = 2000;

        db.insert_onchain_activity(&onchain1).unwrap();
        db.insert_onchain_activity(&onchain2).unwrap();

        // Test with None sort direction (should default to Desc)
        let default_results = db.get_activities(Some(ActivityFilter::All), None, None, None, None, None, None, None).unwrap();
        let timestamps: Vec<u64> = default_results.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(timestamps, vec![2000, 1000]);

        cleanup(&db_path);
    }

    #[test]
    fn test_payment_type_filtering() {
        let (mut db, db_path) = setup();

        // Create activities with different payment types
        let mut sent_activity = create_test_onchain_activity();
        sent_activity.tx_type = PaymentType::Sent;

        let mut received_activity = create_test_onchain_activity();
        received_activity.id = "test_onchain_2".to_string();
        received_activity.tx_type = PaymentType::Received;

        db.insert_onchain_activity(&sent_activity).unwrap();
        db.insert_onchain_activity(&received_activity).unwrap();

        // Test filtering by sent
        let sent_activities = db.get_activities(
            Some(ActivityFilter::All),
            Some(PaymentType::Sent),
            None,
            None,
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(sent_activities.len(), 1);
        assert!(matches!(sent_activities[0], Activity::Onchain(ref a) if a.tx_type == PaymentType::Sent));

        // Test filtering by received
        let received_activities = db.get_activities(
            Some(ActivityFilter::All),
            Some(PaymentType::Received),
            None,
            None,
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(received_activities.len(), 1);
        assert!(matches!(received_activities[0], Activity::Onchain(ref a) if a.tx_type == PaymentType::Received));

        cleanup(&db_path);
    }

    #[test]
    fn test_text_search() {
        let (mut db, db_path) = setup();

        let mut onchain = create_test_onchain_activity();
        onchain.address = "bc1qxyz123".to_string();

        let mut lightning = create_test_lightning_activity();
        lightning.message = "Coffee payment".to_string();
        lightning.invoice = "lnbc123xyz".to_string();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        // Test address search
        let address_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("xyz123".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(address_results.len(), 1);
        assert!(matches!(address_results[0], Activity::Onchain(_)));

        // Test message search
        let message_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("Coffee".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(message_results.len(), 1);
        assert!(matches!(message_results[0], Activity::Lightning(_)));

        cleanup(&db_path);
    }

    #[test]
    fn test_date_range_filtering() {
        let (mut db, db_path) = setup();

        let mut activity1 = create_test_onchain_activity();
        activity1.timestamp = 1000;

        let mut activity2 = create_test_onchain_activity();
        activity2.id = "test_onchain_2".to_string();
        activity2.timestamp = 2000;

        let mut activity3 = create_test_onchain_activity();
        activity3.id = "test_onchain_3".to_string();
        activity3.timestamp = 3000;

        db.insert_onchain_activity(&activity1).unwrap();
        db.insert_onchain_activity(&activity2).unwrap();
        db.insert_onchain_activity(&activity3).unwrap();

        // Test min date
        let min_date_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            None,
            Some(1500),
            None,
            None,
            None
        ).unwrap();
        assert_eq!(min_date_results.len(), 2);

        // Test max date
        let max_date_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            None,
            None,
            Some(2500),
            None,
            None
        ).unwrap();
        assert_eq!(max_date_results.len(), 2);

        // Test date range
        let range_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            None,
            Some(1500),
            Some(2500),
            None,
            None
        ).unwrap();
        assert_eq!(range_results.len(), 1);
        assert_eq!(range_results[0].get_timestamp(), 2000);

        cleanup(&db_path);
    }

    #[test]
    fn test_combined_filtering() {
        let (mut db, db_path) = setup();

        let mut onchain1 = create_test_onchain_activity();
        onchain1.timestamp = 1000;
        onchain1.address = "bc1qxyz".to_string();
        onchain1.tx_type = PaymentType::Sent;

        let mut onchain2 = create_test_onchain_activity();
        onchain2.id = "test_onchain_2".to_string();
        onchain2.timestamp = 2000;
        onchain2.address = "bc1qabc".to_string();
        onchain2.tx_type = PaymentType::Received;

        db.insert_onchain_activity(&onchain1).unwrap();
        db.insert_onchain_activity(&onchain2).unwrap();

        // Add tags
        db.add_tags(&onchain1.id, &["payment".to_string()]).unwrap();
        db.add_tags(&onchain2.id, &["payment".to_string(), "important".to_string()]).unwrap();

        // Test combined filters
        let results = db.get_activities(
            Some(ActivityFilter::Onchain),
            Some(PaymentType::Received),
            Some(vec!["payment".to_string()]),
            Some("abc".to_string()),
            Some(1500),
            Some(2500),
            Some(1),
            Some(SortDirection::Desc)
        ).unwrap();

        assert_eq!(results.len(), 1);
        if let Activity::Onchain(activity) = &results[0] {
            assert_eq!(activity.id, "test_onchain_2");
            assert_eq!(activity.tx_type, PaymentType::Received);
            assert_eq!(activity.timestamp, 2000);
            assert_eq!(activity.address, "bc1qabc");
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_empty_search_terms() {
        let (mut db, db_path) = setup();

        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Test empty search string - should return all results, same as if no search was provided
        let empty_search = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(empty_search.len(), 1); // Changed from 0 to 1

        // Test empty tags array
        let empty_tags = db.get_activities(
            Some(ActivityFilter::All),
            None,
            Some(vec![]),
            None,
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(empty_tags.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_multiple_tags_filtering() {
        let (mut db, db_path) = setup();

        // Create activities with different tag combinations
        let activity1 = create_test_onchain_activity();
        let mut activity2 = create_test_onchain_activity();
        activity2.id = "test_onchain_2".to_string();
        let mut activity3 = create_test_onchain_activity();
        activity3.id = "test_onchain_3".to_string();

        db.insert_onchain_activity(&activity1).unwrap();
        db.insert_onchain_activity(&activity2).unwrap();
        db.insert_onchain_activity(&activity3).unwrap();

        // Add different tag combinations
        db.add_tags(&activity1.id, &["tag1".to_string(), "tag2".to_string()]).unwrap();
        db.add_tags(&activity2.id, &["tag2".to_string(), "tag3".to_string()]).unwrap();
        db.add_tags(&activity3.id, &["tag1".to_string(), "tag3".to_string()]).unwrap();

        // Test filtering with multiple tags (OR condition)
        let results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            Some(vec!["tag1".to_string(), "tag2".to_string()]),
            None,
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(results.len(), 3);

        // Test with non-existent tag mixed with existing tags
        let mixed_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            Some(vec!["tag1".to_string(), "nonexistent".to_string()]),
            None,
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(mixed_results.len(), 2);

        cleanup(&db_path);
    }

    #[test]
    fn test_invalid_date_ranges() {
        let (mut db, db_path) = setup();

        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Test max date before min date
        let invalid_range = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            None,
            Some(2000),
            Some(1000),
            None,
            None
        ).unwrap();
        assert_eq!(invalid_range.len(), 0);

        // Test dates way in the future
        let future_date = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            None,
            Some(u64::MAX - 1000),
            None,
            None,
            None
        ).unwrap();
        assert_eq!(future_date.len(), 0);

        cleanup(&db_path);
    }

    #[test]
    fn test_case_insensitive_search() {
        let (mut db, db_path) = setup();

        let mut lightning = create_test_lightning_activity();
        lightning.message = "Test Coffee Payment".to_string();
        db.insert_lightning_activity(&lightning).unwrap();

        // Test lowercase search
        let lower_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("coffee".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(lower_results.len(), 1);

        // Test uppercase search
        let upper_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("COFFEE".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(upper_results.len(), 1);

        // Test mixed case search
        let mixed_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("CoFfEe".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(mixed_results.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_concurrent_tag_operations() {
        let (mut db, db_path) = setup();
        let mut db_clone = ActivityDB::new(&db_path).unwrap();

        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Add tags from both connections
        db.add_tags(&activity.id, &["tag1".to_string()]).unwrap();
        db_clone.add_tags(&activity.id, &["tag2".to_string()]).unwrap();

        // Verify tags from both connections
        let results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            Some(vec!["tag1".to_string(), "tag2".to_string()]),
            None,
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(results.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_special_characters_search() {
        let (mut db, db_path) = setup();

        let mut onchain = create_test_onchain_activity();
        onchain.address = "bc1q_special%chars".to_string();

        let mut lightning = create_test_lightning_activity();
        lightning.message = "Test with % and _ characters".to_string();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        // Search with special characters
        let special_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("%chars".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(special_results.len(), 1);

        // Search with underscore
        let underscore_results = db.get_activities(
            Some(ActivityFilter::All),
            None,
            None,
            Some("_special".to_string()),
            None,
            None,
            None,
            None
        ).unwrap();
        assert_eq!(underscore_results.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_pagination_with_filters() {
        let (mut db, db_path) = setup();

        // Create multiple activities
        for i in 0..5 {
            let mut activity = create_test_onchain_activity();
            activity.id = format!("test_onchain_{}", i);
            activity.timestamp = 1000 + i as u64;
            activity.address = format!("bc1q_address_{}", i);
            db.insert_onchain_activity(&activity).unwrap();

            // Add tags to even numbered activities
            if i % 2 == 0 {
                db.add_tags(&activity.id, &["even".to_string()]).unwrap();
            }
        }

        // Test pagination with combined filters
        let page1 = db.get_activities(
            Some(ActivityFilter::All),
            None,
            Some(vec!["even".to_string()]),
            Some("address".to_string()),
            Some(1000),
            None,
            Some(2),
            Some(SortDirection::Asc)
        ).unwrap();
        assert_eq!(page1.len(), 2);

        // Get next page
        let min_date = page1.last().unwrap().get_timestamp();
        let page2 = db.get_activities(
            Some(ActivityFilter::All),
            None,
            Some(vec!["even".to_string()]),
            Some("address".to_string()),
            Some(min_date + 1),
            None,
            Some(2),
            Some(SortDirection::Asc)
        ).unwrap();

        assert_eq!(page2.len(), 1);
        assert!(page2[0].get_timestamp() > page1[1].get_timestamp());

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_tags() {
        let (mut db, db_path) = setup();

        // Create some activities with different tags
        let activity1 = create_test_onchain_activity();
        let mut activity2 = create_test_onchain_activity();
        activity2.id = "test_onchain_2".to_string();

        db.insert_onchain_activity(&activity1).unwrap();
        db.insert_onchain_activity(&activity2).unwrap();

        // Add various tags
        db.add_tags(&activity1.id, &["payment".to_string(), "coffee".to_string()]).unwrap();
        db.add_tags(&activity2.id, &["payment".to_string(), "food".to_string()]).unwrap();

        // Get all unique tags
        let all_tags = db.get_all_unique_tags().unwrap();

        // Check results
        assert_eq!(all_tags.len(), 3); // Should be ["coffee", "food", "payment"]
        assert!(all_tags.contains(&"coffee".to_string()));
        assert!(all_tags.contains(&"food".to_string()));
        assert!(all_tags.contains(&"payment".to_string()));

        // Verify they're sorted alphabetically
        assert_eq!(all_tags, vec!["coffee", "food", "payment"]);

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags() {
        let (mut db, db_path) = setup();

        // Create activities
        let activity1 = create_test_onchain_activity();
        let mut activity2 = create_test_onchain_activity();
        activity2.id = "test_onchain_2".to_string();
        let mut activity3 = create_test_lightning_activity();
        activity3.id = "test_lightning_3".to_string();

        db.insert_onchain_activity(&activity1).unwrap();
        db.insert_onchain_activity(&activity2).unwrap();
        db.insert_lightning_activity(&activity3).unwrap();

        // Bulk upsert tags
        let activity_tags = vec![
            ActivityTags {
                activity_id: activity1.id.clone(),
                tags: vec!["payment".to_string(), "coffee".to_string()],
            },
            ActivityTags {
                activity_id: activity2.id.clone(),
                tags: vec!["payment".to_string(), "food".to_string()],
            },
            ActivityTags {
                activity_id: activity3.id.clone(),
                tags: vec!["payment".to_string()],
            },
        ];

        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify tags were added
        let tags1 = db.get_tags(&activity1.id).unwrap();
        assert_eq!(tags1.len(), 2);
        assert!(tags1.contains(&"payment".to_string()));
        assert!(tags1.contains(&"coffee".to_string()));

        let tags2 = db.get_tags(&activity2.id).unwrap();
        assert_eq!(tags2.len(), 2);
        assert!(tags2.contains(&"payment".to_string()));
        assert!(tags2.contains(&"food".to_string()));

        let tags3 = db.get_tags(&activity3.id).unwrap();
        assert_eq!(tags3.len(), 1);
        assert!(tags3.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_idempotent() {
        let (mut db, db_path) = setup();

        // Create activity
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // First upsert
        let activity_tags = vec![
            ActivityTags {
                activity_id: activity.id.clone(),
                tags: vec!["payment".to_string(), "coffee".to_string()],
            },
        ];
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Second upsert with same tags (should be idempotent)
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify tags are still there and not duplicated
        let tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"payment".to_string()));
        assert!(tags.contains(&"coffee".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_adds_new_tags() {
        let (mut db, db_path) = setup();

        // Create activity and add initial tags
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();
        db.add_tags(&activity.id, &["payment".to_string()]).unwrap();

        // Upsert with additional tags (adds new tags, keeps existing)
        let activity_tags = vec![
            ActivityTags {
                activity_id: activity.id.clone(),
                tags: vec!["payment".to_string(), "coffee".to_string(), "food".to_string()],
            },
        ];
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify all tags are present (payment was already there, coffee and food are new)
        let tags = db.get_tags(&activity.id).unwrap();
        assert!(tags.len() >= 3);
        assert!(tags.contains(&"payment".to_string()));
        assert!(tags.contains(&"coffee".to_string()));
        assert!(tags.contains(&"food".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_skips_empty_tags() {
        let (mut db, db_path) = setup();

        // Create activity
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Upsert with empty tags mixed in
        let activity_tags = vec![
            ActivityTags {
                activity_id: activity.id.clone(),
                tags: vec!["payment".to_string(), "".to_string(), "coffee".to_string()],
            },
        ];
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify only non-empty tags were added
        let tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"payment".to_string()));
        assert!(tags.contains(&"coffee".to_string()));
        assert!(!tags.contains(&"".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_multiple_activities() {
        let (mut db, db_path) = setup();

        // Create multiple activities
        let activity1 = create_test_onchain_activity();
        let mut activity2 = create_test_onchain_activity();
        activity2.id = "test_onchain_2".to_string();
        let mut activity3 = create_test_lightning_activity();
        activity3.id = "test_lightning_3".to_string();

        db.insert_onchain_activity(&activity1).unwrap();
        db.insert_onchain_activity(&activity2).unwrap();
        db.insert_lightning_activity(&activity3).unwrap();

        // Bulk upsert tags for all activities in one call
        let activity_tags = vec![
            ActivityTags {
                activity_id: activity1.id.clone(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
            },
            ActivityTags {
                activity_id: activity2.id.clone(),
                tags: vec!["tag2".to_string(), "tag3".to_string()],
            },
            ActivityTags {
                activity_id: activity3.id.clone(),
                tags: vec!["tag1".to_string(), "tag3".to_string(), "tag4".to_string()],
            },
        ];

        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify all tags were added correctly
        let tags1 = db.get_tags(&activity1.id).unwrap();
        assert_eq!(tags1.len(), 2);
        assert!(tags1.contains(&"tag1".to_string()));
        assert!(tags1.contains(&"tag2".to_string()));

        let tags2 = db.get_tags(&activity2.id).unwrap();
        assert_eq!(tags2.len(), 2);
        assert!(tags2.contains(&"tag2".to_string()));
        assert!(tags2.contains(&"tag3".to_string()));

        let tags3 = db.get_tags(&activity3.id).unwrap();
        assert_eq!(tags3.len(), 3);
        assert!(tags3.contains(&"tag1".to_string()));
        assert!(tags3.contains(&"tag3".to_string()));
        assert!(tags3.contains(&"tag4".to_string()));

        cleanup(&db_path);
    }

    // ========== Activity Tags Tests ==========

    #[test]
    fn test_get_all_activities_tags() {
        let (mut db, db_path) = setup();

        // Create onchain and lightning activities
        let mut onchain = create_test_onchain_activity();
        onchain.id = "onchain_1".to_string();
        let mut lightning = create_test_lightning_activity();
        lightning.id = "lightning_1".to_string();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        // Add tags
        db.add_tags(&onchain.id, &["payment".to_string(), "coffee".to_string()]).unwrap();
        db.add_tags(&lightning.id, &["payment".to_string()]).unwrap();

        // Get all activity tags
        let activity_tags = db.get_all_activities_tags().unwrap();

        assert_eq!(activity_tags.len(), 2);

        // Find onchain tags
        let onchain_tags = activity_tags.iter().find(|at| at.activity_id == onchain.id).unwrap();
        assert_eq!(onchain_tags.activity_id, onchain.id);
        assert_eq!(onchain_tags.tags.len(), 2);
        assert!(onchain_tags.tags.contains(&"payment".to_string()));
        assert!(onchain_tags.tags.contains(&"coffee".to_string()));

        // Find lightning tags
        let lightning_tags = activity_tags.iter().find(|at| at.activity_id == lightning.id).unwrap();
        assert_eq!(lightning_tags.activity_id, lightning.id);
        assert_eq!(lightning_tags.tags.len(), 1);
        assert!(lightning_tags.tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_activities_tags_empty() {
        let (db, db_path) = setup();

        let activity_tags = db.get_all_activities_tags().unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_empty_tags() {
        let (mut db, db_path) = setup();

        // Create activity with tags
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();
        db.add_tags(&activity.id, &["old_tag".to_string()]).unwrap();

        // Upsert with empty tags (with INSERT OR IGNORE, won't clear existing tags)
        let activity_tags = vec![
            ActivityTags {
                activity_id: activity.id.clone(),
                tags: vec![],
            },
        ];

        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify old tags still exist (empty tags list doesn't clear)
        let tags = db.get_tags(&activity.id).unwrap();
        assert!(tags.contains(&"old_tag".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_empty_input() {
        let (mut db, db_path) = setup();

        // Test with empty vector
        assert!(db.upsert_tags(&[]).is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_tags_empty_activity_id() {
        let (mut db, db_path) = setup();

        // Test with empty activity_id
        let activity_tags = vec![
            ActivityTags {
                activity_id: "".to_string(),
                tags: vec!["payment".to_string()],
            },
        ];

        assert!(db.upsert_tags(&activity_tags).is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_wipe_all() {
        let (mut db, db_path) = setup();

        // Insert various activities
        let activity1 = create_test_onchain_activity();
        let mut activity2 = create_test_lightning_activity();
        activity2.id = "test_lightning_2".to_string();
        let mut activity3 = create_test_onchain_activity();
        activity3.id = "test_onchain_3".to_string();
        let mut activity4 = create_test_lightning_activity();
        activity4.id = "test_lightning_4".to_string();

        db.insert_onchain_activity(&activity1).unwrap();
        db.insert_lightning_activity(&activity2).unwrap();
        db.insert_onchain_activity(&activity3).unwrap();
        db.insert_lightning_activity(&activity4).unwrap();

        // Add tags
        db.add_tags(&activity1.id, &["payment".to_string()]).unwrap();
        db.add_tags(&activity2.id, &["invoice".to_string()]).unwrap();
        db.add_tags(&activity3.id, &["transfer".to_string()]).unwrap();
        db.add_tags(&activity4.id, &["payment".to_string(), "invoice".to_string()]).unwrap();

        // Insert closed channels
        let mut channel1 = create_test_closed_channel();
        channel1.channel_id = "channel1".to_string();
        let mut channel2 = create_test_closed_channel();
        channel2.channel_id = "channel2".to_string();
        db.upsert_closed_channel(&channel1).unwrap();
        db.upsert_closed_channel(&channel2).unwrap();

        // Verify data exists
        let activities = db.get_activities(None, None, None, None, None, None, None, None).unwrap();
        assert_eq!(activities.len(), 4);
        let tags = db.get_all_unique_tags().unwrap();
        assert_eq!(tags.len(), 3);
        let channels = db.get_all_closed_channels(None).unwrap();
        assert_eq!(channels.len(), 2);

        // Wipe all data
        db.wipe_all().unwrap();

        // Verify everything is deleted
        let activities_after = db.get_activities(None, None, None, None, None, None, None, None).unwrap();
        assert_eq!(activities_after.len(), 0);
        let tags_after = db.get_all_unique_tags().unwrap();
        assert_eq!(tags_after.len(), 0);
        let channels_after = db.get_all_closed_channels(None).unwrap();
        assert_eq!(channels_after.len(), 0);

        // Verify we can still insert new data after wipe
        let new_activity = create_test_onchain_activity();
        db.insert_onchain_activity(&new_activity).unwrap();
        let activities_new = db.get_activities(None, None, None, None, None, None, None, None).unwrap();
        assert_eq!(activities_new.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_closed_channel() {
        let (mut db, db_path) = setup();
        let channel = create_test_closed_channel();
        
        // Insert closed channel
        assert!(db.upsert_closed_channel(&channel).is_ok());

        // Retrieve by ID
        let retrieved = db.get_closed_channel_by_id(&channel.channel_id).unwrap();
        assert!(retrieved.is_some());
        let retrieved_channel = retrieved.unwrap();
        
        assert_eq!(retrieved_channel.channel_id, channel.channel_id);
        assert_eq!(retrieved_channel.counterparty_node_id, channel.counterparty_node_id);
        assert_eq!(retrieved_channel.funding_txo_txid, channel.funding_txo_txid);
        assert_eq!(retrieved_channel.funding_txo_index, channel.funding_txo_index);
        assert_eq!(retrieved_channel.channel_value_sats, channel.channel_value_sats);
        assert_eq!(retrieved_channel.closed_at, channel.closed_at);
        assert_eq!(retrieved_channel.outbound_capacity_msat, channel.outbound_capacity_msat);
        assert_eq!(retrieved_channel.inbound_capacity_msat, channel.inbound_capacity_msat);
        assert_eq!(retrieved_channel.counterparty_unspendable_punishment_reserve, channel.counterparty_unspendable_punishment_reserve);
        assert_eq!(retrieved_channel.unspendable_punishment_reserve, channel.unspendable_punishment_reserve);
        assert_eq!(retrieved_channel.forwarding_fee_proportional_millionths, channel.forwarding_fee_proportional_millionths);
        assert_eq!(retrieved_channel.forwarding_fee_base_msat, channel.forwarding_fee_base_msat);
        assert_eq!(retrieved_channel.channel_name, channel.channel_name);
        assert_eq!(retrieved_channel.channel_closure_reason, channel.channel_closure_reason);

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_closed_channels() {
        let (mut db, db_path) = setup();

        // Insert multiple closed channels with different closed_at timestamps
        let mut channel1 = create_test_closed_channel();
        channel1.channel_id = "channel1".to_string();
        channel1.closed_at = 1000;

        let mut channel2 = create_test_closed_channel();
        channel2.channel_id = "channel2".to_string();
        channel2.closed_at = 2000;

        let mut channel3 = create_test_closed_channel();
        channel3.channel_id = "channel3".to_string();
        channel3.closed_at = 1500;

        db.upsert_closed_channel(&channel1).unwrap();
        db.upsert_closed_channel(&channel2).unwrap();
        db.upsert_closed_channel(&channel3).unwrap();

        // Get all channels, default sort (descending - most recent first)
        let all_channels = db.get_all_closed_channels(None).unwrap();
        assert_eq!(all_channels.len(), 3);
        assert_eq!(all_channels[0].channel_id, "channel2"); // Most recent (2000)
        assert_eq!(all_channels[1].channel_id, "channel3"); // Middle (1500)
        assert_eq!(all_channels[2].channel_id, "channel1"); // Oldest (1000)

        // Get all channels, ascending sort
        let all_channels_asc = db.get_all_closed_channels(Some(SortDirection::Asc)).unwrap();
        assert_eq!(all_channels_asc.len(), 3);
        assert_eq!(all_channels_asc[0].channel_id, "channel1"); // Oldest first
        assert_eq!(all_channels_asc[1].channel_id, "channel3");
        assert_eq!(all_channels_asc[2].channel_id, "channel2"); // Most recent last

        cleanup(&db_path);
    }

    #[test]
    fn test_get_closed_channel_not_found() {
        let (db, db_path) = setup();

        let result = db.get_closed_channel_by_id("nonexistent_channel").unwrap();
        assert!(result.is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_closed_channel_empty_id() {
        let (mut db, db_path) = setup();
        let mut channel = create_test_closed_channel();
        channel.channel_id = "".to_string();

        let result = db.upsert_closed_channel(&channel);
        assert!(result.is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_remove_closed_channel_by_id() {
        let (mut db, db_path) = setup();
        let channel = create_test_closed_channel();
        
        db.upsert_closed_channel(&channel).unwrap();
        
        // Verify it exists
        let retrieved = db.get_closed_channel_by_id(&channel.channel_id).unwrap();
        assert!(retrieved.is_some());

        // Delete it
        let deleted = db.remove_closed_channel_by_id(&channel.channel_id).unwrap();
        assert!(deleted);

        // Verify it's gone
        let retrieved_after = db.get_closed_channel_by_id(&channel.channel_id).unwrap();
        assert!(retrieved_after.is_none());

        // Try to delete again (should return false)
        let deleted_again = db.remove_closed_channel_by_id(&channel.channel_id).unwrap();
        assert!(!deleted_again);

        cleanup(&db_path);
    }

    #[test]
    fn test_wipe_all_closed_channels() {
        let (mut db, db_path) = setup();

        // Insert multiple closed channels
        let mut channel1 = create_test_closed_channel();
        channel1.channel_id = "channel1".to_string();
        let mut channel2 = create_test_closed_channel();
        channel2.channel_id = "channel2".to_string();
        let mut channel3 = create_test_closed_channel();
        channel3.channel_id = "channel3".to_string();

        db.upsert_closed_channel(&channel1).unwrap();
        db.upsert_closed_channel(&channel2).unwrap();
        db.upsert_closed_channel(&channel3).unwrap();

        // Verify they exist
        let all_channels = db.get_all_closed_channels(None).unwrap();
        assert_eq!(all_channels.len(), 3);

        // Wipe all closed channels
        db.wipe_all_closed_channels().unwrap();

        // Verify they're all gone
        let all_channels_after = db.get_all_closed_channels(None).unwrap();
        assert_eq!(all_channels_after.len(), 0);

        // Verify we can still insert new channels after wipe
        let new_channel = create_test_closed_channel();
        db.upsert_closed_channel(&new_channel).unwrap();
        let all_channels_new = db.get_all_closed_channels(None).unwrap();
        assert_eq!(all_channels_new.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_closed_channels() {
        let (mut db, db_path) = setup();

        // Create multiple closed channels
        let mut channels: Vec<ClosedChannelDetails> = Vec::new();
        for i in 1..=5 {
            let mut c = create_test_closed_channel();
            c.channel_id = format!("bulk_channel_{}", i);
            c.closed_at = 1_000 + i as u64;
            c.channel_value_sats = 1_000_000 * i as u64;
            channels.push(c);
        }

        // Bulk insert
        assert!(db.upsert_closed_channels(&channels).is_ok());

        // Verify all inserted
        let all = db.get_all_closed_channels(None).unwrap();
        assert_eq!(all.len(), 5);
        for i in 1..=5 {
            let id = format!("bulk_channel_{}", i);
            let ch = all.iter().find(|c| c.channel_id == id).expect("missing channel");
            assert_eq!(ch.channel_value_sats, 1_000_000 * i as u64);
        }

        // Modify a few and bulk update
        let mut updated = channels.clone();
        updated[0].channel_value_sats = 9_999_999;
        updated[1].channel_name = "Updated Name".to_string();
        updated[2].forwarding_fee_base_msat = 777;
        assert!(db.upsert_closed_channels(&updated).is_ok());

        // Verify updates applied
        let after = db.get_all_closed_channels(None).unwrap();
        let c1 = after.iter().find(|c| c.channel_id == "bulk_channel_1").unwrap();
        assert_eq!(c1.channel_value_sats, 9_999_999);
        let c2 = after.iter().find(|c| c.channel_id == "bulk_channel_2").unwrap();
        assert_eq!(c2.channel_name, "Updated Name");
        let c3 = after.iter().find(|c| c.channel_id == "bulk_channel_3").unwrap();
        assert_eq!(c3.forwarding_fee_base_msat, 777);

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_closed_channels_empty() {
        let (mut db, db_path) = setup();
        assert!(db.upsert_closed_channels(&[]).is_ok());
        let all = db.get_all_closed_channels(None).unwrap();
        assert_eq!(all.len(), 0);
        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_onchain_activities() {
        let (mut db, db_path) = setup();

        let mut acts: Vec<OnchainActivity> = Vec::new();
        for i in 0..5 {
            let mut a = create_test_onchain_activity();
            a.id = format!("onchain_bulk_{}", i);
            a.timestamp = 1_000 + i as u64;
            a.value = 10_000 + i as u64;
            a.address = format!("bc1q_addr_{}", i);
            acts.push(a);
        }

        assert!(db.upsert_onchain_activities(&acts).is_ok());

        let all = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, None, None).unwrap();
        assert_eq!(all.len(), 5);

        let mut updated = acts.clone();
        updated[0].value = 999_999;
        updated[1].fee = 42;
        updated[2].fee_rate = 7;
        updated[3].is_boosted = true;
        assert!(db.upsert_onchain_activities(&updated).is_ok());

        let after = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, None, None).unwrap();
        let map: std::collections::HashMap<String, OnchainActivity> = after.into_iter().map(|a| match a {
            Activity::Onchain(o) => (o.id.clone(), o),
            _ => unreachable!(),
        }).collect();
        assert_eq!(map["onchain_bulk_0"].value, 999_999);
        assert_eq!(map["onchain_bulk_1"].fee, 42);
        assert_eq!(map["onchain_bulk_2"].fee_rate, 7);
        assert!(map["onchain_bulk_3"].is_boosted);

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_onchain_activities_empty() {
        let (mut db, db_path) = setup();
        assert!(db.upsert_onchain_activities(&[]).is_ok());
        let all = db.get_activities(Some(ActivityFilter::Onchain), None, None, None, None, None, None, None).unwrap();
        assert!(all.is_empty());
        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_lightning_activities() {
        let (mut db, db_path) = setup();

        let mut acts: Vec<LightningActivity> = Vec::new();
        for i in 0..5 {
            let mut a = create_test_lightning_activity();
            a.id = format!("lightning_bulk_{}", i);
            a.timestamp = 2_000 + i as u64;
            a.value = 1_000 + i as u64;
            a.message = format!("msg_{}", i);
            acts.push(a);
        }

        assert!(db.upsert_lightning_activities(&acts).is_ok());

        let all = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, None, None).unwrap();
        assert_eq!(all.len(), 5);

        let mut updated = acts.clone();
        updated[0].value = 55;
        updated[1].status = PaymentState::Failed;
        updated[2].fee = Some(0);
        updated[3].message = "updated".to_string();
        assert!(db.upsert_lightning_activities(&updated).is_ok());

        let after = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, None, None).unwrap();
        let map: std::collections::HashMap<String, LightningActivity> = after.into_iter().map(|a| match a {
            Activity::Lightning(l) => (l.id.clone(), l),
            _ => unreachable!(),
        }).collect();
        assert_eq!(map["lightning_bulk_0"].value, 55);
        assert_eq!(map["lightning_bulk_1"].status, PaymentState::Failed);
        assert_eq!(map["lightning_bulk_2"].fee, Some(0));
        assert_eq!(map["lightning_bulk_3"].message, "updated");

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_lightning_activities_empty() {
        let (mut db, db_path) = setup();
        assert!(db.upsert_lightning_activities(&[]).is_ok());
        let all = db.get_activities(Some(ActivityFilter::Lightning), None, None, None, None, None, None, None).unwrap();
        assert!(all.is_empty());
        cleanup(&db_path);
    }

    // ========== Pre-Activity Metadata Tests ==========

    #[test]
    fn test_add_pre_activity_metadata_onchain() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string(), "coffee".to_string()];

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"payment".to_string()));
        assert!(activity_tags.contains(&"coffee".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_lightning() {
        let (mut db, db_path) = setup();
        let payment_hash = "test_lightning_1".to_string();
        let tags = vec!["invoice".to_string(), "payment".to_string()];

        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(payment_hash.clone(), ActivityType::Lightning, tags.clone())).is_ok());

        // Verify tags are transferred when activity is received
        let mut activity = create_test_lightning_activity();
        activity.id = payment_hash.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_lightning_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"invoice".to_string()));
        assert!(activity_tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_empty_identifier() {
        let (mut db, db_path) = setup();
        let tags = vec!["payment".to_string()];

        let result = db.add_pre_activity_metadata(&create_test_pre_activity_metadata("".to_string(), ActivityType::Onchain, tags));
        assert!(result.is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_duplicate() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata1 = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata2.address = Some(address.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 1);
        assert!(activity_tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_replaces_by_address() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let payment_id1 = "payment_id_1".to_string();
        let payment_id2 = "payment_id_2".to_string();

        // Add metadata with payment_id1 and address
        let mut metadata1 = create_test_pre_activity_metadata(payment_id1.clone(), ActivityType::Onchain, vec!["tag1".to_string()]);
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());

        // Verify it exists
        let result1 = db.get_pre_activity_metadata(&payment_id1, false).unwrap();
        assert!(result1.is_some());
        let result_by_address1 = db.get_pre_activity_metadata(&address, true).unwrap();
        assert!(result_by_address1.is_some());
        assert_eq!(result_by_address1.unwrap().payment_id, payment_id1);

        // Add metadata with payment_id2 and same address (should replace metadata1)
        let mut metadata2 = create_test_pre_activity_metadata(payment_id2.clone(), ActivityType::Onchain, vec!["tag2".to_string()]);
        metadata2.address = Some(address.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Verify metadata1 is gone
        let result1_after = db.get_pre_activity_metadata(&payment_id1, false).unwrap();
        assert!(result1_after.is_none());

        // Verify metadata2 exists and can be found by address
        let result2 = db.get_pre_activity_metadata(&payment_id2, false).unwrap();
        assert!(result2.is_some());
        let result_by_address2 = db.get_pre_activity_metadata(&address, true).unwrap();
        assert!(result_by_address2.is_some());
        let metadata2_retrieved = result_by_address2.unwrap();
        assert_eq!(metadata2_retrieved.payment_id, payment_id2);
        assert_eq!(metadata2_retrieved.tags, vec!["tag2".to_string()]);

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_multiple() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        let mut metadata1 = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, vec!["tag1".to_string()]);
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, vec!["tag2".to_string(), "tag3".to_string()]);
        metadata2.address = Some(address.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"tag2".to_string()));
        assert!(activity_tags.contains(&"tag3".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_tags() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Add initial metadata with one tag
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, vec!["tag1".to_string()])).is_ok());

        // Add more tags to existing metadata
        assert!(db.add_pre_activity_metadata_tags(&address, &["tag2".to_string(), "tag3".to_string()]).is_ok());

        // Verify all tags are present
        let all_metadata = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_metadata.len(), 1);
        let metadata = &all_metadata[0];
        assert_eq!(metadata.tags.len(), 3);
        assert!(metadata.tags.contains(&"tag1".to_string()));
        assert!(metadata.tags.contains(&"tag2".to_string()));
        assert!(metadata.tags.contains(&"tag3".to_string()));

        // Add duplicate tag (should not add duplicate)
        assert!(db.add_pre_activity_metadata_tags(&address, &["tag2".to_string()]).is_ok());

        // Verify no duplicate was added
        let all_metadata_after = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_metadata_after.len(), 1);
        let metadata_after = &all_metadata_after[0];
        assert_eq!(metadata_after.tags.len(), 3);
        assert_eq!(metadata_after.tags.iter().filter(|t| *t == "tag2").count(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_tags_nonexistent() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Try to add tags to non-existent metadata (should error)
        let result = db.add_pre_activity_metadata_tags(&address, &["tag1".to_string()]);
        assert!(result.is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_remove_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()];

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        assert!(db.remove_pre_activity_metadata_tags(&address, &["tag2".to_string()]).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"tag1".to_string()));
        assert!(activity_tags.contains(&"tag3".to_string()));
        assert!(!activity_tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_remove_pre_activity_metadata_multiple() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string(), "tag4".to_string()];

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        assert!(db.remove_pre_activity_metadata_tags(&address, &["tag1".to_string(), "tag3".to_string()]).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"tag2".to_string()));
        assert!(activity_tags.contains(&"tag4".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_remove_pre_activity_metadata_nonexistent() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Try to remove tags that don't exist (should not error)
        assert!(db.remove_pre_activity_metadata_tags(&address, &["nonexistent".to_string()]).is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_reset_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()];

        // Add tags
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone())).is_ok());

        // Reset all tags
        assert!(db.reset_pre_activity_metadata_tags(&address).is_ok());

        // Verify no tags are transferred
        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_reset_pre_activity_metadata_empty() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Reset tags that don't exist (should not error)
        assert!(db.reset_pre_activity_metadata_tags(&address).is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()];

        // Add tags
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone())).is_ok());

        // Verify metadata exists
        let all_metadata = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_metadata.len(), 1);

        // Delete all metadata
        assert!(db.delete_pre_activity_metadata(&address).is_ok());

        // Verify metadata is deleted
        let all_metadata_after = db.get_all_pre_activity_metadata().unwrap();
        assert!(all_metadata_after.is_empty());

        // Verify no tags are transferred after deletion
        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transferred_on_received() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut received_activity = create_test_onchain_activity();
        received_activity.id = "received_activity".to_string();
        received_activity.address = address.clone();
        received_activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&received_activity).unwrap();

        let received_tags = db.get_tags(&received_activity.id).unwrap();
        assert_eq!(received_tags.len(), 1);
        assert!(received_tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transferred_on_sent_onchain() {
        let (mut db, db_path) = setup();
        let tx_id = "txid123".to_string();
        let tags = vec!["sent_payment".to_string()];

        let mut metadata = create_test_pre_activity_metadata(tx_id.clone(), ActivityType::Onchain, tags.clone());
        metadata.tx_id = Some(tx_id.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut sent_activity = create_test_onchain_activity();
        sent_activity.tx_id = tx_id.clone();
        sent_activity.tx_type = PaymentType::Sent;
        db.insert_onchain_activity(&sent_activity).unwrap();

        let sent_tags = db.get_tags(&sent_activity.id).unwrap();
        assert_eq!(sent_tags.len(), 1);
        assert!(sent_tags.contains(&"sent_payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_address_update_on_sent() {
        let (mut db, db_path) = setup();
        let tx_id = "txid123".to_string();
        let metadata_address = "bc1qmetadata456".to_string();
        let tags = vec!["sent_payment".to_string()];

        let mut metadata = create_test_pre_activity_metadata(tx_id.clone(), ActivityType::Onchain, tags.clone());
        metadata.tx_id = Some(tx_id.clone());
        metadata.address = Some(metadata_address.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut sent_activity = create_test_onchain_activity();
        sent_activity.tx_id = tx_id.clone();
        sent_activity.address = "bc1qoriginal789".to_string();
        sent_activity.tx_type = PaymentType::Sent;
        db.insert_onchain_activity(&sent_activity).unwrap();

        let retrieved = db.get_activity_by_id(&sent_activity.id).unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.address, metadata_address);
        } else {
            panic!("Expected Onchain activity");
        }

        let sent_tags = db.get_tags(&sent_activity.id).unwrap();
        assert_eq!(sent_tags.len(), 1);
        assert!(sent_tags.contains(&"sent_payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transferred_on_lightning_sent() {
        let (mut db, db_path) = setup();
        let payment_hash = "test_lightning_sent_1".to_string();
        let tags = vec!["sent_invoice".to_string()];

        // Add pre-activity metadata using payment hash
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(payment_hash.clone(), ActivityType::Lightning, tags.clone())).is_ok());

        // Insert sent lightning activity (should transfer tags based on payment hash)
        let mut sent_activity = create_test_lightning_activity();
        sent_activity.id = payment_hash.clone();
        sent_activity.tx_type = PaymentType::Sent;
        db.insert_lightning_activity(&sent_activity).unwrap();

        let sent_tags = db.get_tags(&sent_activity.id).unwrap();
        assert_eq!(sent_tags.len(), 1);
        assert!(sent_tags.contains(&"sent_invoice".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_deleted_after_transfer() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string()];

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity1 = create_test_onchain_activity();
        activity1.address = address.clone();
        activity1.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity1).unwrap();

        let tags1 = db.get_tags(&activity1.id).unwrap();
        assert_eq!(tags1.len(), 2);

        let mut activity2 = create_test_onchain_activity();
        activity2.id = "activity2".to_string();
        activity2.address = address.clone();
        activity2.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity2).unwrap();

        let tags2 = db.get_tags(&activity2.id).unwrap();
        assert!(tags2.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_lightning_received() {
        let (mut db, db_path) = setup();
        let payment_hash = "test_lightning_received_1".to_string();
        let tags = vec!["invoice".to_string(), "payment".to_string()];

        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(payment_hash.clone(), ActivityType::Lightning, tags.clone())).is_ok());

        let mut activity = create_test_lightning_activity();
        activity.id = payment_hash.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_lightning_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"invoice".to_string()));
        assert!(activity_tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_onchain_received_with_ln_payment_hash() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let ln_payment_hash = "ln_payment_hash_abc123".to_string();
        let tags = vec!["payment".to_string(), "coffee".to_string()];

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.id = ln_payment_hash.clone();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"payment".to_string()));
        assert!(activity_tags.contains(&"coffee".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_multiple_identifiers() {
        let (mut db, db_path) = setup();
        let address1 = "bc1qtest123".to_string();
        let address2 = "bc1qtest456".to_string();

        let mut metadata1 = create_test_pre_activity_metadata(address1.clone(), ActivityType::Onchain, vec!["tag1".to_string()]);
        metadata1.address = Some(address1.clone());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(address2.clone(), ActivityType::Onchain, vec!["tag2".to_string()]);
        metadata2.address = Some(address2.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Insert activities for both addresses
        let mut activity1 = create_test_onchain_activity();
        activity1.address = address1.clone();
        activity1.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity1).unwrap();

        let mut activity2 = create_test_onchain_activity();
        activity2.id = "activity2".to_string();
        activity2.address = address2.clone();
        activity2.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity2).unwrap();

        // Verify each activity got its own tags
        let tags1 = db.get_tags(&activity1.id).unwrap();
        assert_eq!(tags1.len(), 1);
        assert!(tags1.contains(&"tag1".to_string()));

        let tags2 = db.get_tags(&activity2.id).unwrap();
        assert_eq!(tags2.len(), 1);
        assert!(tags2.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_onchain_and_lightning_separate() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let payment_hash = "test_lightning_separate_1".to_string();

        let mut metadata1 = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, vec!["onchain_tag".to_string()]);
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        let metadata2 = create_test_pre_activity_metadata(payment_hash.clone(), ActivityType::Lightning, vec!["lightning_tag".to_string()]);
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Insert onchain received activity
        let mut onchain_activity = create_test_onchain_activity();
        onchain_activity.address = address.clone();
        onchain_activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&onchain_activity).unwrap();

        // Insert lightning received activity
        let mut lightning_activity = create_test_lightning_activity();
        lightning_activity.id = payment_hash.clone();
        lightning_activity.tx_type = PaymentType::Received;
        db.insert_lightning_activity(&lightning_activity).unwrap();

        // Verify each got its own tags
        let onchain_tags = db.get_tags(&onchain_activity.id).unwrap();
        assert_eq!(onchain_tags.len(), 1);
        assert!(onchain_tags.contains(&"onchain_tag".to_string()));

        let lightning_tags = db.get_tags(&lightning_activity.id).unwrap();
        assert_eq!(lightning_tags.len(), 1);
        assert!(lightning_tags.contains(&"lightning_tag".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_empty_tags() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Add empty tags (should be allowed, but won't transfer anything meaningful)
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, vec![])).is_ok());

        // Insert received activity
        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        // Verify no tags were transferred
        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_combined_with_regular_tags() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        let mut metadata = create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, vec!["receiving_tag".to_string()]);
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        // Add regular tags to the same activity
        db.add_tags(&activity.id, &["regular_tag".to_string()]).unwrap();

        // Verify both types of tags are present
        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"receiving_tag".to_string()));
        assert!(activity_tags.contains(&"regular_tag".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_get_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string()];

        // Get non-existent metadata (should return None)
        let result = db.get_pre_activity_metadata(&address, false).unwrap();
        assert!(result.is_none());

        // Add pre-activity metadata
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone())).is_ok());

        // Get existing metadata
        let metadata = db.get_pre_activity_metadata(&address, false).unwrap();
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.payment_id, address);
        assert_eq!(metadata.tags.len(), 2);
        assert!(metadata.tags.contains(&"tag1".to_string()));
        assert!(metadata.tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_get_pre_activity_metadata_by_address() {
        let (mut db, db_path) = setup();
        let payment_id = "payment_id_123".to_string();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string()];

        // Add pre-activity metadata with address
        let mut metadata = create_test_pre_activity_metadata(payment_id.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        // Test searching by payment_id
        let result_by_payment_id = db.get_pre_activity_metadata(&payment_id, false).unwrap();
        assert!(result_by_payment_id.is_some());

        // Test searching by address
        let result_by_address = db.get_pre_activity_metadata(&address, true).unwrap();
        assert!(result_by_address.is_some());
        let metadata_by_address = result_by_address.unwrap();
        assert_eq!(metadata_by_address.payment_id, payment_id);
        assert_eq!(metadata_by_address.tags.len(), 2);
        assert!(metadata_by_address.tags.contains(&"tag1".to_string()));
        assert!(metadata_by_address.tags.contains(&"tag2".to_string()));

        // Test that searching by address with wrong search type returns None
        let result_wrong_search = db.get_pre_activity_metadata(&address, false).unwrap();
        assert!(result_wrong_search.is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address1 = "bc1qtest123".to_string();
        let address2 = "bc1qtest456".to_string();
        let invoice = "lightning:invoice123".to_string();

        // Add pre-activity metadata for multiple identifiers
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address1.clone(), ActivityType::Onchain, vec!["tag1".to_string(), "tag2".to_string()])).is_ok());
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(address2.clone(), ActivityType::Onchain, vec!["tag3".to_string()])).is_ok());
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata(invoice.clone(), ActivityType::Lightning, vec!["tag4".to_string(), "tag5".to_string()])).is_ok());

        // Get all pre-activity metadata
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 3);

        // Find tags for address1
        let addr1_tags = all_tags.iter().find(|rt| rt.payment_id == address1).unwrap();
        assert_eq!(addr1_tags.tags.len(), 2);
        assert!(addr1_tags.tags.contains(&"tag1".to_string()));
        assert!(addr1_tags.tags.contains(&"tag2".to_string()));

        // Find tags for address2
        let addr2_tags = all_tags.iter().find(|rt| rt.payment_id == address2).unwrap();
        assert_eq!(addr2_tags.tags.len(), 1);
        assert!(addr2_tags.tags.contains(&"tag3".to_string()));

        // Find tags for invoice
        let invoice_tags = all_tags.iter().find(|rt| rt.payment_id == invoice).unwrap();
        assert_eq!(invoice_tags.tags.len(), 2);
        assert!(invoice_tags.tags.contains(&"tag4".to_string()));
        assert!(invoice_tags.tags.contains(&"tag5".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_pre_activity_metadata_empty() {
        let (db, db_path) = setup();

        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert!(all_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata() {
        let (mut db, db_path) = setup();

        // Create pre-activity metadata for backup/restore
        let pre_activity_metadata = vec![
            PreActivityMetadata {
                payment_id: "bc1qtest123".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                payment_hash: None,
                tx_id: None,
                address: Some("bc1qtest123".to_string()),
                is_receive: true,
                created_at: 0,
            },
            PreActivityMetadata {
                payment_id: "bc1qtest456".to_string(),
                tags: vec!["tag3".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
            PreActivityMetadata {
                payment_id: "lightning:invoice123".to_string(),
                tags: vec!["tag4".to_string(), "tag5".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
        ];

        // Upsert pre-activity metadata
        assert!(db.upsert_pre_activity_metadata(&pre_activity_metadata).is_ok());

        // Verify tags were added
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 3);

        // Verify tags can be transferred
        let mut activity = create_test_onchain_activity();
        activity.address = "bc1qtest123".to_string();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"tag1".to_string()));
        assert!(activity_tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_idempotent() {
        let (mut db, db_path) = setup();

        let pre_activity_metadata = vec![
            PreActivityMetadata {
                payment_id: "bc1qtest123".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
        ];

        // Upsert twice (should be idempotent)
        assert!(db.upsert_pre_activity_metadata(&pre_activity_metadata).is_ok());
        assert!(db.upsert_pre_activity_metadata(&pre_activity_metadata).is_ok());

        // Verify tags are still there
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 1);
        let tags = &all_tags[0];
        assert_eq!(tags.tags.len(), 2);
        assert!(tags.tags.contains(&"tag1".to_string()));
        assert!(tags.tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_updates_existing() {
        let (mut db, db_path) = setup();

        let mut initial_metadata = create_test_pre_activity_metadata("bc1qtest123".to_string(), ActivityType::Onchain, vec!["tag1".to_string()]);
        initial_metadata.address = Some("bc1qtest123".to_string());
        initial_metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&initial_metadata).is_ok());

        let pre_activity_metadata = vec![
            PreActivityMetadata {
                payment_id: "bc1qtest123".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()],
                payment_hash: None,
                tx_id: None,
                address: Some("bc1qtest123".to_string()),
                is_receive: true,
                created_at: 0,
            },
        ];

        assert!(db.upsert_pre_activity_metadata(&pre_activity_metadata).is_ok());

        // Verify all tags are present
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 1);
        let tags = &all_tags[0];
        assert_eq!(tags.tags.len(), 3);
        assert!(tags.tags.contains(&"tag1".to_string()));
        assert!(tags.tags.contains(&"tag2".to_string()));
        assert!(tags.tags.contains(&"tag3".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_empty() {
        let (mut db, db_path) = setup();

        // Upsert with empty vector (should not error)
        assert!(db.upsert_pre_activity_metadata(&[]).is_ok());

        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert!(all_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_empty_identifier() {
        let (mut db, db_path) = setup();

        let pre_activity_metadata = vec![
            PreActivityMetadata {
                payment_id: "".to_string(),
                tags: vec!["tag1".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
        ];

        // Empty identifier is allowed for backup/restore (restores exactly what was backed up)
        assert!(db.upsert_pre_activity_metadata(&pre_activity_metadata).is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_backup_restore_pre_activity_metadata() {
        let (mut db, db_path) = setup();

        let mut metadata1 = create_test_pre_activity_metadata("bc1qtest123".to_string(), ActivityType::Onchain, vec!["tag1".to_string(), "tag2".to_string()]);
        metadata1.address = Some("bc1qtest123".to_string());
        let metadata2 = create_test_pre_activity_metadata("lightning:invoice123".to_string(), ActivityType::Lightning, vec!["tag3".to_string()]);
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Backup: Get all pre-activity metadata
        let backup = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(backup.len(), 2);

        // Simulate restore: Delete and restore
        assert!(db.delete_pre_activity_metadata(&"bc1qtest123".to_string()).is_ok());
        assert!(db.delete_pre_activity_metadata(&"lightning:invoice123".to_string()).is_ok());

        // Verify cleared
        let after_clear = db.get_all_pre_activity_metadata().unwrap();
        assert!(after_clear.is_empty());

        // Restore from backup
        assert!(db.upsert_pre_activity_metadata(&backup).is_ok());

        // Verify restored
        let restored = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(restored.len(), 2);

        // Verify tags work after restore
        let mut activity = create_test_onchain_activity();
        activity.address = "bc1qtest123".to_string();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(&activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"tag1".to_string()));
        assert!(activity_tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_same_identifier() {
        let (mut db, db_path) = setup();

        // Same identifier string (second one replaces first)
        let pre_activity_metadata = vec![
            PreActivityMetadata {
                payment_id: "same_id".to_string(),
                tags: vec!["tag1".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
            PreActivityMetadata {
                payment_id: "same_id".to_string(),
                tags: vec!["tag2".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
        ];

        assert!(db.upsert_pre_activity_metadata(&pre_activity_metadata).is_ok());

        // Verify only the last one is stored (second replaces first)
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 1);

        let metadata = &all_tags[0];
        assert_eq!(metadata.tags.len(), 1);
        assert!(metadata.tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_pre_activity_metadata_ordering() {
        let (mut db, db_path) = setup();

        // Add tags in non-alphabetical order
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata("z_address".to_string(), ActivityType::Onchain, vec!["tag1".to_string()])).is_ok());
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata("a_address".to_string(), ActivityType::Onchain, vec!["tag2".to_string()])).is_ok());
        assert!(db.add_pre_activity_metadata(&create_test_pre_activity_metadata("m_address".to_string(), ActivityType::Onchain, vec!["tag3".to_string()])).is_ok());

        // Get all tags - should be sorted by payment_id
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 3);
        assert_eq!(all_tags[0].payment_id, "a_address");
        assert_eq!(all_tags[1].payment_id, "m_address");
        assert_eq!(all_tags[2].payment_id, "z_address");

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_partial_update() {
        let (mut db, db_path) = setup();

        let mut metadata1 = create_test_pre_activity_metadata("address1".to_string(), ActivityType::Onchain, vec!["tag1".to_string()]);
        metadata1.address = Some("address1".to_string());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata("address2".to_string(), ActivityType::Onchain, vec!["tag2".to_string()]);
        metadata2.address = Some("address2".to_string());
        metadata2.is_receive = true;
        let mut metadata3 = create_test_pre_activity_metadata("address3".to_string(), ActivityType::Onchain, vec!["tag3".to_string()]);
        metadata3.address = Some("address3".to_string());
        metadata3.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata3).is_ok());

        // Get all
        let all = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all.len(), 3);

        // Upsert with new tags for address2 (replaces existing tags)
        let updated = vec![
            PreActivityMetadata {
                payment_id: "address2".to_string(),
                tags: vec!["tag2_updated".to_string(), "tag2_new".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                created_at: 0,
            },
        ];

        assert!(db.upsert_pre_activity_metadata(&updated).is_ok());

        // Verify address1 and address3 unchanged, address2 has replaced tags
        let after = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(after.len(), 3);

        let addr1 = after.iter().find(|rt| rt.payment_id == "address1").unwrap();
        assert_eq!(addr1.tags, vec!["tag1".to_string()]);

        let addr2 = after.iter().find(|rt| rt.payment_id == "address2").unwrap();
        // address2 now has only the new tags (replaced, not merged)
        assert_eq!(addr2.tags.len(), 2);
        assert!(addr2.tags.contains(&"tag2_updated".to_string()));
        assert!(addr2.tags.contains(&"tag2_new".to_string()));
        assert!(!addr2.tags.contains(&"tag2".to_string()));

        let addr3 = after.iter().find(|rt| rt.payment_id == "address3").unwrap();
        assert_eq!(addr3.tags, vec!["tag3".to_string()]);

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_pre_activity_metadata_after_transfer() {
        let (mut db, db_path) = setup();

        let mut metadata1 = create_test_pre_activity_metadata("bc1qtest123".to_string(), ActivityType::Onchain, vec!["tag1".to_string(), "tag2".to_string()]);
        metadata1.address = Some("bc1qtest123".to_string());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata("bc1qtest456".to_string(), ActivityType::Onchain, vec!["tag3".to_string()]);
        metadata2.address = Some("bc1qtest456".to_string());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Get all before transfer
        let before = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(before.len(), 2);

        // Transfer tags for one address
        let mut activity = create_test_onchain_activity();
        activity.address = "bc1qtest123".to_string();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        // Get all after transfer - should only have the untransferred one
        let after = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].payment_id, "bc1qtest456");
        assert_eq!(after[0].tags, vec!["tag3".to_string()]);

        cleanup(&db_path);
    }
}
