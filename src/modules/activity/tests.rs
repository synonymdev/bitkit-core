#[cfg(test)]
mod tests {
    use crate::activity::{ActivityDB, OnchainActivity, LightningActivity, PaymentType, PaymentState, Activity, ActivityFilter, SortDirection};
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
        let mut activity1 = create_test_onchain_activity();
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
}
