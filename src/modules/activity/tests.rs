#[cfg(test)]
mod tests {
    use crate::activity::{
        Activity, ActivityDB, ActivityFilter, ActivityTags, ActivityType, ClosedChannelDetails,
        LightningActivity, OnchainActivity, PaymentState, PaymentType, PreActivityMetadata,
        SortDirection, TransactionDetails, TxInput, TxOutput, DEFAULT_WALLET_ID,
    };
    use rand::random;
    use std::fs;

    fn setup() -> (ActivityDB, String) {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        let db = ActivityDB::new(&db_path).unwrap();
        (db, db_path)
    }

    fn cleanup(db_path: &str) {
        fs::remove_file(db_path).ok();
    }

    fn primary_key_columns(db: &ActivityDB, table: &str) -> Vec<String> {
        let mut stmt = db
            .conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .unwrap();
        let mut columns = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(5)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        columns.retain(|(pk_index, _)| *pk_index > 0);
        columns.sort_by_key(|(pk_index, _)| *pk_index);
        columns.into_iter().map(|(_, column)| column).collect()
    }

    fn create_test_onchain_activity() -> OnchainActivity {
        OnchainActivity {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
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
            contact: None,
            created_at: None,
            updated_at: None,
            seen_at: None,
        }
    }

    fn create_test_lightning_activity() -> LightningActivity {
        LightningActivity {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            id: "test_lightning_1".to_string(),
            tx_type: PaymentType::Received,
            status: PaymentState::Succeeded,
            value: 10000,
            fee: Some(1),
            invoice: "lightning:abc".to_string(),
            message: "Test payment".to_string(),
            timestamp: 1234567890,
            preimage: Some("preimage123".to_string()),
            contact: None,
            created_at: None,
            updated_at: None,
            seen_at: None,
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

    fn create_test_pre_activity_metadata(
        payment_id: String,
        _payment_type: ActivityType,
        tags: Vec<String>,
    ) -> PreActivityMetadata {
        PreActivityMetadata {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            payment_id,
            tags,
            payment_hash: None,
            tx_id: None,
            address: None,
            is_receive: false,
            fee_rate: 0,
            is_transfer: false,
            channel_id: None,
            created_at: 0,
        }
    }

    #[test]
    fn test_db_initialization() {
        let (db, db_path) = setup();
        assert!(
            db.conn.is_autocommit(),
            "Database should be in autocommit mode"
        );
        cleanup(&db_path);
    }

    #[test]
    fn test_activity_migrations_add_contact_column() {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "
                CREATE TABLE activities (
                    id TEXT PRIMARY KEY,
                    activity_type TEXT NOT NULL CHECK (activity_type IN ('onchain', 'lightning')),
                    tx_type TEXT NOT NULL CHECK (tx_type IN ('sent', 'received')),
                    timestamp INTEGER NOT NULL CHECK (timestamp > 0),
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                )",
                [],
            )
            .unwrap();
        }

        let db = ActivityDB::new(&db_path).unwrap();
        let mut stmt = db.conn.prepare("PRAGMA table_info(activities)").unwrap();
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(columns.contains(&"seen_at".to_string()));
        assert!(columns.contains(&"contact".to_string()));
        assert!(columns.contains(&"wallet_id".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_activity_migration_rebuilds_wallet_primary_keys() {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE activities (
                    id TEXT PRIMARY KEY,
                    wallet_id TEXT NOT NULL DEFAULT 'bitkit' CHECK (length(wallet_id) > 0),
                    activity_type TEXT NOT NULL CHECK (activity_type IN ('onchain', 'lightning')),
                    tx_type TEXT NOT NULL CHECK (tx_type IN ('sent', 'received')),
                    timestamp INTEGER NOT NULL CHECK (timestamp > 0),
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    seen_at INTEGER CHECK (seen_at IS NULL OR seen_at > 0),
                    contact TEXT CHECK (contact IS NULL OR length(contact) > 0)
                );
                CREATE TABLE onchain_activity (
                    id TEXT PRIMARY KEY,
                    tx_id TEXT NOT NULL,
                    address TEXT NOT NULL CHECK (length(address) > 0),
                    confirmed BOOLEAN NOT NULL,
                    value INTEGER NOT NULL CHECK (value >= 0),
                    fee INTEGER NOT NULL CHECK (fee >= 0),
                    fee_rate INTEGER NOT NULL CHECK (fee_rate >= 0),
                    is_boosted BOOLEAN NOT NULL,
                    boost_tx_ids TEXT NOT NULL,
                    is_transfer BOOLEAN NOT NULL,
                    does_exist BOOLEAN NOT NULL,
                    confirm_timestamp INTEGER CHECK (
                        confirm_timestamp IS NULL OR confirm_timestamp >= 0
                    ),
                    channel_id TEXT CHECK (
                        channel_id IS NULL OR length(channel_id) > 0
                    ),
                    transfer_tx_id TEXT CHECK (
                        transfer_tx_id IS NULL OR length(transfer_tx_id) > 0
                    )
                );
                CREATE TABLE lightning_activity (
                    id TEXT PRIMARY KEY,
                    invoice TEXT NOT NULL CHECK (length(invoice) > 0),
                    value INTEGER NOT NULL CHECK (value >= 0),
                    status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed')),
                    fee INTEGER CHECK (fee IS NULL OR fee >= 0),
                    message TEXT NOT NULL,
                    preimage TEXT CHECK (
                        preimage IS NULL OR length(preimage) > 0
                    )
                );
                CREATE TABLE activity_tags (
                    activity_id TEXT NOT NULL,
                    tag TEXT NOT NULL,
                    PRIMARY KEY (activity_id, tag)
                );
                INSERT INTO activities (
                    id, wallet_id, activity_type, tx_type, timestamp, created_at,
                    updated_at, seen_at, contact
                )
                VALUES (
                    'legacy_activity', 'hardware-wallet-1', 'onchain', 'sent',
                    1234567890, 1234567890, 1234567891, 1234567892, 'contact_pubky'
                );
                INSERT INTO onchain_activity (
                    id, tx_id, address, confirmed, value, fee, fee_rate, is_boosted,
                    boost_tx_ids, is_transfer, does_exist, confirm_timestamp,
                    channel_id, transfer_tx_id
                )
                VALUES (
                    'legacy_activity', 'legacy_txid', 'bc1qlegacy', 1, 5000, 50, 1,
                    0, '', 0, 1, 1234567893, NULL, NULL
                );
                INSERT INTO activity_tags (activity_id, tag)
                VALUES ('legacy_activity', 'legacy_tag');
                ",
            )
            .unwrap();
        }

        let db = ActivityDB::new(&db_path).unwrap();
        assert_eq!(
            primary_key_columns(&db, "activities"),
            vec!["wallet_id", "id"]
        );
        assert_eq!(
            primary_key_columns(&db, "onchain_activity"),
            vec!["wallet_id", "id"]
        );
        assert_eq!(
            primary_key_columns(&db, "lightning_activity"),
            vec!["wallet_id", "id"]
        );
        assert_eq!(
            primary_key_columns(&db, "activity_tags"),
            vec!["wallet_id", "activity_id", "tag"]
        );

        let activity = db
            .get_activity_by_id("hardware-wallet-1", "legacy_activity")
            .unwrap()
            .unwrap();
        match activity {
            Activity::Onchain(activity) => {
                assert_eq!(activity.wallet_id, "hardware-wallet-1");
                assert_eq!(activity.tx_id, "legacy_txid");
                assert_eq!(activity.contact, Some("contact_pubky".to_string()));
                assert_eq!(activity.seen_at, Some(1234567892));
            }
            Activity::Lightning(_) => panic!("Expected onchain activity"),
        }
        assert_eq!(
            db.get_tags("hardware-wallet-1", "legacy_activity").unwrap(),
            vec!["legacy_tag".to_string()]
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_activity_migration_defaults_pre_wallet_rows_to_bitkit() {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE activities (
                    id TEXT PRIMARY KEY,
                    activity_type TEXT NOT NULL CHECK (activity_type IN ('onchain', 'lightning')),
                    tx_type TEXT NOT NULL CHECK (tx_type IN ('sent', 'received')),
                    timestamp INTEGER NOT NULL CHECK (timestamp > 0),
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                );
                CREATE TABLE onchain_activity (
                    id TEXT PRIMARY KEY,
                    tx_id TEXT NOT NULL,
                    address TEXT NOT NULL CHECK (length(address) > 0),
                    confirmed BOOLEAN NOT NULL,
                    value INTEGER NOT NULL CHECK (value >= 0),
                    fee INTEGER NOT NULL CHECK (fee >= 0),
                    fee_rate INTEGER NOT NULL CHECK (fee_rate >= 0),
                    is_boosted BOOLEAN NOT NULL,
                    boost_tx_ids TEXT NOT NULL,
                    is_transfer BOOLEAN NOT NULL,
                    does_exist BOOLEAN NOT NULL,
                    confirm_timestamp INTEGER CHECK (
                        confirm_timestamp IS NULL OR confirm_timestamp >= 0
                    ),
                    channel_id TEXT CHECK (
                        channel_id IS NULL OR length(channel_id) > 0
                    ),
                    transfer_tx_id TEXT CHECK (
                        transfer_tx_id IS NULL OR length(transfer_tx_id) > 0
                    )
                );
                CREATE TABLE lightning_activity (
                    id TEXT PRIMARY KEY,
                    invoice TEXT NOT NULL CHECK (length(invoice) > 0),
                    value INTEGER NOT NULL CHECK (value >= 0),
                    status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed')),
                    fee INTEGER CHECK (fee IS NULL OR fee >= 0),
                    message TEXT NOT NULL,
                    preimage TEXT CHECK (
                        preimage IS NULL OR length(preimage) > 0
                    )
                );
                CREATE TABLE activity_tags (
                    activity_id TEXT NOT NULL,
                    tag TEXT NOT NULL,
                    PRIMARY KEY (activity_id, tag)
                );
                INSERT INTO activities (
                    id, activity_type, tx_type, timestamp, created_at, updated_at
                )
                VALUES
                    ('legacy_onchain', 'onchain', 'sent', 1234567890, 1234567890, 1234567891),
                    ('legacy_lightning', 'lightning', 'received', 1234567990, 1234567990, 1234567991);
                INSERT INTO onchain_activity (
                    id, tx_id, address, confirmed, value, fee, fee_rate, is_boosted,
                    boost_tx_ids, is_transfer, does_exist, confirm_timestamp,
                    channel_id, transfer_tx_id
                )
                VALUES (
                    'legacy_onchain', 'legacy_onchain_txid', 'bc1qlegacy', 1, 5000, 50, 1,
                    0, '', 0, 1, 1234567893, NULL, NULL
                );
                INSERT INTO lightning_activity (
                    id, invoice, value, status, fee, message, preimage
                )
                VALUES (
                    'legacy_lightning', 'lightning:legacy', 7000, 'succeeded',
                    3, 'legacy message', 'legacy_preimage'
                );
                INSERT INTO activity_tags (activity_id, tag)
                VALUES
                    ('legacy_onchain', 'legacy_onchain_tag'),
                    ('legacy_lightning', 'legacy_lightning_tag');
                ",
            )
            .unwrap();
        }

        let db = ActivityDB::new(&db_path).unwrap();
        assert_eq!(
            primary_key_columns(&db, "activities"),
            vec!["wallet_id", "id"]
        );
        assert_eq!(
            primary_key_columns(&db, "onchain_activity"),
            vec!["wallet_id", "id"]
        );
        assert_eq!(
            primary_key_columns(&db, "lightning_activity"),
            vec!["wallet_id", "id"]
        );
        assert_eq!(
            primary_key_columns(&db, "activity_tags"),
            vec!["wallet_id", "activity_id", "tag"]
        );

        let onchain = db
            .get_activity_by_id(DEFAULT_WALLET_ID, "legacy_onchain")
            .unwrap()
            .unwrap();
        match onchain {
            Activity::Onchain(activity) => {
                assert_eq!(activity.wallet_id, DEFAULT_WALLET_ID);
                assert_eq!(activity.tx_id, "legacy_onchain_txid");
                assert_eq!(activity.seen_at, None);
                assert_eq!(activity.contact, None);
            }
            Activity::Lightning(_) => panic!("Expected onchain activity"),
        }

        let lightning = db
            .get_activity_by_id(DEFAULT_WALLET_ID, "legacy_lightning")
            .unwrap()
            .unwrap();
        match lightning {
            Activity::Lightning(activity) => {
                assert_eq!(activity.wallet_id, DEFAULT_WALLET_ID);
                assert_eq!(activity.invoice, "lightning:legacy");
                assert_eq!(activity.message, "legacy message");
            }
            Activity::Onchain(_) => panic!("Expected lightning activity"),
        }

        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, "legacy_onchain").unwrap(),
            vec!["legacy_onchain_tag".to_string()]
        );
        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, "legacy_lightning").unwrap(),
            vec!["legacy_lightning_tag".to_string()]
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_migration_defaults_rows_to_bitkit() {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "
                CREATE TABLE pre_activity_metadata (
                    payment_id TEXT PRIMARY KEY,
                    tags TEXT NOT NULL,
                    payment_hash TEXT,
                    tx_id TEXT,
                    address TEXT,
                    is_receive BOOLEAN NOT NULL DEFAULT FALSE,
                    fee_rate INTEGER NOT NULL DEFAULT 0,
                    is_transfer BOOLEAN NOT NULL DEFAULT FALSE,
                    channel_id TEXT,
                    created_at INTEGER NOT NULL DEFAULT 0
                )
                ",
                [],
            )
            .unwrap();
            conn.execute(
                "
                INSERT INTO pre_activity_metadata (
                    payment_id, tags, payment_hash, tx_id, address,
                    is_receive, fee_rate, is_transfer, channel_id, created_at
                ) VALUES (?1, ?2, NULL, NULL, ?3, 1, 0, 0, NULL, 1234)
                ",
                rusqlite::params!["legacy_payment", "[\"legacy\"]", "bc1qlegacy"],
            )
            .unwrap();
        }

        let db = ActivityDB::new(&db_path).unwrap();

        assert_eq!(
            primary_key_columns(&db, "pre_activity_metadata"),
            vec!["wallet_id".to_string(), "payment_id".to_string()]
        );

        let metadata = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "legacy_payment", false)
            .unwrap()
            .unwrap();
        assert_eq!(metadata.wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(metadata.payment_id, "legacy_payment");
        assert_eq!(metadata.tags, vec!["legacy".to_string()]);

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_migration_handles_older_schema() {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "
                CREATE TABLE pre_activity_metadata (
                    payment_id TEXT PRIMARY KEY,
                    payment_type TEXT NOT NULL,
                    tags TEXT NOT NULL,
                    payment_hash TEXT,
                    tx_id TEXT,
                    address TEXT,
                    is_receive BOOLEAN NOT NULL DEFAULT FALSE,
                    created_at INTEGER NOT NULL DEFAULT 0
                )
                ",
                [],
            )
            .unwrap();
            conn.execute(
                "
                INSERT INTO pre_activity_metadata (
                    payment_id, payment_type, tags, payment_hash, tx_id,
                    address, is_receive, created_at
                ) VALUES (?1, 'onchain', ?2, NULL, NULL, ?3, 1, 1234)
                ",
                rusqlite::params!["older_payment", "[\"older\"]", "bc1qolder"],
            )
            .unwrap();
        }

        let db = ActivityDB::new(&db_path).unwrap();

        assert_eq!(
            primary_key_columns(&db, "pre_activity_metadata"),
            vec!["wallet_id".to_string(), "payment_id".to_string()]
        );

        let metadata = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "older_payment", false)
            .unwrap()
            .unwrap();
        assert_eq!(metadata.wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(metadata.payment_id, "older_payment");
        assert_eq!(metadata.tags, vec!["older".to_string()]);
        assert_eq!(metadata.address, Some("bc1qolder".to_string()));
        assert!(metadata.is_receive);
        assert_eq!(metadata.fee_rate, 0);
        assert!(!metadata.is_transfer);
        assert_eq!(metadata.channel_id, None);

        cleanup(&db_path);
    }

    #[test]
    fn test_transaction_details_migration_rebuilds_wallet_primary_key() {
        let db_path = format!("test_db_{}.sqlite", random::<u64>());
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE transaction_details (
                    tx_id TEXT PRIMARY KEY,
                    amount_sats INTEGER NOT NULL,
                    inputs TEXT NOT NULL,
                    outputs TEXT NOT NULL
                );
                INSERT INTO transaction_details (tx_id, amount_sats, inputs, outputs)
                VALUES ('legacy_txid', 1234, '[]', '[]');
                ",
            )
            .unwrap();
        }

        let db = ActivityDB::new(&db_path).unwrap();
        let primary_keys = db
            .conn
            .prepare("PRAGMA table_info(transaction_details)")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(primary_keys
            .iter()
            .any(|(column, pk)| column == "wallet_id" && *pk > 0));
        assert!(primary_keys
            .iter()
            .any(|(column, pk)| column == "tx_id" && *pk > 0));

        let details = db
            .get_transaction_details(DEFAULT_WALLET_ID, "legacy_txid")
            .unwrap()
            .unwrap();
        assert_eq!(details.wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(details.amount_sats, 1234);

        cleanup(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_onchain_activity() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        assert!(db.insert_onchain_activity(&activity).is_ok());

        let activities = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(activities.len(), 1);
        if let Activity::Onchain(retrieved) = &activities[0] {
            assert_eq!(retrieved.wallet_id.as_str(), DEFAULT_WALLET_ID);
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
    fn test_get_activities_optional_wallet_id_filters_and_sorts() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";

        let mut main = create_test_onchain_activity();
        main.id = "main_tx".to_string();
        main.tx_id = "main_txid".to_string();
        main.timestamp = 100;

        let mut hw_newer = create_test_onchain_activity();
        hw_newer.wallet_id = wallet_id.to_string();
        hw_newer.id = "hardware-wallet-1:tx_newer".to_string();
        hw_newer.tx_id = "tx_newer".to_string();
        hw_newer.timestamp = 300;

        let mut hw_older = create_test_onchain_activity();
        hw_older.wallet_id = wallet_id.to_string();
        hw_older.id = "hardware-wallet-1:tx_older".to_string();
        hw_older.tx_id = "tx_older".to_string();
        hw_older.timestamp = 200;

        db.upsert_onchain_activities(&[main.clone(), hw_newer.clone(), hw_older.clone()])
            .unwrap();

        let wallet_activities = db
            .get_activities(
                Some(wallet_id),
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(SortDirection::Desc),
            )
            .unwrap();

        assert_eq!(wallet_activities.len(), 2);
        assert_eq!(wallet_activities[0].get_id(), hw_newer.id);
        assert_eq!(wallet_activities[1].get_id(), hw_older.id);

        let unified = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                Some(2),
                Some(SortDirection::Desc),
            )
            .unwrap();

        assert_eq!(unified.len(), 2);
        assert_eq!(unified[0].get_id(), hw_newer.id);
        assert_eq!(unified[1].get_id(), hw_older.id);

        cleanup(&db_path);
    }

    #[test]
    fn test_wallet_scoped_txid_lookup_allows_collisions() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";

        let mut main = create_test_onchain_activity();
        main.id = "bitkit:shared_txid".to_string();
        main.tx_id = "shared_txid".to_string();
        main.value = 10_000;

        let mut hardware = create_test_onchain_activity();
        hardware.wallet_id = wallet_id.to_string();
        hardware.id = "hardware-wallet-1:shared_txid".to_string();
        hardware.tx_id = "shared_txid".to_string();
        hardware.value = 20_000;

        db.upsert_onchain_activities(&[main.clone(), hardware.clone()])
            .unwrap();

        let default_lookup = db
            .get_activity_by_tx_id(DEFAULT_WALLET_ID, "shared_txid")
            .unwrap()
            .unwrap();
        assert_eq!(default_lookup.wallet_id.as_str(), DEFAULT_WALLET_ID);
        assert_eq!(default_lookup.value, main.value);

        let scoped_lookup = db
            .get_activity_by_tx_id(wallet_id, "shared_txid")
            .unwrap()
            .unwrap();
        assert_eq!(scoped_lookup.wallet_id.as_str(), wallet_id);
        assert_eq!(scoped_lookup.value, hardware.value);

        cleanup(&db_path);
    }

    #[test]
    fn test_wallet_scoped_app_shaped_activity_collision() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";
        let shared_id = "shared_activity_txid";
        let shared_tag = "shared_tag";

        let mut main = create_test_onchain_activity();
        main.id = shared_id.to_string();
        main.tx_id = shared_id.to_string();
        main.value = 10_000;

        let mut hardware = create_test_onchain_activity();
        hardware.wallet_id = wallet_id.to_string();
        hardware.id = shared_id.to_string();
        hardware.tx_id = shared_id.to_string();
        hardware.value = 20_000;

        db.insert_onchain_activity(&main).unwrap();
        db.insert_onchain_activity(&hardware).unwrap();
        db.add_tags(DEFAULT_WALLET_ID, shared_id, &[shared_tag.to_string()])
            .unwrap();
        db.add_tags(wallet_id, shared_id, &[shared_tag.to_string()])
            .unwrap();

        let default_lookup = db
            .get_activity_by_tx_id(DEFAULT_WALLET_ID, shared_id)
            .unwrap()
            .unwrap();
        assert_eq!(default_lookup.wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(default_lookup.value, main.value);

        let hardware_lookup = db
            .get_activity_by_tx_id(wallet_id, shared_id)
            .unwrap()
            .unwrap();
        assert_eq!(hardware_lookup.wallet_id, wallet_id);
        assert_eq!(hardware_lookup.value, hardware.value);

        let all_tagged = db
            .get_activities_by_tag(None, shared_tag, None, None)
            .unwrap();
        assert_eq!(all_tagged.len(), 2);
        assert!(all_tagged
            .iter()
            .any(|activity| activity.get_wallet_id() == DEFAULT_WALLET_ID));
        assert!(all_tagged
            .iter()
            .any(|activity| activity.get_wallet_id() == wallet_id));

        let default_tagged = db
            .get_activities_by_tag(Some(DEFAULT_WALLET_ID), shared_tag, None, None)
            .unwrap();
        assert_eq!(default_tagged.len(), 1);
        assert_eq!(default_tagged[0].get_wallet_id(), DEFAULT_WALLET_ID);

        let hardware_tagged = db
            .get_activities_by_tag(Some(wallet_id), shared_tag, None, None)
            .unwrap();
        assert_eq!(hardware_tagged.len(), 1);
        assert_eq!(hardware_tagged[0].get_wallet_id(), wallet_id);

        cleanup(&db_path);
    }

    #[test]
    fn test_wallet_scoped_activity_ids_allow_collisions() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";
        let activity_id = "shared_activity_id";

        let mut main = create_test_onchain_activity();
        main.id = activity_id.to_string();
        main.tx_id = "default_wallet_txid".to_string();
        main.value = 10_000;

        let mut hardware = create_test_onchain_activity();
        hardware.wallet_id = wallet_id.to_string();
        hardware.id = activity_id.to_string();
        hardware.tx_id = "hardware_wallet_txid".to_string();
        hardware.value = 20_000;

        db.insert_onchain_activity(&main).unwrap();
        db.insert_onchain_activity(&hardware).unwrap();

        let default_activity = db
            .get_activity_by_id(DEFAULT_WALLET_ID, activity_id)
            .unwrap()
            .unwrap();
        match default_activity {
            Activity::Onchain(activity) => {
                assert_eq!(activity.wallet_id, DEFAULT_WALLET_ID);
                assert_eq!(activity.value, main.value);
            }
            Activity::Lightning(_) => panic!("Expected onchain activity"),
        }

        let hardware_activity = db
            .get_activity_by_id(wallet_id, activity_id)
            .unwrap()
            .unwrap();
        match hardware_activity {
            Activity::Onchain(activity) => {
                assert_eq!(activity.wallet_id, wallet_id);
                assert_eq!(activity.value, hardware.value);
            }
            Activity::Lightning(_) => panic!("Expected onchain activity"),
        }

        db.add_tags(DEFAULT_WALLET_ID, activity_id, &["main".to_string()])
            .unwrap();
        db.add_tags(wallet_id, activity_id, &["hardware".to_string()])
            .unwrap();

        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, activity_id).unwrap(),
            vec!["main".to_string()]
        );
        assert_eq!(
            db.get_tags(wallet_id, activity_id).unwrap(),
            vec!["hardware".to_string()]
        );

        assert!(db.delete_activity_by_id(wallet_id, activity_id).unwrap());
        assert!(db
            .get_activity_by_id(DEFAULT_WALLET_ID, activity_id)
            .unwrap()
            .is_some());
        assert!(db
            .get_activity_by_id(wallet_id, activity_id)
            .unwrap()
            .is_none());

        cleanup(&db_path);
    }

    fn json_has_wallet_id<T: serde::Serialize>(value: &T) -> bool {
        serde_json::to_value(value)
            .unwrap()
            .get("wallet_id")
            .is_some()
    }

    #[test]
    fn test_default_wallet_activity_always_serializes_wallet_id() {
        // The current/canonical payload always includes `wallet_id`, even for the default
        // wallet. A freshly serialized default-wallet record is therefore distinguishable
        // from legacy data, and round-trips back to the default wallet.
        let onchain = create_test_onchain_activity();
        assert_eq!(onchain.wallet_id, DEFAULT_WALLET_ID);
        assert!(
            json_has_wallet_id(&onchain),
            "default-wallet onchain activity must serialize wallet_id (current payload)"
        );
        let onchain_json = serde_json::to_string(&onchain).unwrap();
        let decoded: OnchainActivity = serde_json::from_str(&onchain_json).unwrap();
        assert_eq!(decoded.wallet_id, DEFAULT_WALLET_ID);

        let lightning = create_test_lightning_activity();
        assert_eq!(lightning.wallet_id, DEFAULT_WALLET_ID);
        assert!(
            json_has_wallet_id(&lightning),
            "default-wallet lightning activity must serialize wallet_id (current payload)"
        );
        let lightning_json = serde_json::to_string(&lightning).unwrap();
        let decoded: LightningActivity = serde_json::from_str(&lightning_json).unwrap();
        assert_eq!(decoded.wallet_id, DEFAULT_WALLET_ID);
    }

    #[test]
    fn test_old_v1_activity_json_without_wallet_id_decodes() {
        // Old JSON authored before wallet_id existed (no wallet_id key) must still decode,
        // defaulting to the built-in Bitkit wallet.
        let onchain_v1 = r#"{
            "id": "legacy_onchain",
            "tx_type": "Sent",
            "tx_id": "legacy_txid",
            "value": 5000,
            "fee": 50,
            "fee_rate": 1,
            "address": "bc1qlegacy",
            "confirmed": true,
            "timestamp": 1234567890,
            "is_boosted": false,
            "boost_tx_ids": [],
            "is_transfer": false,
            "does_exist": true,
            "confirm_timestamp": null,
            "channel_id": null,
            "transfer_tx_id": null
        }"#;
        let decoded: OnchainActivity = serde_json::from_str(onchain_v1).unwrap();
        assert_eq!(decoded.wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(decoded.tx_id, "legacy_txid");

        let lightning_v1 = r#"{
            "id": "legacy_lightning",
            "tx_type": "Received",
            "status": "Succeeded",
            "value": 7000,
            "fee": 3,
            "invoice": "lightning:legacy",
            "message": "legacy message",
            "timestamp": 1234567990,
            "preimage": null
        }"#;
        let decoded: LightningActivity = serde_json::from_str(lightning_v1).unwrap();
        assert_eq!(decoded.wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(decoded.invoice, "lightning:legacy");
    }

    #[test]
    fn test_hardware_wallet_activity_serializes_wallet_id() {
        // Wallet-scoped (non-default) records keep wallet_id in the JSON and round-trip it
        // back unchanged.
        let wallet_id = crate::activity::derive_wallet_id(
            "trezor".to_string(),
            vec!["xpubA".to_string(), "xpubB".to_string()],
        )
        .unwrap();

        let mut onchain = create_test_onchain_activity();
        onchain.wallet_id = wallet_id.clone();
        assert!(json_has_wallet_id(&onchain));
        let decoded: OnchainActivity =
            serde_json::from_str(&serde_json::to_string(&onchain).unwrap()).unwrap();
        assert_eq!(decoded.wallet_id, wallet_id);

        let mut lightning = create_test_lightning_activity();
        lightning.wallet_id = wallet_id.clone();
        assert!(json_has_wallet_id(&lightning));
        let decoded: LightningActivity =
            serde_json::from_str(&serde_json::to_string(&lightning).unwrap()).unwrap();
        assert_eq!(decoded.wallet_id, wallet_id);
    }

    #[test]
    fn test_wallet_scoped_metadata_models_always_serialize_wallet_id() {
        // Tags, pre-activity metadata and transaction details gained wallet_id in the same
        // change; the current/canonical payload always carries wallet_id, for the default
        // wallet and for any other scope alike.
        let scoped = "hardware-wallet-1";

        let default_tags = ActivityTags {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            activity_id: "act1".to_string(),
            tags: vec!["tag".to_string()],
        };
        assert!(json_has_wallet_id(&default_tags));
        let scoped_tags = ActivityTags {
            wallet_id: scoped.to_string(),
            ..default_tags.clone()
        };
        assert!(json_has_wallet_id(&scoped_tags));

        let default_meta = create_test_pre_activity_metadata(
            "pay1".to_string(),
            ActivityType::Onchain,
            vec!["tag".to_string()],
        );
        assert!(json_has_wallet_id(&default_meta));
        let scoped_meta = PreActivityMetadata {
            wallet_id: scoped.to_string(),
            ..default_meta.clone()
        };
        assert!(json_has_wallet_id(&scoped_meta));

        let default_details = TransactionDetails {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            tx_id: "txid".to_string(),
            amount_sats: 1000,
            inputs: vec![],
            outputs: vec![],
        };
        assert!(json_has_wallet_id(&default_details));
        let scoped_details = TransactionDetails {
            wallet_id: scoped.to_string(),
            ..default_details.clone()
        };
        assert!(json_has_wallet_id(&scoped_details));
    }

    #[test]
    fn test_mixed_v1_v2_lookup_and_search_is_wallet_scoped() {
        // A v1 (default-wallet) and v2 (hardware-wallet) activity sharing the same raw id
        // must remain distinct: wallet-scoped lookup, list and search each return only the
        // matching record, never a mixed or duplicated v1/v2 pair.
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";
        let shared_id = "shared_raw_id";

        let mut v1 = create_test_onchain_activity();
        v1.id = shared_id.to_string();
        v1.tx_id = "v1_txid".to_string();
        v1.address = "bc1qv1default".to_string();
        v1.value = 10_000;

        let mut v2 = create_test_onchain_activity();
        v2.wallet_id = wallet_id.to_string();
        v2.id = shared_id.to_string();
        v2.tx_id = "v2_txid".to_string();
        v2.address = "bc1qv2hardware".to_string();
        v2.value = 20_000;

        db.insert_onchain_activity(&v1).unwrap();
        db.insert_onchain_activity(&v2).unwrap();

        let default_activity = db
            .get_activity_by_id(DEFAULT_WALLET_ID, shared_id)
            .unwrap()
            .unwrap();
        assert_eq!(default_activity.get_wallet_id(), DEFAULT_WALLET_ID);

        let scoped_activity = db
            .get_activity_by_id(wallet_id, shared_id)
            .unwrap()
            .unwrap();
        assert_eq!(scoped_activity.get_wallet_id(), wallet_id);

        // Wallet-scoped list returns exactly one record for each wallet, not the mixed pair.
        let default_list = db
            .get_activities(
                Some(DEFAULT_WALLET_ID),
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(default_list.len(), 1);
        assert_eq!(default_list[0].get_wallet_id(), DEFAULT_WALLET_ID);

        let scoped_list = db
            .get_activities(
                Some(wallet_id),
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(scoped_list.len(), 1);
        assert_eq!(scoped_list[0].get_wallet_id(), wallet_id);

        // Search stays wallet-scoped: the hardware address is invisible to the default wallet.
        let scoped_search = db
            .get_activities(
                Some(DEFAULT_WALLET_ID),
                None,
                None,
                None,
                Some("bc1qv2hardware".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(scoped_search.is_empty());

        // Positive side: each wallet's own address search returns exactly its own row.
        let hardware_search = db
            .get_activities(
                Some(wallet_id),
                None,
                None,
                None,
                Some("bc1qv2hardware".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(hardware_search.len(), 1);
        assert_eq!(hardware_search[0].get_wallet_id(), wallet_id);

        let default_search = db
            .get_activities(
                Some(DEFAULT_WALLET_ID),
                None,
                None,
                None,
                Some("bc1qv1default".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(default_search.len(), 1);
        assert_eq!(default_search[0].get_wallet_id(), DEFAULT_WALLET_ID);

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_activities_by_wallet_id_cleans_scoped_data() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";

        let mut main = create_test_onchain_activity();
        main.id = "bitkit:cleanup_txid".to_string();
        main.tx_id = "cleanup_txid".to_string();

        let mut hardware = create_test_onchain_activity();
        hardware.wallet_id = wallet_id.to_string();
        hardware.id = "hardware-wallet-1:cleanup_txid".to_string();
        hardware.tx_id = "cleanup_txid".to_string();

        db.upsert_onchain_activities(&[main.clone(), hardware.clone()])
            .unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &main.id, &["main".to_string()])
            .unwrap();
        db.add_tags(wallet_id, &hardware.id, &["hardware".to_string()])
            .unwrap();

        let mut main_details = create_test_transaction_details();
        main_details.tx_id = "cleanup_txid".to_string();
        main_details.amount_sats = 1;

        let mut hardware_details = create_test_transaction_details();
        hardware_details.wallet_id = wallet_id.to_string();
        hardware_details.tx_id = "cleanup_txid".to_string();
        hardware_details.amount_sats = 2;

        db.upsert_transaction_details(&[main_details, hardware_details])
            .unwrap();

        let mut main_metadata = create_test_pre_activity_metadata(
            "bitkit:cleanup_pending".to_string(),
            ActivityType::Onchain,
            vec!["main-pending".to_string()],
        );
        main_metadata.address = Some("bc1qbitkitcleanup".to_string());

        let mut hardware_metadata = create_test_pre_activity_metadata(
            "hardware-wallet-1:cleanup_pending".to_string(),
            ActivityType::Onchain,
            vec!["hardware-pending".to_string()],
        );
        hardware_metadata.wallet_id = wallet_id.to_string();
        hardware_metadata.address = Some("bc1qhardwarecleanup".to_string());

        db.add_pre_activity_metadata(&main_metadata).unwrap();
        db.add_pre_activity_metadata(&hardware_metadata).unwrap();

        let deleted = db.delete_activities_by_wallet_id(wallet_id).unwrap();
        assert_eq!(deleted, 1);

        assert!(db
            .get_activity_by_id(wallet_id, &hardware.id)
            .unwrap()
            .is_none());
        assert!(db
            .get_activity_by_id(DEFAULT_WALLET_ID, &main.id)
            .unwrap()
            .is_some());
        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, &main.id).unwrap(),
            vec!["main".to_string()]
        );
        assert!(db.get_tags(wallet_id, &hardware.id).unwrap().is_empty());

        let main_details = db
            .get_transaction_details(DEFAULT_WALLET_ID, "cleanup_txid")
            .unwrap()
            .unwrap();
        assert_eq!(main_details.amount_sats, 1);
        assert!(db
            .get_transaction_details(wallet_id, "cleanup_txid")
            .unwrap()
            .is_none());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &main_metadata.payment_id, false)
            .unwrap()
            .is_some());
        assert!(db
            .get_pre_activity_metadata(wallet_id, &hardware_metadata.payment_id, false)
            .unwrap()
            .is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_lightning_activity() {
        let (mut db, db_path) = setup();
        let activity = create_test_lightning_activity();
        assert!(db.insert_lightning_activity(&activity).is_ok());

        let activities = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
    fn test_contact_preserved_for_activity_variants() {
        let (mut db, db_path) = setup();
        let mut onchain = create_test_onchain_activity();
        let mut lightning = create_test_lightning_activity();

        onchain.contact = Some("onchain_contact_pubky".to_string());
        lightning.contact = Some("lightning_contact_pubky".to_string());

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        let onchain_by_id = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &onchain.id)
            .unwrap()
            .unwrap();
        match onchain_by_id {
            Activity::Onchain(activity) => {
                assert_eq!(activity.contact, Some("onchain_contact_pubky".to_string()));
            }
            Activity::Lightning(_) => panic!("Expected Onchain activity"),
        }

        let onchain_by_tx_id = db
            .get_activity_by_tx_id(DEFAULT_WALLET_ID, &onchain.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            onchain_by_tx_id.contact,
            Some("onchain_contact_pubky".to_string())
        );

        let activities = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert!(activities.iter().any(|activity| {
            matches!(activity, Activity::Onchain(a) if a.contact.as_deref() == Some("onchain_contact_pubky"))
        }));
        assert!(activities.iter().any(|activity| {
            matches!(activity, Activity::Lightning(a) if a.contact.as_deref() == Some("lightning_contact_pubky"))
        }));

        cleanup(&db_path);
    }

    #[test]
    fn test_contact_updates_and_searches() {
        let (mut db, db_path) = setup();
        let mut activity = create_test_lightning_activity();

        db.insert_lightning_activity(&activity).unwrap();
        activity.contact = Some("searchable_contact_pubky".to_string());
        db.update_lightning_activity_by_id(&activity.id, &activity)
            .unwrap();

        let results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("searchable_contact".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        match &results[0] {
            Activity::Lightning(retrieved) => {
                assert_eq!(
                    retrieved.contact,
                    Some("searchable_contact_pubky".to_string())
                );
            }
            Activity::Onchain(_) => panic!("Expected Lightning activity"),
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

        let all_activities = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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

        let retrieved = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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

        let all_activities = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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

        let activities = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                Some(3),
                None,
            )
            .unwrap();
        assert_eq!(all.len(), 3);

        let onchain = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                Some(2),
                None,
            )
            .unwrap();
        assert_eq!(onchain.len(), 2);

        let lightning = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                Some(4),
                None,
            )
            .unwrap();
        assert_eq!(lightning.len(), 4);

        // Test without limits
        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(all.len(), 10);

        cleanup(&db_path);
    }

    #[test]
    fn test_zero_limit() {
        let (mut db, db_path) = setup();
        db.insert_onchain_activity(&create_test_onchain_activity())
            .unwrap();
        db.insert_lightning_activity(&create_test_lightning_activity())
            .unwrap();

        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                Some(0),
                None,
            )
            .unwrap();
        assert_eq!(all.len(), 0);

        let onchain = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                Some(0),
                None,
            )
            .unwrap();
        assert_eq!(onchain.len(), 0);

        let lightning = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                Some(0),
                None,
            )
            .unwrap();
        assert_eq!(lightning.len(), 0);

        cleanup(&db_path);
    }

    #[test]
    fn test_tags_add_retrieve() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = vec!["payment".to_string(), "coffee".to_string()];
        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &tags).unwrap();
        let retrieved_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &tags).unwrap();

        db.remove_tags(
            DEFAULT_WALLET_ID,
            &activity.id,
            &vec!["payment".to_string()],
        )
        .unwrap();
        let remaining_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert_eq!(remaining_tags.len(), 1);
        assert_eq!(remaining_tags[0], "coffee");

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activities_by_tag() {
        let (mut db, db_path) = setup();
        let hardware_wallet_id = "hardware-wallet-1";
        let onchain = create_test_onchain_activity();
        let mut lightning = create_test_lightning_activity();
        lightning.id = "test_lightning_tagged".to_string();
        let mut hardware = create_test_onchain_activity();
        hardware.wallet_id = hardware_wallet_id.to_string();
        hardware.id = "hardware_tagged".to_string();
        hardware.tx_id = "hardware_tagged_txid".to_string();

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();
        db.insert_onchain_activity(&hardware).unwrap();

        db.add_tags(DEFAULT_WALLET_ID, &onchain.id, &["payment".to_string()])
            .unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &lightning.id, &["payment".to_string()])
            .unwrap();
        db.add_tags(hardware_wallet_id, &hardware.id, &["payment".to_string()])
            .unwrap();

        let activities = db
            .get_activities_by_tag(None, "payment", None, None)
            .unwrap();
        assert_eq!(activities.len(), 3);

        let default_wallet_activities = db
            .get_activities_by_tag(Some(DEFAULT_WALLET_ID), "payment", None, None)
            .unwrap();
        assert_eq!(default_wallet_activities.len(), 2);
        assert!(default_wallet_activities
            .iter()
            .all(|activity| activity.get_wallet_id() == DEFAULT_WALLET_ID));

        let hardware_activities = db
            .get_activities_by_tag(Some(hardware_wallet_id), "payment", None, None)
            .unwrap();
        assert_eq!(hardware_activities.len(), 1);
        assert_eq!(hardware_activities[0].get_wallet_id(), hardware_wallet_id);

        let limited = db
            .get_activities_by_tag(None, "payment", Some(1), None)
            .unwrap();
        assert_eq!(limited.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_tags_on_nonexistent_activity() {
        let (mut db, db_path) = setup();
        let tags = vec!["test".to_string()];
        assert!(db
            .add_tags(DEFAULT_WALLET_ID, "nonexistent", &tags)
            .is_err());
        cleanup(&db_path);
    }

    #[test]
    fn test_duplicate_tags() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = vec!["test".to_string(), "test".to_string()];
        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &tags).unwrap();

        let retrieved_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert_eq!(retrieved_tags.len(), 1);

        cleanup(&db_path);
    }

    #[test]
    fn test_get_tags_empty() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert!(tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_activity_removes_tags() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &["test".to_string()])
            .unwrap();
        db.delete_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();

        let tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert!(
            tags.is_empty(),
            "Tags should be removed after activity deletion"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activities_by_nonexistent_tag() {
        let (db, db_path) = setup();
        let activities = db
            .get_activities_by_tag(None, "nonexistent", None, None)
            .unwrap();
        assert!(activities.is_empty());
        cleanup(&db_path);
    }

    #[test]
    fn test_operations_after_deletion() {
        let (mut db, db_path) = setup();

        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();
        db.delete_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();

        // These operations should fail or return empty results after deletion
        assert!(db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .is_none());
        assert!(db
            .update_onchain_activity_by_id(&activity.id, &activity)
            .is_err());
        assert!(db
            .add_tags(DEFAULT_WALLET_ID, &activity.id, &["test".to_string()])
            .is_err());

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
        assert!(
            result.is_ok(),
            "Failed to insert activity: {:?}",
            result.err()
        );

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
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

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
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

        let activities = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
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
        assert!(db
            .update_onchain_activity_by_id(&activity.id, &activity)
            .is_ok());

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
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
    fn test_update_onchain_metadata_transfer_targets_activity_id_argument() {
        let (mut db, db_path) = setup();

        let activity = create_test_onchain_activity();
        let activity_id = activity.id.clone();
        db.insert_onchain_activity(&activity).unwrap();

        let mut metadata = create_test_pre_activity_metadata(
            "updated_txid".to_string(),
            ActivityType::Onchain,
            vec!["target-row".to_string()],
        );
        metadata.tx_id = Some("updated_txid".to_string());
        db.add_pre_activity_metadata(&metadata).unwrap();

        let mut updated = activity;
        updated.id = "mismatched_activity_id".to_string();
        updated.tx_id = "updated_txid".to_string();

        db.update_onchain_activity_by_id(&activity_id, &updated)
            .unwrap();

        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, &activity_id).unwrap(),
            vec!["target-row".to_string()]
        );
        assert!(db
            .get_tags(DEFAULT_WALLET_ID, "mismatched_activity_id")
            .unwrap()
            .is_empty());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "updated_txid", false)
            .unwrap()
            .is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_update_lightning_metadata_transfer_targets_activity_id_argument() {
        let (mut db, db_path) = setup();

        let activity = create_test_lightning_activity();
        let activity_id = activity.id.clone();
        db.insert_lightning_activity(&activity).unwrap();

        let metadata = create_test_pre_activity_metadata(
            "mismatched_lightning_id".to_string(),
            ActivityType::Lightning,
            vec!["target-row".to_string()],
        );
        db.add_pre_activity_metadata(&metadata).unwrap();

        let mut updated = activity;
        updated.id = "mismatched_lightning_id".to_string();
        updated.status = PaymentState::Succeeded;

        db.update_lightning_activity_by_id(&activity_id, &updated)
            .unwrap();

        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, &activity_id).unwrap(),
            vec!["target-row".to_string()]
        );
        assert!(db
            .get_tags(DEFAULT_WALLET_ID, "mismatched_lightning_id")
            .unwrap()
            .is_empty());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "mismatched_lightning_id", false)
            .unwrap()
            .is_none());

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

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &onchain.id)
            .unwrap()
            .unwrap();
        if let Activity::Onchain(retrieved) = retrieved {
            assert_eq!(retrieved.value, onchain.value);
            assert!(retrieved.created_at.is_some());
            let first_update = retrieved.updated_at;

            // Test update path
            std::thread::sleep(std::time::Duration::from_secs(1));
            let mut metadata = create_test_pre_activity_metadata(
                onchain.tx_id.clone(),
                ActivityType::Onchain,
                vec!["upsert-update-tag".to_string()],
            );
            metadata.tx_id = Some(onchain.tx_id.clone());
            db.add_pre_activity_metadata(&metadata).unwrap();

            onchain.value = 100_000;
            let updated = Activity::Onchain(onchain);
            assert!(db.upsert_activity(&updated).is_ok());

            // Verify update
            let retrieved = db
                .get_activity_by_id(DEFAULT_WALLET_ID, &updated.get_id())
                .unwrap()
                .unwrap();
            if let Activity::Onchain(retrieved) = retrieved {
                assert_eq!(retrieved.value, 100_000);
                assert!(retrieved.updated_at > first_update);
            }
            assert_eq!(
                db.get_tags(DEFAULT_WALLET_ID, &updated.get_id()).unwrap(),
                vec!["upsert-update-tag".to_string()]
            );
            assert!(db
                .get_pre_activity_metadata(DEFAULT_WALLET_ID, "txid123", false)
                .unwrap()
                .is_none());
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

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &lightning.id)
            .unwrap()
            .unwrap();
        if let Activity::Lightning(retrieved) = retrieved {
            assert_eq!(retrieved.status, PaymentState::Pending);

            // Update to succeeded
            std::thread::sleep(std::time::Duration::from_millis(1));
            let metadata = create_test_pre_activity_metadata(
                lightning.id.clone(),
                ActivityType::Lightning,
                vec!["lightning-upsert-update-tag".to_string()],
            );
            db.add_pre_activity_metadata(&metadata).unwrap();

            lightning.status = PaymentState::Succeeded;
            let updated = Activity::Lightning(lightning);
            assert!(db.upsert_activity(&updated).is_ok());

            // Verify status change
            let retrieved = db
                .get_activity_by_id(DEFAULT_WALLET_ID, &updated.get_id())
                .unwrap()
                .unwrap();
            if let Activity::Lightning(retrieved) = retrieved {
                assert_eq!(retrieved.status, PaymentState::Succeeded);
                assert!(retrieved.created_at.is_some());
                assert!(retrieved.updated_at.is_some());
            }
            assert_eq!(
                db.get_tags(DEFAULT_WALLET_ID, &updated.get_id()).unwrap(),
                vec!["lightning-upsert-update-tag".to_string()]
            );
            assert!(db
                .get_pre_activity_metadata(DEFAULT_WALLET_ID, "test_lightning_1", false)
                .unwrap()
                .is_none());
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
    fn test_upsert_activity_empty_wallet_id_fails() {
        let (mut db, db_path) = setup();
        let mut activity = create_test_onchain_activity();
        activity.wallet_id = "".to_string();
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

        let initial = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &onchain.id)
            .unwrap()
            .unwrap();
        if let Activity::Onchain(initial) = initial {
            let created_at = initial.created_at.unwrap();

            // Update and verify created_at stays the same
            std::thread::sleep(std::time::Duration::from_secs(1));
            onchain.value = 100_000;
            let updated = Activity::Onchain(onchain);
            assert!(db.upsert_activity(&updated).is_ok());

            let retrieved = db
                .get_activity_by_id(DEFAULT_WALLET_ID, &updated.get_id())
                .unwrap()
                .unwrap();
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
        let asc_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(SortDirection::Asc),
            )
            .unwrap();
        let asc_timestamps: Vec<u64> = asc_results.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 1001, 1002]);

        // Test descending order
        let desc_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(SortDirection::Desc),
            )
            .unwrap();
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
        db.add_tags(DEFAULT_WALLET_ID, &onchain1.id, &[tag.clone()])
            .unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &onchain2.id, &[tag.clone()])
            .unwrap();

        // Test ascending order
        let asc_activities = db
            .get_activities_by_tag(None, &tag, None, Some(SortDirection::Asc))
            .unwrap();
        let asc_timestamps: Vec<u64> = asc_activities.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 2000]);

        // Test descending order
        let desc_activities = db
            .get_activities_by_tag(None, &tag, None, Some(SortDirection::Desc))
            .unwrap();
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
        let asc_limited = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                Some(3),
                Some(SortDirection::Asc),
            )
            .unwrap();
        let asc_timestamps: Vec<u64> = asc_limited.iter().map(|a| a.get_timestamp()).collect();
        assert_eq!(asc_timestamps, vec![1000, 1001, 1002]);

        // Test descending order with limit
        let desc_limited = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                Some(3),
                Some(SortDirection::Desc),
            )
            .unwrap();
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
        let asc_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(SortDirection::Asc),
            )
            .unwrap();
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
        let default_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
        let sent_activities = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                Some(PaymentType::Sent),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(sent_activities.len(), 1);
        assert!(
            matches!(sent_activities[0], Activity::Onchain(ref a) if a.tx_type == PaymentType::Sent)
        );

        // Test filtering by received
        let received_activities = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                Some(PaymentType::Received),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(received_activities.len(), 1);
        assert!(
            matches!(received_activities[0], Activity::Onchain(ref a) if a.tx_type == PaymentType::Received)
        );

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
        let address_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("xyz123".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(address_results.len(), 1);
        assert!(matches!(address_results[0], Activity::Onchain(_)));

        // Test message search
        let message_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("Coffee".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
        let min_date_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                Some(1500),
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(min_date_results.len(), 2);

        // Test max date
        let max_date_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                None,
                Some(2500),
                None,
                None,
            )
            .unwrap();
        assert_eq!(max_date_results.len(), 2);

        // Test date range
        let range_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                Some(1500),
                Some(2500),
                None,
                None,
            )
            .unwrap();
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
        db.add_tags(DEFAULT_WALLET_ID, &onchain1.id, &["payment".to_string()])
            .unwrap();
        db.add_tags(
            DEFAULT_WALLET_ID,
            &onchain2.id,
            &["payment".to_string(), "important".to_string()],
        )
        .unwrap();

        // Test combined filters
        let results = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                Some(PaymentType::Received),
                Some(vec!["payment".to_string()]),
                Some("abc".to_string()),
                Some(1500),
                Some(2500),
                Some(1),
                Some(SortDirection::Desc),
            )
            .unwrap();

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
        let empty_search = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(empty_search.len(), 1); // Changed from 0 to 1

        // Test empty tags array
        let empty_tags = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                Some(vec![]),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity1.id,
            &["tag1".to_string(), "tag2".to_string()],
        )
        .unwrap();
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity2.id,
            &["tag2".to_string(), "tag3".to_string()],
        )
        .unwrap();
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity3.id,
            &["tag1".to_string(), "tag3".to_string()],
        )
        .unwrap();

        // Test filtering with multiple tags (OR condition)
        let results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                Some(vec!["tag1".to_string(), "tag2".to_string()]),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(results.len(), 3);

        // Test with non-existent tag mixed with existing tags
        let mixed_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                Some(vec!["tag1".to_string(), "nonexistent".to_string()]),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(mixed_results.len(), 2);

        cleanup(&db_path);
    }

    #[test]
    fn test_invalid_date_ranges() {
        let (mut db, db_path) = setup();

        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Test max date before min date
        let invalid_range = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                Some(2000),
                Some(1000),
                None,
                None,
            )
            .unwrap();
        assert_eq!(invalid_range.len(), 0);

        // Test dates way in the future
        let future_date = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                None,
                Some(u64::MAX - 1000),
                None,
                None,
                None,
            )
            .unwrap();
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
        let lower_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("coffee".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(lower_results.len(), 1);

        // Test uppercase search
        let upper_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("COFFEE".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(upper_results.len(), 1);

        // Test mixed case search
        let mixed_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("CoFfEe".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &["tag1".to_string()])
            .unwrap();
        db_clone
            .add_tags(DEFAULT_WALLET_ID, &activity.id, &["tag2".to_string()])
            .unwrap();

        // Verify tags from both connections
        let results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                Some(vec!["tag1".to_string(), "tag2".to_string()]),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
        let special_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("%chars".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(special_results.len(), 1);

        // Search with underscore
        let underscore_results = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                None,
                Some("_special".to_string()),
                None,
                None,
                None,
                None,
            )
            .unwrap();
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
                db.add_tags(DEFAULT_WALLET_ID, &activity.id, &["even".to_string()])
                    .unwrap();
            }
        }

        // Test pagination with combined filters
        let page1 = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                Some(vec!["even".to_string()]),
                Some("address".to_string()),
                Some(1000),
                None,
                Some(2),
                Some(SortDirection::Asc),
            )
            .unwrap();
        assert_eq!(page1.len(), 2);

        // Get next page
        let min_date = page1.last().unwrap().get_timestamp();
        let page2 = db
            .get_activities(
                None,
                Some(ActivityFilter::All),
                None,
                Some(vec!["even".to_string()]),
                Some("address".to_string()),
                Some(min_date + 1),
                None,
                Some(2),
                Some(SortDirection::Asc),
            )
            .unwrap();

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
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity1.id,
            &["payment".to_string(), "coffee".to_string()],
        )
        .unwrap();
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity2.id,
            &["payment".to_string(), "food".to_string()],
        )
        .unwrap();

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
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity1.id.clone(),
                tags: vec!["payment".to_string(), "coffee".to_string()],
            },
            ActivityTags {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity2.id.clone(),
                tags: vec!["payment".to_string(), "food".to_string()],
            },
            ActivityTags {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity3.id.clone(),
                tags: vec!["payment".to_string()],
            },
        ];

        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify tags were added
        let tags1 = db.get_tags(DEFAULT_WALLET_ID, &activity1.id).unwrap();
        assert_eq!(tags1.len(), 2);
        assert!(tags1.contains(&"payment".to_string()));
        assert!(tags1.contains(&"coffee".to_string()));

        let tags2 = db.get_tags(DEFAULT_WALLET_ID, &activity2.id).unwrap();
        assert_eq!(tags2.len(), 2);
        assert!(tags2.contains(&"payment".to_string()));
        assert!(tags2.contains(&"food".to_string()));

        let tags3 = db.get_tags(DEFAULT_WALLET_ID, &activity3.id).unwrap();
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
        let activity_tags = vec![ActivityTags {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            activity_id: activity.id.clone(),
            tags: vec!["payment".to_string(), "coffee".to_string()],
        }];
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Second upsert with same tags (should be idempotent)
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify tags are still there and not duplicated
        let tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &["payment".to_string()])
            .unwrap();

        // Upsert with additional tags (adds new tags, keeps existing)
        let activity_tags = vec![ActivityTags {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            activity_id: activity.id.clone(),
            tags: vec![
                "payment".to_string(),
                "coffee".to_string(),
                "food".to_string(),
            ],
        }];
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify all tags are present (payment was already there, coffee and food are new)
        let tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        let activity_tags = vec![ActivityTags {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            activity_id: activity.id.clone(),
            tags: vec!["payment".to_string(), "".to_string(), "coffee".to_string()],
        }];
        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify only non-empty tags were added
        let tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity1.id.clone(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
            },
            ActivityTags {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity2.id.clone(),
                tags: vec!["tag2".to_string(), "tag3".to_string()],
            },
            ActivityTags {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity3.id.clone(),
                tags: vec!["tag1".to_string(), "tag3".to_string(), "tag4".to_string()],
            },
        ];

        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify all tags were added correctly
        let tags1 = db.get_tags(DEFAULT_WALLET_ID, &activity1.id).unwrap();
        assert_eq!(tags1.len(), 2);
        assert!(tags1.contains(&"tag1".to_string()));
        assert!(tags1.contains(&"tag2".to_string()));

        let tags2 = db.get_tags(DEFAULT_WALLET_ID, &activity2.id).unwrap();
        assert_eq!(tags2.len(), 2);
        assert!(tags2.contains(&"tag2".to_string()));
        assert!(tags2.contains(&"tag3".to_string()));

        let tags3 = db.get_tags(DEFAULT_WALLET_ID, &activity3.id).unwrap();
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
        db.add_tags(
            DEFAULT_WALLET_ID,
            &onchain.id,
            &["payment".to_string(), "coffee".to_string()],
        )
        .unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &lightning.id, &["payment".to_string()])
            .unwrap();

        // Get all activity tags
        let activity_tags = db.get_all_activities_tags().unwrap();

        assert_eq!(activity_tags.len(), 2);

        // Find onchain tags
        let onchain_tags = activity_tags
            .iter()
            .find(|at| at.activity_id == onchain.id)
            .unwrap();
        assert_eq!(onchain_tags.activity_id, onchain.id);
        assert_eq!(onchain_tags.tags.len(), 2);
        assert!(onchain_tags.tags.contains(&"payment".to_string()));
        assert!(onchain_tags.tags.contains(&"coffee".to_string()));

        // Find lightning tags
        let lightning_tags = activity_tags
            .iter()
            .find(|at| at.activity_id == lightning.id)
            .unwrap();
        assert_eq!(lightning_tags.activity_id, lightning.id);
        assert_eq!(lightning_tags.tags.len(), 1);
        assert!(lightning_tags.tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_activity_tags_backup_keeps_duplicate_ids_wallet_scoped() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";
        let activity_id = "shared_activity_id";

        let mut main = create_test_onchain_activity();
        main.id = activity_id.to_string();
        main.tx_id = "main_shared_activity_tags_txid".to_string();

        let mut hardware = create_test_onchain_activity();
        hardware.wallet_id = wallet_id.to_string();
        hardware.id = activity_id.to_string();
        hardware.tx_id = "hardware_shared_activity_tags_txid".to_string();

        db.insert_onchain_activity(&main).unwrap();
        db.insert_onchain_activity(&hardware).unwrap();

        db.upsert_tags(&[
            ActivityTags {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                activity_id: activity_id.to_string(),
                tags: vec!["main".to_string()],
            },
            ActivityTags {
                wallet_id: wallet_id.to_string(),
                activity_id: activity_id.to_string(),
                tags: vec!["hardware".to_string()],
            },
        ])
        .unwrap();

        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, activity_id).unwrap(),
            vec!["main".to_string()]
        );
        assert_eq!(
            db.get_tags(wallet_id, activity_id).unwrap(),
            vec!["hardware".to_string()]
        );

        let activity_tags = db.get_all_activities_tags().unwrap();
        assert_eq!(activity_tags.len(), 2);

        let main_tags = activity_tags
            .iter()
            .find(|tags| tags.wallet_id == DEFAULT_WALLET_ID && tags.activity_id == activity_id)
            .unwrap();
        assert_eq!(main_tags.tags, vec!["main".to_string()]);

        let hardware_tags = activity_tags
            .iter()
            .find(|tags| tags.wallet_id == wallet_id && tags.activity_id == activity_id)
            .unwrap();
        assert_eq!(hardware_tags.tags, vec!["hardware".to_string()]);

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
        db.add_tags(DEFAULT_WALLET_ID, &activity.id, &["old_tag".to_string()])
            .unwrap();

        // Upsert with empty tags (with INSERT OR IGNORE, won't clear existing tags)
        let activity_tags = vec![ActivityTags {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            activity_id: activity.id.clone(),
            tags: vec![],
        }];

        assert!(db.upsert_tags(&activity_tags).is_ok());

        // Verify old tags still exist (empty tags list doesn't clear)
        let tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        let activity_tags = vec![ActivityTags {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            activity_id: "".to_string(),
            tags: vec!["payment".to_string()],
        }];

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
        db.add_tags(DEFAULT_WALLET_ID, &activity1.id, &["payment".to_string()])
            .unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &activity2.id, &["invoice".to_string()])
            .unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &activity3.id, &["transfer".to_string()])
            .unwrap();
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity4.id,
            &["payment".to_string(), "invoice".to_string()],
        )
        .unwrap();

        // Insert closed channels
        let mut channel1 = create_test_closed_channel();
        channel1.channel_id = "channel1".to_string();
        let mut channel2 = create_test_closed_channel();
        channel2.channel_id = "channel2".to_string();
        db.upsert_closed_channel(&channel1).unwrap();
        db.upsert_closed_channel(&channel2).unwrap();

        // Verify data exists
        let activities = db
            .get_activities(None, None, None, None, None, None, None, None, None)
            .unwrap();
        assert_eq!(activities.len(), 4);
        let tags = db.get_all_unique_tags().unwrap();
        assert_eq!(tags.len(), 3);
        let channels = db.get_all_closed_channels(None).unwrap();
        assert_eq!(channels.len(), 2);

        // Wipe all data
        db.wipe_all().unwrap();

        // Verify everything is deleted
        let activities_after = db
            .get_activities(None, None, None, None, None, None, None, None, None)
            .unwrap();
        assert_eq!(activities_after.len(), 0);
        let tags_after = db.get_all_unique_tags().unwrap();
        assert_eq!(tags_after.len(), 0);
        let channels_after = db.get_all_closed_channels(None).unwrap();
        assert_eq!(channels_after.len(), 0);

        // Verify we can still insert new data after wipe
        let new_activity = create_test_onchain_activity();
        db.insert_onchain_activity(&new_activity).unwrap();
        let activities_new = db
            .get_activities(None, None, None, None, None, None, None, None, None)
            .unwrap();
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
        assert_eq!(
            retrieved_channel.counterparty_node_id,
            channel.counterparty_node_id
        );
        assert_eq!(retrieved_channel.funding_txo_txid, channel.funding_txo_txid);
        assert_eq!(
            retrieved_channel.funding_txo_index,
            channel.funding_txo_index
        );
        assert_eq!(
            retrieved_channel.channel_value_sats,
            channel.channel_value_sats
        );
        assert_eq!(retrieved_channel.closed_at, channel.closed_at);
        assert_eq!(
            retrieved_channel.outbound_capacity_msat,
            channel.outbound_capacity_msat
        );
        assert_eq!(
            retrieved_channel.inbound_capacity_msat,
            channel.inbound_capacity_msat
        );
        assert_eq!(
            retrieved_channel.counterparty_unspendable_punishment_reserve,
            channel.counterparty_unspendable_punishment_reserve
        );
        assert_eq!(
            retrieved_channel.unspendable_punishment_reserve,
            channel.unspendable_punishment_reserve
        );
        assert_eq!(
            retrieved_channel.forwarding_fee_proportional_millionths,
            channel.forwarding_fee_proportional_millionths
        );
        assert_eq!(
            retrieved_channel.forwarding_fee_base_msat,
            channel.forwarding_fee_base_msat
        );
        assert_eq!(retrieved_channel.channel_name, channel.channel_name);
        assert_eq!(
            retrieved_channel.channel_closure_reason,
            channel.channel_closure_reason
        );

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
        let all_channels_asc = db
            .get_all_closed_channels(Some(SortDirection::Asc))
            .unwrap();
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
            let ch = all
                .iter()
                .find(|c| c.channel_id == id)
                .expect("missing channel");
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
        let c1 = after
            .iter()
            .find(|c| c.channel_id == "bulk_channel_1")
            .unwrap();
        assert_eq!(c1.channel_value_sats, 9_999_999);
        let c2 = after
            .iter()
            .find(|c| c.channel_id == "bulk_channel_2")
            .unwrap();
        assert_eq!(c2.channel_name, "Updated Name");
        let c3 = after
            .iter()
            .find(|c| c.channel_id == "bulk_channel_3")
            .unwrap();
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

        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(all.len(), 5);

        let mut updated = acts.clone();
        updated[0].value = 999_999;
        updated[1].fee = 42;
        updated[2].fee_rate = 7;
        updated[3].is_boosted = true;
        assert!(db.upsert_onchain_activities(&updated).is_ok());

        let after = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let map: std::collections::HashMap<String, OnchainActivity> = after
            .into_iter()
            .map(|a| match a {
                Activity::Onchain(o) => (o.id.clone(), o),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(map["onchain_bulk_0"].value, 999_999);
        assert_eq!(map["onchain_bulk_1"].fee, 42);
        assert_eq!(map["onchain_bulk_2"].fee_rate, 7);
        assert!(map["onchain_bulk_3"].is_boosted);

        cleanup(&db_path);
    }

    #[test]
    fn test_bulk_upserts_preserve_tags_and_seen_state() {
        let (mut db, db_path) = setup();
        let seen_timestamp = 1234567999;

        let mut onchain = create_test_onchain_activity();
        onchain.id = "onchain_preserve_state".to_string();
        onchain.tx_id = "onchain_preserve_state_txid".to_string();

        db.upsert_onchain_activities(&[onchain.clone()]).unwrap();
        db.add_tags(DEFAULT_WALLET_ID, &onchain.id, &["onchain_tag".to_string()])
            .unwrap();
        db.mark_activity_as_seen(DEFAULT_WALLET_ID, &onchain.id, seen_timestamp)
            .unwrap();

        let mut updated_onchain = onchain.clone();
        updated_onchain.value = 99_999;
        db.upsert_onchain_activities(&[updated_onchain]).unwrap();

        let retrieved_onchain = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &onchain.id)
            .unwrap()
            .unwrap();
        match retrieved_onchain {
            Activity::Onchain(activity) => {
                assert_eq!(activity.value, 99_999);
                assert_eq!(activity.seen_at, Some(seen_timestamp));
            }
            Activity::Lightning(_) => panic!("Expected onchain activity"),
        }
        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, &onchain.id).unwrap(),
            vec!["onchain_tag".to_string()]
        );

        let mut lightning = create_test_lightning_activity();
        lightning.id = "lightning_preserve_state".to_string();

        db.upsert_lightning_activities(&[lightning.clone()])
            .unwrap();
        db.add_tags(
            DEFAULT_WALLET_ID,
            &lightning.id,
            &["lightning_tag".to_string()],
        )
        .unwrap();
        db.mark_activity_as_seen(DEFAULT_WALLET_ID, &lightning.id, seen_timestamp)
            .unwrap();

        let mut updated_lightning = lightning.clone();
        updated_lightning.value = 77_777;
        db.upsert_lightning_activities(&[updated_lightning])
            .unwrap();

        let retrieved_lightning = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &lightning.id)
            .unwrap()
            .unwrap();
        match retrieved_lightning {
            Activity::Lightning(activity) => {
                assert_eq!(activity.value, 77_777);
                assert_eq!(activity.seen_at, Some(seen_timestamp));
            }
            Activity::Onchain(_) => panic!("Expected lightning activity"),
        }
        assert_eq!(
            db.get_tags(DEFAULT_WALLET_ID, &lightning.id).unwrap(),
            vec!["lightning_tag".to_string()]
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_bulk_upsert_onchain_transfers_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";
        let address = "bc1qbulkmetadata".to_string();

        let mut metadata = create_test_pre_activity_metadata(
            "hardware_bulk_pending".to_string(),
            ActivityType::Onchain,
            vec!["bulk-tag".to_string()],
        );
        metadata.wallet_id = wallet_id.to_string();
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.fee_rate = 7;
        db.add_pre_activity_metadata(&metadata).unwrap();

        let mut activity = create_test_onchain_activity();
        activity.wallet_id = wallet_id.to_string();
        activity.id = "hardware_bulk_activity".to_string();
        activity.tx_id = "hardware_bulk_txid".to_string();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.fee_rate = 1;

        db.upsert_onchain_activities(&[activity.clone()]).unwrap();

        assert_eq!(
            db.get_tags(wallet_id, &activity.id).unwrap(),
            vec!["bulk-tag".to_string()]
        );
        let retrieved = db
            .get_activity_by_id(wallet_id, &activity.id)
            .unwrap()
            .unwrap();
        match retrieved {
            Activity::Onchain(onchain) => assert_eq!(onchain.fee_rate, 7),
            Activity::Lightning(_) => panic!("Expected onchain activity"),
        }
        assert!(db
            .get_pre_activity_metadata(wallet_id, &address, true)
            .unwrap()
            .is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_bulk_upsert_lightning_transfers_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";
        let payment_id = "hardware_bulk_lightning".to_string();

        let mut metadata = create_test_pre_activity_metadata(
            payment_id.clone(),
            ActivityType::Lightning,
            vec!["lightning-bulk-tag".to_string()],
        );
        metadata.wallet_id = wallet_id.to_string();
        db.add_pre_activity_metadata(&metadata).unwrap();

        let mut activity = create_test_lightning_activity();
        activity.wallet_id = wallet_id.to_string();
        activity.id = payment_id.clone();

        db.upsert_lightning_activities(&[activity.clone()]).unwrap();

        assert_eq!(
            db.get_tags(wallet_id, &activity.id).unwrap(),
            vec!["lightning-bulk-tag".to_string()]
        );
        assert!(db
            .get_pre_activity_metadata(wallet_id, &payment_id, false)
            .unwrap()
            .is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_onchain_activities_empty() {
        let (mut db, db_path) = setup();
        assert!(db.upsert_onchain_activities(&[]).is_ok());
        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::Onchain),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
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

        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(all.len(), 5);

        let mut updated = acts.clone();
        updated[0].value = 55;
        updated[1].status = PaymentState::Failed;
        updated[2].fee = Some(0);
        updated[3].message = "updated".to_string();
        assert!(db.upsert_lightning_activities(&updated).is_ok());

        let after = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let map: std::collections::HashMap<String, LightningActivity> = after
            .into_iter()
            .map(|a| match a {
                Activity::Lightning(l) => (l.id.clone(), l),
                _ => unreachable!(),
            })
            .collect();
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
        let all = db
            .get_activities(
                None,
                Some(ActivityFilter::Lightning),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(all.is_empty());
        cleanup(&db_path);
    }

    // ========== Pre-Activity Metadata Tests ==========

    #[test]
    fn test_add_pre_activity_metadata_onchain() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string(), "coffee".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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

        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                payment_hash.clone(),
                ActivityType::Lightning,
                tags.clone()
            ))
            .is_ok());

        // Verify tags are transferred when activity is received
        let mut activity = create_test_lightning_activity();
        activity.id = payment_hash.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_lightning_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"invoice".to_string()));
        assert!(activity_tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_empty_identifier() {
        let (mut db, db_path) = setup();
        let tags = vec!["payment".to_string()];

        let result = db.add_pre_activity_metadata(&create_test_pre_activity_metadata(
            "".to_string(),
            ActivityType::Onchain,
            tags,
        ));
        assert!(result.is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_duplicate() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata1 =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        let mut metadata2 =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata2.address = Some(address.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        let mut metadata1 = create_test_pre_activity_metadata(
            payment_id1.clone(),
            ActivityType::Onchain,
            vec!["tag1".to_string()],
        );
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());

        // Verify it exists
        let result1 = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &payment_id1, false)
            .unwrap();
        assert!(result1.is_some());
        let result_by_address1 = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
            .unwrap();
        assert!(result_by_address1.is_some());
        assert_eq!(result_by_address1.unwrap().payment_id, payment_id1);

        // Add metadata with payment_id2 and same address (should replace metadata1)
        let mut metadata2 = create_test_pre_activity_metadata(
            payment_id2.clone(),
            ActivityType::Onchain,
            vec!["tag2".to_string()],
        );
        metadata2.address = Some(address.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Verify metadata1 is gone
        let result1_after = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &payment_id1, false)
            .unwrap();
        assert!(result1_after.is_none());

        // Verify metadata2 exists and can be found by address
        let result2 = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &payment_id2, false)
            .unwrap();
        assert!(result2.is_some());
        let result_by_address2 = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
            .unwrap();
        assert!(result_by_address2.is_some());
        let metadata2_retrieved = result_by_address2.unwrap();
        assert_eq!(metadata2_retrieved.payment_id, payment_id2);
        assert_eq!(metadata2_retrieved.tags, vec!["tag2".to_string()]);

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_address_replacement_is_receive_scoped() {
        let (mut db, db_path) = setup();
        let address = "bc1qsharedmetadataaddress".to_string();

        let mut receive_metadata = create_test_pre_activity_metadata(
            "receive_pending_1".to_string(),
            ActivityType::Onchain,
            vec!["receive-old".to_string()],
        );
        receive_metadata.address = Some(address.clone());
        receive_metadata.is_receive = true;
        db.add_pre_activity_metadata(&receive_metadata).unwrap();

        let mut sent_metadata = create_test_pre_activity_metadata(
            "sent_txid_1".to_string(),
            ActivityType::Onchain,
            vec!["sent".to_string()],
        );
        sent_metadata.tx_id = Some("sent_txid_1".to_string());
        sent_metadata.address = Some(address.clone());
        sent_metadata.is_receive = false;
        db.add_pre_activity_metadata(&sent_metadata).unwrap();

        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "receive_pending_1", false)
            .unwrap()
            .is_some());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "sent_txid_1", false)
            .unwrap()
            .is_some());

        let mut new_receive_metadata = create_test_pre_activity_metadata(
            "receive_pending_2".to_string(),
            ActivityType::Onchain,
            vec!["receive-new".to_string()],
        );
        new_receive_metadata.address = Some(address.clone());
        new_receive_metadata.is_receive = true;
        db.add_pre_activity_metadata(&new_receive_metadata).unwrap();

        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "receive_pending_1", false)
            .unwrap()
            .is_none());
        assert_eq!(
            db.get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
                .unwrap()
                .unwrap()
                .payment_id,
            "receive_pending_2"
        );
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "sent_txid_1", false)
            .unwrap()
            .is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_multiple() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        let mut metadata1 = create_test_pre_activity_metadata(
            address.clone(),
            ActivityType::Onchain,
            vec!["tag1".to_string()],
        );
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(
            address.clone(),
            ActivityType::Onchain,
            vec!["tag2".to_string(), "tag3".to_string()],
        );
        metadata2.address = Some(address.clone());
        metadata2.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address.clone(),
                ActivityType::Onchain,
                vec!["tag1".to_string()]
            ))
            .is_ok());

        // Add more tags to existing metadata
        assert!(db
            .add_pre_activity_metadata_tags(
                DEFAULT_WALLET_ID,
                &address,
                &["tag2".to_string(), "tag3".to_string()]
            )
            .is_ok());

        // Verify all tags are present
        let all_metadata = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_metadata.len(), 1);
        let metadata = &all_metadata[0];
        assert_eq!(metadata.tags.len(), 3);
        assert!(metadata.tags.contains(&"tag1".to_string()));
        assert!(metadata.tags.contains(&"tag2".to_string()));
        assert!(metadata.tags.contains(&"tag3".to_string()));

        // Add duplicate tag (should not add duplicate)
        assert!(db
            .add_pre_activity_metadata_tags(DEFAULT_WALLET_ID, &address, &["tag2".to_string()])
            .is_ok());

        // Verify no duplicate was added
        let all_metadata_after = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_metadata_after.len(), 1);
        let metadata_after = &all_metadata_after[0];
        assert_eq!(metadata_after.tags.len(), 3);
        assert_eq!(
            metadata_after.tags.iter().filter(|t| *t == "tag2").count(),
            1
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_add_pre_activity_metadata_tags_nonexistent() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Try to add tags to non-existent metadata (should error)
        let result =
            db.add_pre_activity_metadata_tags(DEFAULT_WALLET_ID, &address, &["tag1".to_string()]);
        assert!(result.is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_remove_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        assert!(db
            .remove_pre_activity_metadata_tags(DEFAULT_WALLET_ID, &address, &["tag2".to_string()])
            .is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        let tags = vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "tag3".to_string(),
            "tag4".to_string(),
        ];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        assert!(db
            .remove_pre_activity_metadata_tags(
                DEFAULT_WALLET_ID,
                &address,
                &["tag1".to_string(), "tag3".to_string()]
            )
            .is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        assert!(db
            .remove_pre_activity_metadata_tags(
                DEFAULT_WALLET_ID,
                &address,
                &["nonexistent".to_string()]
            )
            .is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_reset_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()];

        // Add tags
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address.clone(),
                ActivityType::Onchain,
                tags.clone()
            ))
            .is_ok());

        // Reset all tags
        assert!(db
            .reset_pre_activity_metadata_tags(DEFAULT_WALLET_ID, &address)
            .is_ok());

        // Verify no tags are transferred
        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_reset_pre_activity_metadata_empty() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Reset tags that don't exist (should not error)
        assert!(db
            .reset_pre_activity_metadata_tags(DEFAULT_WALLET_ID, &address)
            .is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_pre_activity_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()];

        // Add tags
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address.clone(),
                ActivityType::Onchain,
                tags.clone()
            ))
            .is_ok());

        // Verify metadata exists
        let all_metadata = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_metadata.len(), 1);

        // Delete all metadata
        assert!(db
            .delete_pre_activity_metadata(DEFAULT_WALLET_ID, &address)
            .is_ok());

        // Verify metadata is deleted
        let all_metadata_after = db.get_all_pre_activity_metadata().unwrap();
        assert!(all_metadata_after.is_empty());

        // Verify no tags are transferred after deletion
        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transferred_on_received() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut received_activity = create_test_onchain_activity();
        received_activity.id = "received_activity".to_string();
        received_activity.address = address.clone();
        received_activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&received_activity).unwrap();

        let received_tags = db
            .get_tags(DEFAULT_WALLET_ID, &received_activity.id)
            .unwrap();
        assert_eq!(received_tags.len(), 1);
        assert!(received_tags.contains(&"payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_received_lookup_ignores_sent_only_address_metadata() {
        let (mut db, db_path) = setup();
        let address = "bc1qsentonlyaddress".to_string();

        let mut metadata = create_test_pre_activity_metadata(
            "sent_only_txid".to_string(),
            ActivityType::Onchain,
            vec!["sent-only".to_string()],
        );
        metadata.tx_id = Some("sent_only_txid".to_string());
        metadata.address = Some(address.clone());
        metadata.is_receive = false;
        db.add_pre_activity_metadata(&metadata).unwrap();

        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
            .unwrap()
            .is_none());

        let mut received_activity = create_test_onchain_activity();
        received_activity.id = "received_should_ignore_sent_metadata".to_string();
        received_activity.tx_id = "received_should_ignore_sent_metadata_txid".to_string();
        received_activity.address = address.clone();
        received_activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&received_activity).unwrap();

        assert!(db
            .get_tags(DEFAULT_WALLET_ID, &received_activity.id)
            .unwrap()
            .is_empty());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "sent_only_txid", false)
            .unwrap()
            .is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transferred_on_sent_onchain() {
        let (mut db, db_path) = setup();
        let tx_id = "txid123".to_string();
        let tags = vec!["sent_payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(tx_id.clone(), ActivityType::Onchain, tags.clone());
        metadata.tx_id = Some(tx_id.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut sent_activity = create_test_onchain_activity();
        sent_activity.tx_id = tx_id.clone();
        sent_activity.tx_type = PaymentType::Sent;
        db.insert_onchain_activity(&sent_activity).unwrap();

        let sent_tags = db.get_tags(DEFAULT_WALLET_ID, &sent_activity.id).unwrap();
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

        let mut metadata =
            create_test_pre_activity_metadata(tx_id.clone(), ActivityType::Onchain, tags.clone());
        metadata.tx_id = Some(tx_id.clone());
        metadata.address = Some(metadata_address.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut sent_activity = create_test_onchain_activity();
        sent_activity.tx_id = tx_id.clone();
        sent_activity.address = "bc1qoriginal789".to_string();
        sent_activity.tx_type = PaymentType::Sent;
        db.insert_onchain_activity(&sent_activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &sent_activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.address, metadata_address);
        } else {
            panic!("Expected Onchain activity");
        }

        let sent_tags = db.get_tags(DEFAULT_WALLET_ID, &sent_activity.id).unwrap();
        assert_eq!(sent_tags.len(), 1);
        assert!(sent_tags.contains(&"sent_payment".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_fee_rate_transfer() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.fee_rate = 10;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.fee_rate = 0;
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.fee_rate, 10);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_is_transfer_transfer() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.is_transfer = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.is_transfer = false;
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.is_transfer, true);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_channel_id_transfer() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let channel_id = "channel_abc123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.channel_id = Some(channel_id.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.channel_id = None;
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.channel_id, Some(channel_id));
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_all_fields_transfer() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let channel_id = "channel_xyz789".to_string();
        let tags = vec!["payment".to_string(), "transfer".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.fee_rate = 15;
        metadata.is_transfer = true;
        metadata.channel_id = Some(channel_id.clone());
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.fee_rate = 0;
        activity.is_transfer = false;
        activity.channel_id = None;
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.address, address);
            assert_eq!(activity.fee_rate, 15);
            assert_eq!(activity.is_transfer, true);
            assert_eq!(activity.channel_id, Some(channel_id));
            let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
            assert_eq!(activity_tags.len(), 2);
            assert!(activity_tags.contains(&"payment".to_string()));
            assert!(activity_tags.contains(&"transfer".to_string()));
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_fee_rate_zero_not_transferred() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.fee_rate = 0;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.fee_rate = 5;
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.fee_rate, 5);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_is_transfer_false_not_transferred() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["payment".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        metadata.is_transfer = false;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.is_transfer = false;
        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap();
        if let Activity::Onchain(activity) = retrieved.unwrap() {
            assert_eq!(activity.is_transfer, false);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transferred_on_lightning_sent() {
        let (mut db, db_path) = setup();
        let payment_hash = "test_lightning_sent_1".to_string();
        let tags = vec!["sent_invoice".to_string()];

        // Add pre-activity metadata using payment hash
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                payment_hash.clone(),
                ActivityType::Lightning,
                tags.clone()
            ))
            .is_ok());

        // Insert sent lightning activity (should transfer tags based on payment hash)
        let mut sent_activity = create_test_lightning_activity();
        sent_activity.id = payment_hash.clone();
        sent_activity.tx_type = PaymentType::Sent;
        db.insert_lightning_activity(&sent_activity).unwrap();

        let sent_tags = db.get_tags(DEFAULT_WALLET_ID, &sent_activity.id).unwrap();
        assert_eq!(sent_tags.len(), 1);
        assert!(sent_tags.contains(&"sent_invoice".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_deleted_after_transfer() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let tags = vec!["tag1".to_string(), "tag2".to_string()];

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity1 = create_test_onchain_activity();
        activity1.address = address.clone();
        activity1.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity1).unwrap();

        let tags1 = db.get_tags(DEFAULT_WALLET_ID, &activity1.id).unwrap();
        assert_eq!(tags1.len(), 2);

        let mut activity2 = create_test_onchain_activity();
        activity2.id = "activity2".to_string();
        activity2.address = address.clone();
        activity2.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity2).unwrap();

        let tags2 = db.get_tags(DEFAULT_WALLET_ID, &activity2.id).unwrap();
        assert!(tags2.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_lightning_received() {
        let (mut db, db_path) = setup();
        let payment_hash = "test_lightning_received_1".to_string();
        let tags = vec!["invoice".to_string(), "payment".to_string()];

        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                payment_hash.clone(),
                ActivityType::Lightning,
                tags.clone()
            ))
            .is_ok());

        let mut activity = create_test_lightning_activity();
        activity.id = payment_hash.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_lightning_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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

        let mut metadata =
            create_test_pre_activity_metadata(address.clone(), ActivityType::Onchain, tags.clone());
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.id = ln_payment_hash.clone();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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

        let mut metadata1 = create_test_pre_activity_metadata(
            address1.clone(),
            ActivityType::Onchain,
            vec!["tag1".to_string()],
        );
        metadata1.address = Some(address1.clone());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(
            address2.clone(),
            ActivityType::Onchain,
            vec!["tag2".to_string()],
        );
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
        let tags1 = db.get_tags(DEFAULT_WALLET_ID, &activity1.id).unwrap();
        assert_eq!(tags1.len(), 1);
        assert!(tags1.contains(&"tag1".to_string()));

        let tags2 = db.get_tags(DEFAULT_WALLET_ID, &activity2.id).unwrap();
        assert_eq!(tags2.len(), 1);
        assert!(tags2.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_transfer_is_wallet_scoped() {
        let (mut db, db_path) = setup();
        let hardware_wallet_id = "hardware-wallet-1";
        let address = "bc1qsharedaddress".to_string();

        let mut metadata = create_test_pre_activity_metadata(
            "bitkit_pending".to_string(),
            ActivityType::Onchain,
            vec!["bitkit_tag".to_string()],
        );
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut hardware_activity = create_test_onchain_activity();
        hardware_activity.wallet_id = hardware_wallet_id.to_string();
        hardware_activity.id = "hardware_shared_address_activity".to_string();
        hardware_activity.tx_id = "hardware_shared_address_txid".to_string();
        hardware_activity.address = address.clone();
        hardware_activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&hardware_activity).unwrap();

        let hardware_tags = db
            .get_tags(hardware_wallet_id, &hardware_activity.id)
            .unwrap();
        assert!(hardware_tags.is_empty());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
            .unwrap()
            .is_some());

        let mut bitkit_activity = create_test_onchain_activity();
        bitkit_activity.id = "bitkit_shared_address_activity".to_string();
        bitkit_activity.tx_id = "bitkit_shared_address_txid".to_string();
        bitkit_activity.address = address.clone();
        bitkit_activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&bitkit_activity).unwrap();

        let bitkit_tags = db.get_tags(DEFAULT_WALLET_ID, &bitkit_activity.id).unwrap();
        assert_eq!(bitkit_tags, vec!["bitkit_tag".to_string()]);
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
            .unwrap()
            .is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_allows_same_payment_id_across_wallets() {
        let (mut db, db_path) = setup();
        let hardware_wallet_id = "hardware-wallet-1";
        let payment_id = "shared_pending_payment".to_string();

        let default_metadata = create_test_pre_activity_metadata(
            payment_id.clone(),
            ActivityType::Lightning,
            vec!["bitkit_tag".to_string()],
        );
        let mut hardware_metadata = create_test_pre_activity_metadata(
            payment_id.clone(),
            ActivityType::Lightning,
            vec!["hardware_tag".to_string()],
        );
        hardware_metadata.wallet_id = hardware_wallet_id.to_string();

        assert!(db.add_pre_activity_metadata(&default_metadata).is_ok());
        assert!(db.add_pre_activity_metadata(&hardware_metadata).is_ok());

        assert!(db
            .add_pre_activity_metadata_tags(
                hardware_wallet_id,
                &payment_id,
                &["hardware_extra".to_string()]
            )
            .is_ok());

        let default_result = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &payment_id, false)
            .unwrap()
            .unwrap();
        let hardware_result = db
            .get_pre_activity_metadata(hardware_wallet_id, &payment_id, false)
            .unwrap()
            .unwrap();

        assert_eq!(default_result.tags, vec!["bitkit_tag".to_string()]);
        assert_eq!(
            hardware_result.tags,
            vec!["hardware_tag".to_string(), "hardware_extra".to_string()]
        );

        assert!(db
            .delete_pre_activity_metadata(hardware_wallet_id, &payment_id)
            .is_ok());
        assert!(db
            .get_pre_activity_metadata(hardware_wallet_id, &payment_id, false)
            .unwrap()
            .is_none());
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &payment_id, false)
            .unwrap()
            .is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_onchain_and_lightning_separate() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();
        let payment_hash = "test_lightning_separate_1".to_string();

        let mut metadata1 = create_test_pre_activity_metadata(
            address.clone(),
            ActivityType::Onchain,
            vec!["onchain_tag".to_string()],
        );
        metadata1.address = Some(address.clone());
        metadata1.is_receive = true;
        let metadata2 = create_test_pre_activity_metadata(
            payment_hash.clone(),
            ActivityType::Lightning,
            vec!["lightning_tag".to_string()],
        );
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
        let onchain_tags = db
            .get_tags(DEFAULT_WALLET_ID, &onchain_activity.id)
            .unwrap();
        assert_eq!(onchain_tags.len(), 1);
        assert!(onchain_tags.contains(&"onchain_tag".to_string()));

        let lightning_tags = db
            .get_tags(DEFAULT_WALLET_ID, &lightning_activity.id)
            .unwrap();
        assert_eq!(lightning_tags.len(), 1);
        assert!(lightning_tags.contains(&"lightning_tag".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_empty_tags() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        // Add empty tags (should be allowed, but won't transfer anything meaningful)
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address.clone(),
                ActivityType::Onchain,
                vec![]
            ))
            .is_ok());

        // Insert received activity
        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        // Verify no tags were transferred
        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert!(activity_tags.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_pre_activity_metadata_combined_with_regular_tags() {
        let (mut db, db_path) = setup();
        let address = "bc1qtest123".to_string();

        let mut metadata = create_test_pre_activity_metadata(
            address.clone(),
            ActivityType::Onchain,
            vec!["receiving_tag".to_string()],
        );
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        // Add regular tags to the same activity
        db.add_tags(
            DEFAULT_WALLET_ID,
            &activity.id,
            &["regular_tag".to_string()],
        )
        .unwrap();

        // Verify both types of tags are present
        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
        let result = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, false)
            .unwrap();
        assert!(result.is_none());

        // Add pre-activity metadata
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address.clone(),
                ActivityType::Onchain,
                tags.clone()
            ))
            .is_ok());

        // Get existing metadata
        let metadata = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, false)
            .unwrap();
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
        let mut metadata = create_test_pre_activity_metadata(
            payment_id.clone(),
            ActivityType::Onchain,
            tags.clone(),
        );
        metadata.address = Some(address.clone());
        metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata).is_ok());

        // Test searching by payment_id
        let result_by_payment_id = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &payment_id, false)
            .unwrap();
        assert!(result_by_payment_id.is_some());

        // Test searching by address
        let result_by_address = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
            .unwrap();
        assert!(result_by_address.is_some());
        let metadata_by_address = result_by_address.unwrap();
        assert_eq!(metadata_by_address.payment_id, payment_id);
        assert_eq!(metadata_by_address.tags.len(), 2);
        assert!(metadata_by_address.tags.contains(&"tag1".to_string()));
        assert!(metadata_by_address.tags.contains(&"tag2".to_string()));

        // Test that searching by address with wrong search type returns None
        let result_wrong_search = db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, false)
            .unwrap();
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
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address1.clone(),
                ActivityType::Onchain,
                vec!["tag1".to_string(), "tag2".to_string()]
            ))
            .is_ok());
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                address2.clone(),
                ActivityType::Onchain,
                vec!["tag3".to_string()]
            ))
            .is_ok());
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                invoice.clone(),
                ActivityType::Lightning,
                vec!["tag4".to_string(), "tag5".to_string()]
            ))
            .is_ok());

        // Get all pre-activity metadata
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 3);

        // Find tags for address1
        let addr1_tags = all_tags
            .iter()
            .find(|rt| rt.payment_id == address1)
            .unwrap();
        assert_eq!(addr1_tags.tags.len(), 2);
        assert!(addr1_tags.tags.contains(&"tag1".to_string()));
        assert!(addr1_tags.tags.contains(&"tag2".to_string()));

        // Find tags for address2
        let addr2_tags = all_tags
            .iter()
            .find(|rt| rt.payment_id == address2)
            .unwrap();
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
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "bc1qtest123".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                payment_hash: None,
                tx_id: None,
                address: Some("bc1qtest123".to_string()),
                is_receive: true,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
            PreActivityMetadata {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "bc1qtest456".to_string(),
                tags: vec!["tag3".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
            PreActivityMetadata {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "lightning:invoice123".to_string(),
                tags: vec!["tag4".to_string(), "tag5".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
        ];

        // Upsert pre-activity metadata
        assert!(db
            .upsert_pre_activity_metadata(&pre_activity_metadata)
            .is_ok());

        // Verify tags were added
        let all_tags = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all_tags.len(), 3);

        // Verify tags can be transferred
        let mut activity = create_test_onchain_activity();
        activity.address = "bc1qtest123".to_string();
        activity.tx_type = PaymentType::Received;
        db.insert_onchain_activity(&activity).unwrap();

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
        assert_eq!(activity_tags.len(), 2);
        assert!(activity_tags.contains(&"tag1".to_string()));
        assert!(activity_tags.contains(&"tag2".to_string()));

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_replaces_receive_address_only() {
        let (mut db, db_path) = setup();
        let address = "bc1qbulkrestoreaddress".to_string();

        let pre_activity_metadata = vec![
            PreActivityMetadata {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "receive_old".to_string(),
                tags: vec!["receive-old".to_string()],
                payment_hash: None,
                tx_id: None,
                address: Some(address.clone()),
                is_receive: true,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
            PreActivityMetadata {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "sent_txid".to_string(),
                tags: vec!["sent".to_string()],
                payment_hash: None,
                tx_id: Some("sent_txid".to_string()),
                address: Some(address.clone()),
                is_receive: false,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
            PreActivityMetadata {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "receive_new".to_string(),
                tags: vec!["receive-new".to_string()],
                payment_hash: None,
                tx_id: None,
                address: Some(address.clone()),
                is_receive: true,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
        ];

        db.upsert_pre_activity_metadata(&pre_activity_metadata)
            .unwrap();

        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "receive_old", false)
            .unwrap()
            .is_none());
        assert_eq!(
            db.get_pre_activity_metadata(DEFAULT_WALLET_ID, &address, true)
                .unwrap()
                .unwrap()
                .payment_id,
            "receive_new"
        );
        assert!(db
            .get_pre_activity_metadata(DEFAULT_WALLET_ID, "sent_txid", false)
            .unwrap()
            .is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_pre_activity_metadata_idempotent() {
        let (mut db, db_path) = setup();

        let pre_activity_metadata = vec![PreActivityMetadata {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            payment_id: "bc1qtest123".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            payment_hash: None,
            tx_id: None,
            address: None,
            is_receive: false,
            fee_rate: 0,
            is_transfer: false,
            channel_id: None,
            created_at: 0,
        }];

        // Upsert twice (should be idempotent)
        assert!(db
            .upsert_pre_activity_metadata(&pre_activity_metadata)
            .is_ok());
        assert!(db
            .upsert_pre_activity_metadata(&pre_activity_metadata)
            .is_ok());

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

        let mut initial_metadata = create_test_pre_activity_metadata(
            "bc1qtest123".to_string(),
            ActivityType::Onchain,
            vec!["tag1".to_string()],
        );
        initial_metadata.address = Some("bc1qtest123".to_string());
        initial_metadata.is_receive = true;
        assert!(db.add_pre_activity_metadata(&initial_metadata).is_ok());

        let pre_activity_metadata = vec![PreActivityMetadata {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            payment_id: "bc1qtest123".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()],
            payment_hash: None,
            tx_id: None,
            address: Some("bc1qtest123".to_string()),
            is_receive: true,
            fee_rate: 0,
            is_transfer: false,
            channel_id: None,
            created_at: 0,
        }];

        assert!(db
            .upsert_pre_activity_metadata(&pre_activity_metadata)
            .is_ok());

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

        let pre_activity_metadata = vec![PreActivityMetadata {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            payment_id: "".to_string(),
            tags: vec!["tag1".to_string()],
            payment_hash: None,
            tx_id: None,
            address: None,
            is_receive: false,
            fee_rate: 0,
            is_transfer: false,
            channel_id: None,
            created_at: 0,
        }];

        // Empty identifier is allowed for backup/restore (restores exactly what was backed up)
        assert!(db
            .upsert_pre_activity_metadata(&pre_activity_metadata)
            .is_ok());

        cleanup(&db_path);
    }

    #[test]
    fn test_backup_restore_pre_activity_metadata() {
        let (mut db, db_path) = setup();

        let mut metadata1 = create_test_pre_activity_metadata(
            "bc1qtest123".to_string(),
            ActivityType::Onchain,
            vec!["tag1".to_string(), "tag2".to_string()],
        );
        metadata1.address = Some("bc1qtest123".to_string());
        metadata1.is_receive = true;
        let metadata2 = create_test_pre_activity_metadata(
            "lightning:invoice123".to_string(),
            ActivityType::Lightning,
            vec!["tag3".to_string()],
        );
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());

        // Backup: Get all pre-activity metadata
        let backup = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(backup.len(), 2);

        // Simulate restore: Delete and restore
        assert!(db
            .delete_pre_activity_metadata(DEFAULT_WALLET_ID, &"bc1qtest123".to_string())
            .is_ok());
        assert!(db
            .delete_pre_activity_metadata(DEFAULT_WALLET_ID, &"lightning:invoice123".to_string())
            .is_ok());

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

        let activity_tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
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
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "same_id".to_string(),
                tags: vec!["tag1".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
            PreActivityMetadata {
                wallet_id: DEFAULT_WALLET_ID.to_string(),
                payment_id: "same_id".to_string(),
                tags: vec!["tag2".to_string()],
                payment_hash: None,
                tx_id: None,
                address: None,
                is_receive: false,
                fee_rate: 0,
                is_transfer: false,
                channel_id: None,
                created_at: 0,
            },
        ];

        assert!(db
            .upsert_pre_activity_metadata(&pre_activity_metadata)
            .is_ok());

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
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                "z_address".to_string(),
                ActivityType::Onchain,
                vec!["tag1".to_string()]
            ))
            .is_ok());
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                "a_address".to_string(),
                ActivityType::Onchain,
                vec!["tag2".to_string()]
            ))
            .is_ok());
        assert!(db
            .add_pre_activity_metadata(&create_test_pre_activity_metadata(
                "m_address".to_string(),
                ActivityType::Onchain,
                vec!["tag3".to_string()]
            ))
            .is_ok());

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

        let mut metadata1 = create_test_pre_activity_metadata(
            "address1".to_string(),
            ActivityType::Onchain,
            vec!["tag1".to_string()],
        );
        metadata1.address = Some("address1".to_string());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(
            "address2".to_string(),
            ActivityType::Onchain,
            vec!["tag2".to_string()],
        );
        metadata2.address = Some("address2".to_string());
        metadata2.is_receive = true;
        let mut metadata3 = create_test_pre_activity_metadata(
            "address3".to_string(),
            ActivityType::Onchain,
            vec!["tag3".to_string()],
        );
        metadata3.address = Some("address3".to_string());
        metadata3.is_receive = true;
        assert!(db.add_pre_activity_metadata(&metadata1).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata2).is_ok());
        assert!(db.add_pre_activity_metadata(&metadata3).is_ok());

        // Get all
        let all = db.get_all_pre_activity_metadata().unwrap();
        assert_eq!(all.len(), 3);

        // Upsert with new tags for address2 (replaces existing tags)
        let updated = vec![PreActivityMetadata {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            payment_id: "address2".to_string(),
            tags: vec!["tag2_updated".to_string(), "tag2_new".to_string()],
            payment_hash: None,
            tx_id: None,
            address: None,
            is_receive: false,
            fee_rate: 0,
            is_transfer: false,
            channel_id: None,
            created_at: 0,
        }];

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

        let mut metadata1 = create_test_pre_activity_metadata(
            "bc1qtest123".to_string(),
            ActivityType::Onchain,
            vec!["tag1".to_string(), "tag2".to_string()],
        );
        metadata1.address = Some("bc1qtest123".to_string());
        metadata1.is_receive = true;
        let mut metadata2 = create_test_pre_activity_metadata(
            "bc1qtest456".to_string(),
            ActivityType::Onchain,
            vec!["tag3".to_string()],
        );
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

    #[test]
    fn test_is_address_used_no_activities() {
        let (db, db_path) = setup();
        let address = "bc1qunused123".to_string();

        let is_used = db.is_address_used(&address).unwrap();
        assert!(!is_used, "Address with no activities should return false");

        cleanup(&db_path);
    }

    #[test]
    fn test_is_address_used_with_received_activity() {
        let (mut db, db_path) = setup();
        let address = "bc1qreceived123".to_string();

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.id = "test_received_1".to_string();

        db.insert_onchain_activity(&activity).unwrap();

        let is_used = db.is_address_used(&address).unwrap();
        assert!(is_used, "Address with received activity should return true");

        cleanup(&db_path);
    }

    #[test]
    fn test_is_address_used_with_sent_activity() {
        let (mut db, db_path) = setup();
        let address = "bc1qsent123".to_string();

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Sent;
        activity.id = "test_sent_1".to_string();

        db.insert_onchain_activity(&activity).unwrap();

        let is_used = db.is_address_used(&address).unwrap();
        assert!(is_used, "Address with sent activity should return true");

        cleanup(&db_path);
    }

    #[test]
    fn test_is_address_used_with_multiple_activities() {
        let (mut db, db_path) = setup();
        let address = "bc1qmultiple123".to_string();

        // Add received activity
        let mut received_activity = create_test_onchain_activity();
        received_activity.address = address.clone();
        received_activity.tx_type = PaymentType::Received;
        received_activity.id = "test_received_1".to_string();
        received_activity.confirmed = true;
        db.insert_onchain_activity(&received_activity).unwrap();

        // Add sent activity
        let mut sent_activity = create_test_onchain_activity();
        sent_activity.address = address.clone();
        sent_activity.tx_type = PaymentType::Sent;
        sent_activity.id = "test_sent_1".to_string();
        sent_activity.confirmed = false;
        db.insert_onchain_activity(&sent_activity).unwrap();

        let is_used = db.is_address_used(&address).unwrap();
        assert!(
            is_used,
            "Address with multiple activities should return true"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_is_address_used_with_unconfirmed_activity() {
        let (mut db, db_path) = setup();
        let address = "bc1qunconfirmed123".to_string();

        let mut activity = create_test_onchain_activity();
        activity.address = address.clone();
        activity.tx_type = PaymentType::Received;
        activity.id = "test_unconfirmed_1".to_string();
        activity.confirmed = false;

        db.insert_onchain_activity(&activity).unwrap();

        let is_used = db.is_address_used(&address).unwrap();
        assert!(
            is_used,
            "Address with unconfirmed activity should return true"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_is_address_used_different_addresses() {
        let (mut db, db_path) = setup();
        let used_address = "bc1qused123".to_string();
        let unused_address = "bc1qunused456".to_string();

        // Add activity for one address
        let mut activity = create_test_onchain_activity();
        activity.address = used_address.clone();
        activity.tx_type = PaymentType::Received;
        activity.id = "test_used_1".to_string();
        db.insert_onchain_activity(&activity).unwrap();

        // Check used address
        let is_used = db.is_address_used(&used_address).unwrap();
        assert!(is_used, "Address with activity should return true");

        // Check unused address
        let is_unused = db.is_address_used(&unused_address).unwrap();
        assert!(!is_unused, "Address without activity should return false");

        cleanup(&db_path);
    }

    #[test]
    fn test_is_address_used_only_onchain_activities() {
        let (mut db, db_path) = setup();
        let address = "bc1qonchain123".to_string();

        // Add lightning activity (should not affect the check)
        let lightning_activity = create_test_lightning_activity();
        db.insert_lightning_activity(&lightning_activity).unwrap();

        // Address should still be unused since no onchain activity
        let is_used = db.is_address_used(&address).unwrap();
        assert!(
            !is_used,
            "Address should return false if only lightning activities exist"
        );

        // Now add onchain activity
        let mut onchain_activity = create_test_onchain_activity();
        onchain_activity.address = address.clone();
        onchain_activity.tx_type = PaymentType::Received;
        onchain_activity.id = "test_onchain_1".to_string();
        db.insert_onchain_activity(&onchain_activity).unwrap();

        // Now it should be used
        let is_used_after = db.is_address_used(&address).unwrap();
        assert!(
            is_used_after,
            "Address should return true after onchain activity is added"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activity_by_tx_id_not_found() {
        let (db, db_path) = setup();
        let tx_id = "nonexistent_tx_id".to_string();

        let activity = db.get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id).unwrap();
        assert!(activity.is_none(), "Non-existent tx_id should return None");

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activity_by_tx_id_found() {
        let (mut db, db_path) = setup();
        let tx_id = "test_tx_id_123".to_string();

        let mut activity = create_test_onchain_activity();
        activity.tx_id = tx_id.clone();
        activity.id = "test_activity_1".to_string();

        db.insert_onchain_activity(&activity).unwrap();

        let retrieved = db.get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id).unwrap();
        assert!(retrieved.is_some(), "Activity should be found by tx_id");

        if let Some(retrieved_activity) = retrieved {
            assert_eq!(retrieved_activity.tx_id, tx_id);
            assert_eq!(retrieved_activity.id, activity.id);
            assert_eq!(retrieved_activity.value, activity.value);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activity_by_tx_id_multiple_activities() {
        let (mut db, db_path) = setup();
        let tx_id = "shared_tx_id".to_string();

        // Insert first activity
        let mut activity1 = create_test_onchain_activity();
        activity1.tx_id = tx_id.clone();
        activity1.id = "test_activity_1".to_string();
        activity1.value = 10000;
        db.insert_onchain_activity(&activity1).unwrap();

        // Insert second activity with same tx_id (shouldn't happen in practice, but test it)
        let mut activity2 = create_test_onchain_activity();
        activity2.tx_id = tx_id.clone();
        activity2.id = "test_activity_2".to_string();
        activity2.value = 20000;
        db.insert_onchain_activity(&activity2).unwrap();

        // Should return the first one found
        let retrieved = db.get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id).unwrap();
        assert!(retrieved.is_some(), "Activity should be found by tx_id");

        if let Some(retrieved_activity) = retrieved {
            assert_eq!(retrieved_activity.tx_id, tx_id);
            // Should return one of them (implementation dependent which one)
            assert!(retrieved_activity.id == activity1.id || retrieved_activity.id == activity2.id);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activity_by_tx_id_different_tx_ids() {
        let (mut db, db_path) = setup();
        let tx_id1 = "tx_id_1".to_string();
        let tx_id2 = "tx_id_2".to_string();

        let mut activity1 = create_test_onchain_activity();
        activity1.tx_id = tx_id1.clone();
        activity1.id = "test_activity_1".to_string();
        db.insert_onchain_activity(&activity1).unwrap();

        let mut activity2 = create_test_onchain_activity();
        activity2.tx_id = tx_id2.clone();
        activity2.id = "test_activity_2".to_string();
        db.insert_onchain_activity(&activity2).unwrap();

        // Get first activity
        let retrieved1 = db
            .get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id1)
            .unwrap();
        assert!(retrieved1.is_some(), "First activity should be found");
        if let Some(retrieved) = retrieved1 {
            assert_eq!(retrieved.tx_id, tx_id1);
            assert_eq!(retrieved.id, activity1.id);
        } else {
            panic!("Expected Onchain activity");
        }

        // Get second activity
        let retrieved2 = db
            .get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id2)
            .unwrap();
        assert!(retrieved2.is_some(), "Second activity should be found");
        if let Some(retrieved) = retrieved2 {
            assert_eq!(retrieved.tx_id, tx_id2);
            assert_eq!(retrieved.id, activity2.id);
        } else {
            panic!("Expected Onchain activity");
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_get_activity_by_tx_id_only_onchain() {
        let (mut db, db_path) = setup();
        let tx_id = "onchain_tx_id".to_string();

        // Add lightning activity (should not be found by tx_id)
        let lightning_activity = create_test_lightning_activity();
        db.insert_lightning_activity(&lightning_activity).unwrap();

        // Try to get by tx_id - should return None since lightning doesn't have tx_id
        let retrieved = db.get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id).unwrap();
        assert!(
            retrieved.is_none(),
            "Lightning activities should not be found by tx_id"
        );

        // Add onchain activity
        let mut onchain_activity = create_test_onchain_activity();
        onchain_activity.tx_id = tx_id.clone();
        onchain_activity.id = "test_onchain_1".to_string();
        db.insert_onchain_activity(&onchain_activity).unwrap();

        // Now should find it
        let retrieved = db.get_activity_by_tx_id(DEFAULT_WALLET_ID, &tx_id).unwrap();
        assert!(
            retrieved.is_some(),
            "Onchain activity should be found by tx_id"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_mark_activity_as_seen_onchain() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Verify initial state - seen_at should be None
        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
        assert!(
            retrieved.get_seen_at().is_none(),
            "seen_at should be None initially"
        );

        // Mark as seen
        let seen_timestamp = 1234567900u64;
        db.mark_activity_as_seen(DEFAULT_WALLET_ID, &activity.id, seen_timestamp)
            .unwrap();

        // Verify seen_at is now set
        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            retrieved.get_seen_at(),
            Some(seen_timestamp),
            "seen_at should be set"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_mark_activity_as_seen_lightning() {
        let (mut db, db_path) = setup();
        let activity = create_test_lightning_activity();
        db.insert_lightning_activity(&activity).unwrap();

        // Verify initial state - seen_at should be None
        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
        assert!(
            retrieved.get_seen_at().is_none(),
            "seen_at should be None initially"
        );

        // Mark as seen
        let seen_timestamp = 1234567900u64;
        db.mark_activity_as_seen(DEFAULT_WALLET_ID, &activity.id, seen_timestamp)
            .unwrap();

        // Verify seen_at is now set
        let retrieved = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &activity.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            retrieved.get_seen_at(),
            Some(seen_timestamp),
            "seen_at should be set"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_mark_activity_as_seen_nonexistent() {
        let (mut db, db_path) = setup();

        // Try to mark a non-existent activity as seen
        let result = db.mark_activity_as_seen(DEFAULT_WALLET_ID, "nonexistent_id", 1234567900);
        assert!(result.is_err(), "Should fail for non-existent activity");

        cleanup(&db_path);
    }

    #[test]
    fn test_seen_at_preserved_in_get_activities() {
        let (mut db, db_path) = setup();

        // Insert two activities
        let mut onchain = create_test_onchain_activity();
        onchain.timestamp = 1000;
        let mut lightning = create_test_lightning_activity();
        lightning.timestamp = 2000;

        db.insert_onchain_activity(&onchain).unwrap();
        db.insert_lightning_activity(&lightning).unwrap();

        // Mark only onchain as seen
        let seen_timestamp = 3000u64;
        db.mark_activity_as_seen(DEFAULT_WALLET_ID, &onchain.id, seen_timestamp)
            .unwrap();

        // Get all activities
        let activities = db
            .get_activities(None, None, None, None, None, None, None, None, None)
            .unwrap();
        assert_eq!(activities.len(), 2);

        for activity in activities {
            match activity {
                Activity::Onchain(o) => {
                    assert_eq!(
                        o.seen_at,
                        Some(seen_timestamp),
                        "Onchain should have seen_at set"
                    );
                }
                Activity::Lightning(l) => {
                    assert!(l.seen_at.is_none(), "Lightning should not have seen_at set");
                }
            }
        }

        cleanup(&db_path);
    }

    #[test]
    fn test_seen_at_preserved_in_get_activity_by_tx_id() {
        let (mut db, db_path) = setup();
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        // Mark as seen
        let seen_timestamp = 1234567900u64;
        db.mark_activity_as_seen(DEFAULT_WALLET_ID, &activity.id, seen_timestamp)
            .unwrap();

        // Retrieve by tx_id and verify seen_at
        let retrieved = db
            .get_activity_by_tx_id(DEFAULT_WALLET_ID, &activity.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            retrieved.seen_at,
            Some(seen_timestamp),
            "seen_at should be preserved when getting by tx_id"
        );

        cleanup(&db_path);
    }

    fn create_test_transaction_details() -> TransactionDetails {
        TransactionDetails {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            tx_id: "tx123abc".to_string(),
            amount_sats: 50000,
            inputs: vec![TxInput {
                txid: "prev_tx_abc".to_string(),
                vout: 0,
                scriptsig: "00".to_string(),
                witness: vec!["witness1".to_string(), "witness2".to_string()],
                sequence: 0xffffffff,
            }],
            outputs: vec![
                TxOutput {
                    scriptpubkey: "0014abc123".to_string(),
                    scriptpubkey_type: Some("p2wpkh".to_string()),
                    scriptpubkey_address: Some("bc1qtest...".to_string()),
                    value: 45000,
                    n: 0,
                },
                TxOutput {
                    scriptpubkey: "0014def456".to_string(),
                    scriptpubkey_type: Some("p2wpkh".to_string()),
                    scriptpubkey_address: Some("bc1qchange...".to_string()),
                    value: 4500,
                    n: 1,
                },
            ],
        }
    }

    #[test]
    fn test_upsert_and_get_transaction_details() {
        let (mut db, db_path) = setup();
        let details = create_test_transaction_details();

        // Upsert
        db.upsert_transaction_details(&[details.clone()]).unwrap();

        // Retrieve
        let retrieved = db
            .get_transaction_details(DEFAULT_WALLET_ID, &details.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.tx_id, details.tx_id);
        assert_eq!(retrieved.amount_sats, details.amount_sats);
        assert_eq!(retrieved.inputs.len(), 1);
        assert_eq!(retrieved.outputs.len(), 2);
        assert_eq!(retrieved.inputs[0].txid, "prev_tx_abc");
        assert_eq!(retrieved.outputs[0].value, 45000);

        cleanup(&db_path);
    }

    #[test]
    fn test_transaction_details_not_found() {
        let (db, db_path) = setup();

        let retrieved = db
            .get_transaction_details(DEFAULT_WALLET_ID, "nonexistent_tx")
            .unwrap();
        assert!(retrieved.is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_transaction_details_updates_existing() {
        let (mut db, db_path) = setup();
        let mut details = create_test_transaction_details();

        // Initial insert
        db.upsert_transaction_details(&[details.clone()]).unwrap();

        // Update with new amount
        details.amount_sats = 100000;
        db.upsert_transaction_details(&[details.clone()]).unwrap();

        // Verify update
        let retrieved = db
            .get_transaction_details(DEFAULT_WALLET_ID, &details.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.amount_sats, 100000);

        cleanup(&db_path);
    }

    #[test]
    fn test_transaction_details_are_wallet_scoped() {
        let (mut db, db_path) = setup();
        let wallet_id = "hardware-wallet-1";

        let mut main_details = create_test_transaction_details();
        main_details.tx_id = "shared_details_txid".to_string();
        main_details.amount_sats = 10_000;

        let mut hardware_details = create_test_transaction_details();
        hardware_details.wallet_id = wallet_id.to_string();
        hardware_details.tx_id = "shared_details_txid".to_string();
        hardware_details.amount_sats = -25_000;

        db.upsert_transaction_details(&[main_details.clone(), hardware_details.clone()])
            .unwrap();

        let main = db
            .get_transaction_details(DEFAULT_WALLET_ID, "shared_details_txid")
            .unwrap()
            .unwrap();
        let hardware = db
            .get_transaction_details(wallet_id, "shared_details_txid")
            .unwrap()
            .unwrap();

        assert_eq!(main.amount_sats, main_details.amount_sats);
        assert_eq!(hardware.amount_sats, hardware_details.amount_sats);

        db.delete_transaction_details(wallet_id, "shared_details_txid")
            .unwrap();

        assert!(db
            .get_transaction_details(wallet_id, "shared_details_txid")
            .unwrap()
            .is_none());
        assert!(db
            .get_transaction_details(DEFAULT_WALLET_ID, "shared_details_txid")
            .unwrap()
            .is_some());

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_transaction_details() {
        let (mut db, db_path) = setup();
        let details = create_test_transaction_details();

        db.upsert_transaction_details(&[details.clone()]).unwrap();

        // Delete
        let deleted = db
            .delete_transaction_details(DEFAULT_WALLET_ID, &details.tx_id)
            .unwrap();
        assert!(deleted);

        // Verify deletion
        let retrieved = db
            .get_transaction_details(DEFAULT_WALLET_ID, &details.tx_id)
            .unwrap();
        assert!(retrieved.is_none());

        cleanup(&db_path);
    }

    #[test]
    fn test_delete_nonexistent_transaction_details() {
        let (mut db, db_path) = setup();

        let deleted = db
            .delete_transaction_details(DEFAULT_WALLET_ID, "nonexistent_tx")
            .unwrap();
        assert!(!deleted);

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_transaction_details_multiple() {
        let (mut db, db_path) = setup();

        let details1 = create_test_transaction_details();
        let mut details2 = create_test_transaction_details();
        details2.tx_id = "tx456def".to_string();
        details2.amount_sats = -25000; // Outgoing

        db.upsert_transaction_details(&[details1.clone(), details2.clone()])
            .unwrap();

        // Verify both were inserted
        let all = db.get_all_transaction_details().unwrap();
        assert_eq!(all.len(), 2);

        let retrieved1 = db
            .get_transaction_details(DEFAULT_WALLET_ID, &details1.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved1.amount_sats, 50000);

        let retrieved2 = db
            .get_transaction_details(DEFAULT_WALLET_ID, &details2.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved2.amount_sats, -25000);

        cleanup(&db_path);
    }

    #[test]
    fn test_get_all_transaction_details() {
        let (mut db, db_path) = setup();

        // Initially empty
        let all = db.get_all_transaction_details().unwrap();
        assert!(all.is_empty());

        // Add some
        let details1 = create_test_transaction_details();
        let mut details2 = create_test_transaction_details();
        details2.tx_id = "tx789ghi".to_string();

        db.upsert_transaction_details(&[details1, details2])
            .unwrap();

        let all = db.get_all_transaction_details().unwrap();
        assert_eq!(all.len(), 2);

        cleanup(&db_path);
    }

    #[test]
    fn test_wipe_all_transaction_details() {
        let (mut db, db_path) = setup();

        let details1 = create_test_transaction_details();
        let mut details2 = create_test_transaction_details();
        details2.tx_id = "tx999xyz".to_string();

        db.upsert_transaction_details(&[details1, details2])
            .unwrap();

        // Wipe all
        db.wipe_all_transaction_details().unwrap();

        let all = db.get_all_transaction_details().unwrap();
        assert!(all.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_transaction_details_empty_tx_id_fails() {
        let (mut db, db_path) = setup();

        let mut details = create_test_transaction_details();
        details.tx_id = "".to_string();

        let result = db.upsert_transaction_details(&[details]);
        assert!(result.is_err());

        cleanup(&db_path);
    }

    #[test]
    fn test_transaction_details_complex_witness() {
        let (mut db, db_path) = setup();

        let details = TransactionDetails {
            wallet_id: DEFAULT_WALLET_ID.to_string(),
            tx_id: "tx_with_complex_witness".to_string(),
            amount_sats: 10000,
            inputs: vec![TxInput {
                txid: "prev_tx".to_string(),
                vout: 1,
                scriptsig: "".to_string(),
                witness: vec![
                    "304402...".to_string(),
                    "02abc...".to_string(),
                    "c0...".to_string(),
                ],
                sequence: 0xfffffffd,
            }],
            outputs: vec![TxOutput {
                scriptpubkey: "5120...".to_string(),
                scriptpubkey_type: Some("p2tr".to_string()),
                scriptpubkey_address: Some("bc1p...".to_string()),
                value: 9500,
                n: 0,
            }],
        };

        db.upsert_transaction_details(&[details.clone()]).unwrap();

        let retrieved = db
            .get_transaction_details(DEFAULT_WALLET_ID, &details.tx_id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.inputs[0].witness.len(), 3);
        assert_eq!(
            retrieved.outputs[0].scriptpubkey_type,
            Some("p2tr".to_string())
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_wipe_all_includes_transaction_details() {
        let (mut db, db_path) = setup();

        // Add activity and transaction details
        let activity = create_test_onchain_activity();
        db.insert_onchain_activity(&activity).unwrap();

        let details = create_test_transaction_details();
        db.upsert_transaction_details(&[details]).unwrap();

        // Wipe all
        db.wipe_all().unwrap();

        // Verify transaction details are also wiped
        let all = db.get_all_transaction_details().unwrap();
        assert!(all.is_empty());

        cleanup(&db_path);
    }

    #[test]
    fn test_upsert_preserves_contact_and_unconfirmed_timestamp() {
        let (mut db, db_path) = setup();

        // Seed: unconfirmed activity with a user-set contact and first-seen timestamp.
        let mut seed = create_test_onchain_activity();
        seed.confirmed = false;
        seed.timestamp = 1_000;
        seed.contact = Some("npub_alice".to_string());
        db.insert_onchain_activity(&seed).unwrap();

        // Watcher refresh: still unconfirmed, no contact known, fresh "now" timestamp.
        let mut refresh = seed.clone();
        refresh.contact = None;
        refresh.timestamp = 9_999;
        db.upsert_activity(&Activity::Onchain(refresh)).unwrap();

        let got = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &seed.id)
            .unwrap()
            .unwrap();
        let Activity::Onchain(o) = got else {
            panic!("expected onchain")
        };
        assert_eq!(
            o.contact.as_deref(),
            Some("npub_alice"),
            "contact must survive a None refresh"
        );
        assert_eq!(
            o.timestamp, 1_000,
            "unconfirmed timestamp must not churn on refresh"
        );

        // Once confirmed, the block timestamp is applied.
        let mut confirmed = seed.clone();
        confirmed.contact = None;
        confirmed.confirmed = true;
        confirmed.timestamp = 2_000;
        db.upsert_activity(&Activity::Onchain(confirmed)).unwrap();

        let got = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &seed.id)
            .unwrap()
            .unwrap();
        let Activity::Onchain(o) = got else {
            panic!("expected onchain")
        };
        assert_eq!(
            o.timestamp, 2_000,
            "confirmed tx adopts the block timestamp"
        );
        assert_eq!(
            o.contact.as_deref(),
            Some("npub_alice"),
            "contact still preserved"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_update_activity_is_pure_replacement() {
        let (mut db, db_path) = setup();

        let mut seed = create_test_onchain_activity();
        seed.confirmed = false;
        seed.timestamp = 1_000;
        seed.contact = Some("npub_alice".to_string());
        db.insert_onchain_activity(&seed).unwrap();

        // update_* is a literal replacement: it clears the contact (None) and
        // moves the timestamp even while the tx is unconfirmed.
        let mut replaced = seed.clone();
        replaced.contact = None;
        replaced.timestamp = 5_000;
        replaced.confirmed = false;
        db.update_onchain_activity_by_id(&seed.id, &replaced)
            .unwrap();

        let got = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &seed.id)
            .unwrap()
            .unwrap();
        let Activity::Onchain(o) = got else {
            panic!("expected onchain")
        };
        assert_eq!(o.contact, None, "update clears contact");
        assert_eq!(
            o.timestamp, 5_000,
            "update moves timestamp even when unconfirmed"
        );

        cleanup(&db_path);
    }

    #[test]
    fn test_batch_upsert_preserves_contact_and_unconfirmed_timestamp() {
        let (mut db, db_path) = setup();

        let mut seed = create_test_onchain_activity();
        seed.confirmed = false;
        seed.timestamp = 1_000;
        seed.contact = Some("npub_bob".to_string());
        db.insert_onchain_activity(&seed).unwrap();

        // Batch path (the watcher refresh entry point): same tx, no contact, new now.
        let mut refresh = seed.clone();
        refresh.contact = None;
        refresh.timestamp = 9_999;
        db.upsert_onchain_activities(&[refresh]).unwrap();

        let got = db
            .get_activity_by_id(DEFAULT_WALLET_ID, &seed.id)
            .unwrap()
            .unwrap();
        let Activity::Onchain(o) = got else {
            panic!("expected onchain")
        };
        assert_eq!(o.contact.as_deref(), Some("npub_bob"));
        assert_eq!(o.timestamp, 1_000);

        cleanup(&db_path);
    }

    #[test]
    fn test_derive_wallet_id_is_order_independent() {
        use crate::activity::derive_wallet_id;

        let a = derive_wallet_id(
            "trezor".to_string(),
            vec![
                "xpubA".to_string(),
                "xpubB".to_string(),
                "xpubC".to_string(),
            ],
        );
        let b = derive_wallet_id(
            "trezor".to_string(),
            vec![
                "xpubC".to_string(),
                "xpubA".to_string(),
                "xpubB".to_string(),
            ],
        );
        assert_eq!(a.unwrap(), b.unwrap());
    }

    #[test]
    fn test_derive_wallet_id_device_type_changes_id() {
        use crate::activity::derive_wallet_id;

        let xpubs = vec!["xpubA".to_string(), "xpubB".to_string()];
        let trezor = derive_wallet_id("trezor".to_string(), xpubs.clone()).unwrap();
        let ledger = derive_wallet_id("ledger".to_string(), xpubs).unwrap();

        assert_ne!(trezor, ledger);
        assert!(trezor.starts_with("trezor:"));
        assert!(ledger.starts_with("ledger:"));
    }

    #[test]
    fn test_derive_wallet_id_known_vector() {
        use crate::activity::derive_wallet_id;
        use bitcoin::hashes::{sha256, Hash};

        // Canonical form: sort lexicographically, join with "\n", SHA256, hex.
        let expected_hash = hex::encode(sha256::Hash::hash(b"xpubA\nxpubB").to_byte_array());
        let id = derive_wallet_id(
            "trezor".to_string(),
            vec!["xpubB".to_string(), "xpubA".to_string()],
        )
        .unwrap();
        assert_eq!(id, format!("trezor:{}", expected_hash));
    }

    #[test]
    fn test_derive_wallet_id_rejects_empty_and_blank_input() {
        use crate::activity::derive_wallet_id;

        // Empty xpubs must not collapse every device of a type into one id.
        assert!(derive_wallet_id("trezor".to_string(), vec![]).is_err());
        // Blank entries are rejected too.
        assert!(derive_wallet_id("trezor".to_string(), vec!["".to_string()]).is_err());
        // Blank device_type is rejected.
        assert!(derive_wallet_id("".to_string(), vec!["xpubA".to_string()]).is_err());
    }
}
