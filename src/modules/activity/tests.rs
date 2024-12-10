#[cfg(test)]
mod tests {
    use crate::activity::{
        ActivityDB, OnchainActivity, LightningActivity, ActivityType,
        PaymentType, PaymentState, Activity,
    };
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
            activity_type: ActivityType::Onchain,
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
            activity_type: ActivityType::Lightning,
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

        let activities = db.get_all_onchain_activities(None).unwrap();
        assert_eq!(activities.len(), 1);
        let retrieved = &activities[0];
        assert_eq!(retrieved.id, activity.id);
        assert_eq!(retrieved.value, activity.value);
        assert_eq!(retrieved.fee, activity.fee);
        assert!(retrieved.created_at.is_some());
        assert!(retrieved.updated_at.is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_lightning_activity() {
        let (mut db, db_path) = setup();
        let activity = create_test_lightning_activity();
        assert!(db.insert_lightning_activity(&activity).is_ok());

        let activities = db.get_all_lightning_activities(None).unwrap();
        assert_eq!(activities.len(), 1);
        let retrieved = &activities[0];
        assert_eq!(retrieved.id, activity.id);
        assert_eq!(retrieved.value, activity.value);
        assert_eq!(retrieved.message, activity.message);
        assert!(retrieved.created_at.is_some());
        assert!(retrieved.updated_at.is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_activities() {
        let (mut db, db_path) = setup();
        let onchain = create_test_onchain_activity();
        let lightning = create_test_lightning_activity();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        let all_activities = db.get_all_activities(None).unwrap();
        assert_eq!(all_activities.len(), 2);

        // Check ordering by timestamp descending (they have the same timestamp in this test)
        // The order should not matter if they have identical timestamps, but both should appear.
        assert!(all_activities.iter().any(|a| a.get_id() == onchain.id));
        assert!(all_activities.iter().any(|a| a.get_id() == lightning.id));

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activity_by_id() {
        let (mut db, db_path) = setup();
        let onchain = create_test_onchain_activity();
        db.insert_onchain_activity(&onchain).unwrap();

        let result = db.get_activity_by_id(&onchain.id).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            Activity::Onchain(a) => {
                assert_eq!(a.id, onchain.id);
                assert_eq!(a.value, onchain.value);
            },
            _ => panic!("Expected Onchain activity"),
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_update_onchain_activity() {
        let (mut db, db_path) = setup();
        let mut activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        activity.value = 100000;
        db.update_onchain_activity_by_id(&activity.id, &activity).unwrap();

        let updated = db.get_activity_by_id(&activity.id).unwrap().unwrap();
        if let Activity::Onchain(a) = updated {
            assert_eq!(a.value, 100000);
        } else {
            panic!("Wrong activity type returned");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_update_lightning_activity() {
        let (mut db, db_path) = setup();
        let mut activity = create_test_lightning_activity();
        db.insert_lightning_activity(&activity).unwrap();

        activity.value = 20000;
        db.update_lightning_activity_by_id(&activity.id, &activity).unwrap();

        let updated = db.get_activity_by_id(&activity.id).unwrap().unwrap();
        match updated {
            Activity::Lightning(a) => assert_eq!(a.value, 20000),
            _ => panic!("Wrong activity type returned"),
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_activity() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        assert!(db.delete_activity_by_id(&activity.id).unwrap());
        assert!(db.get_activity_by_id(&activity.id).unwrap().is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_invalid_operations() {
        let (mut db, db_path) = setup();
        // Non-existent activity
        assert!(db.get_activity_by_id("non_existent").unwrap().is_none());

        // Updating non-existent activity
        let activity = create_test_onchain_activity();
        assert!(db.update_onchain_activity_by_id("non_existent", &activity).is_err());

        // Deleting non-existent activity
        assert!(!db.delete_activity_by_id("non_existent").unwrap());

        cleanup(&db_path);
    }

    #[test]
    fn test_activity_timestamps() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db.get_all_onchain_activities(None).unwrap();
        assert!(retrieved[0].created_at.is_some());
        assert!(retrieved[0].updated_at.is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_constraint_violations() {
        let (mut db, db_path) = setup();

        // Empty invoice on lightning activity
        let mut lightning = create_test_lightning_activity();
        lightning.invoice = "".to_string();
        assert!(db.insert_lightning_activity(&lightning).is_err());

        // Negative fee on onchain activity
        let mut onchain = create_test_onchain_activity();
        onchain.fee = -1;
        assert!(db.insert_onchain_activity(&onchain).is_err());

        // Negative fee_rate
        onchain.fee = 500;
        onchain.fee_rate = -1;
        assert!(db.insert_onchain_activity(&onchain).is_err());

        // Empty address
        onchain.fee_rate = 1;
        onchain.address = "".to_string();
        assert!(db.insert_onchain_activity(&onchain).is_err());

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

        let all_activities = db.get_all_activities(None).unwrap();
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

        let activities = db.get_all_activities(None).unwrap();
        let timestamps: Vec<i64> = activities.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(timestamps, vec![2000, 1500, 1000]);

        cleanup(&db_path);
    }

    #[test]
    fn test_invalid_data() {
        let (mut db, db_path) = setup();

        let mut activity = create_test_onchain_activity();
        activity.id = "".to_string();
        assert!(db.insert_onchain_activity(&activity).is_err());

        let activity1 = create_test_onchain_activity();
        db.insert_onchain_activity(&activity1).unwrap();
        // Insert duplicate ID
        assert!(db.insert_onchain_activity(&activity1).is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_limits_on_all_activities() {
        let (mut db, db_path) = setup();

        // Insert multiple activities
        for i in 0..5 {
            let mut onchain = create_test_onchain_activity();
            onchain.id = format!("test_onchain_{}", i);
            onchain.timestamp = 1234567890 + i as i64;
            db.insert_onchain_activity(&onchain).unwrap();

            let mut lightning = create_test_lightning_activity();
            lightning.id = format!("test_lightning_{}", i);
            lightning.timestamp = 1234567890 + i as i64;
            db.insert_lightning_activity(&lightning).unwrap();
        }

        let activities = db.get_all_activities(Some(3)).unwrap();
        assert_eq!(activities.len(), 3);

        let activities = db.get_all_activities(Some(7)).unwrap();
        assert_eq!(activities.len(), 7);

        let activities = db.get_all_activities(Some(20)).unwrap();
        assert_eq!(activities.len(), 10);

        let activities = db.get_all_activities(None).unwrap();
        assert_eq!(activities.len(), 10);

        cleanup(&db_path);
    }

    #[test]
    fn test_limits_on_onchain_activities() {
        let (mut db, db_path) = setup();
        for i in 0..5 {
            let mut activity = create_test_onchain_activity();
            activity.id = format!("test_onchain_{}", i);
            activity.timestamp = 1234567890 + i as i64;
            db.insert_onchain_activity(&activity).unwrap();
        }

        let activities = db.get_all_onchain_activities(Some(2)).unwrap();
        assert_eq!(activities.len(), 2);

        let activities = db.get_all_onchain_activities(Some(10)).unwrap();
        assert_eq!(activities.len(), 5);

        let activities = db.get_all_onchain_activities(None).unwrap();
        assert_eq!(activities.len(), 5);

        cleanup(&db_path);
    }

    #[test]
    fn test_limits_on_lightning_activities() {
        let (mut db, db_path) = setup();
        for i in 0..5 {
            let mut activity = create_test_lightning_activity();
            activity.id = format!("test_lightning_{}", i);
            activity.timestamp = 1234567890 + i as i64;
            db.insert_lightning_activity(&activity).unwrap();
        }

        let activities = db.get_all_lightning_activities(Some(2)).unwrap();
        assert_eq!(activities.len(), 2);

        let activities = db.get_all_lightning_activities(Some(10)).unwrap();
        assert_eq!(activities.len(), 5);

        let activities = db.get_all_lightning_activities(None).unwrap();
        assert_eq!(activities.len(), 5);

        cleanup(&db_path);
    }

    #[test]
    fn test_zero_limit() {
        let (mut db, db_path) = setup();
        db.insert_onchain_activity(&create_test_onchain_activity()).unwrap();
        db.insert_lightning_activity(&create_test_lightning_activity()).unwrap();

        let activities = db.get_all_activities(Some(0)).unwrap();
        assert_eq!(activities.len(), 0);

        let onchain = db.get_all_onchain_activities(Some(0)).unwrap();
        assert_eq!(onchain.len(), 0);

        let lightning = db.get_all_lightning_activities(Some(0)).unwrap();
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

        let activities = db.get_activities_by_tag("payment", None).unwrap();
        assert_eq!(activities.len(), 2);

        let limited = db.get_activities_by_tag("payment", Some(1)).unwrap();
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
        let activities = db.get_activities_by_tag("nonexistent", None).unwrap();
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
}
