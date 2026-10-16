use super::{
    Activity, ActivityDB, OnchainActivity, PaymentType, PreActivityMetadata, DEFAULT_WALLET_ID,
};

fn original() -> OnchainActivity {
    OnchainActivity {
        wallet_id: DEFAULT_WALLET_ID.into(),
        id: "original-activity".into(),
        tx_type: PaymentType::Sent,
        tx_id: "original-tx".into(),
        value: 10000,
        fee: 100,
        fee_rate: 1,
        address: "recipient".into(),
        confirmed: false,
        timestamp: 100,
        is_boosted: false,
        boost_tx_ids: vec![],
        is_transfer: false,
        does_exist: true,
        confirm_timestamp: None,
        channel_id: None,
        transfer_tx_id: None,
        contact: None,
        created_at: None,
        updated_at: None,
        seen_at: None,
    }
}

fn replacement() -> OnchainActivity {
    OnchainActivity {
        id: "replacement-activity".into(),
        tx_id: "replacement-tx".into(),
        fee: 2500,
        ..original()
    }
}

fn setup() -> (ActivityDB, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("activity.sqlite");
    let mut db = ActivityDB::new(path.to_str().unwrap()).unwrap();
    db.insert_onchain_activity(&original()).unwrap();
    (db, directory)
}

fn pending_metadata() -> PreActivityMetadata {
    PreActivityMetadata {
        wallet_id: DEFAULT_WALLET_ID.into(),
        payment_id: "replacement-tx".into(),
        tags: vec!["pending-tag".into()],
        payment_hash: None,
        tx_id: Some("replacement-tx".into()),
        address: None,
        is_receive: false,
        fee_rate: 3,
        is_transfer: false,
        channel_id: None,
        created_at: 100,
    }
}

fn record(db: &mut ActivityDB) {
    db.record_rbf_boost(DEFAULT_WALLET_ID, "original-activity", "replacement-tx", 25)
        .unwrap();
}

#[test]
fn replacement_fee_survives_creation_stale_sync_confirmation_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("activity.sqlite");
    let mut db = ActivityDB::new(path.to_str().unwrap()).unwrap();
    db.insert_onchain_activity(&original()).unwrap();
    record(&mut db);
    assert_eq!(
        db.get_pre_activity_metadata(DEFAULT_WALLET_ID, "replacement-tx", false)
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
    drop(db);

    let mut db = ActivityDB::new(path.to_str().unwrap()).unwrap();
    let mut stale = replacement();
    db.upsert_onchain_activity_preserving_fee_rate(&stale)
        .unwrap();
    assert!(db
        .get_pre_activity_metadata(DEFAULT_WALLET_ID, "replacement-tx", false)
        .unwrap()
        .is_none());
    stale.confirmed = true;
    stale.confirm_timestamp = Some(200);
    stale.id = "different-payment-id".into();
    db.upsert_onchain_activity_preserving_fee_rate(&stale)
        .unwrap();
    let stored = db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
        .unwrap()
        .unwrap();
    assert_eq!(stored.id, "replacement-activity");
    assert_eq!(stored.fee_rate, 25);
    assert_eq!(stored.fee, 2500);
    assert!(stored.confirmed);
    assert_eq!(stored.confirm_timestamp, Some(200));
    drop(db);
    let db = ActivityDB::new(path.to_str().unwrap()).unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
}

#[test]
fn recording_after_replacement_arrives_preserves_other_fields_and_tags() {
    let (mut db, _directory) = setup();
    let mut activity = replacement();
    activity.contact = Some("contact".into());
    activity.is_boosted = true;
    activity.boost_tx_ids = vec!["original-tx".into()];
    db.insert_onchain_activity(&activity).unwrap();
    db.mark_activity_as_seen(DEFAULT_WALLET_ID, &activity.id, 123)
        .unwrap();
    db.add_tags(DEFAULT_WALLET_ID, &activity.id, &["existing-tag".into()])
        .unwrap();
    db.add_pre_activity_metadata(&pending_metadata()).unwrap();
    record(&mut db);
    let stored = db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
        .unwrap()
        .unwrap();
    assert_eq!(stored.fee_rate, 25);
    assert_eq!(stored.fee, 2500);
    assert_eq!(stored.contact, activity.contact);
    assert_eq!(stored.seen_at, Some(123));
    assert_eq!(stored.boost_tx_ids, activity.boost_tx_ids);
    assert!(stored.is_boosted);
    let mut tags = db.get_tags(DEFAULT_WALLET_ID, &activity.id).unwrap();
    tags.sort();
    assert_eq!(tags, vec!["existing-tag", "pending-tag"]);
    assert!(db
        .get_pre_activity_metadata(DEFAULT_WALLET_ID, "replacement-tx", false)
        .unwrap()
        .is_none());
    db.upsert_onchain_activity_preserving_fee_rate(&replacement())
        .unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
}

#[test]
fn pending_tags_survive_recording_before_replacement_arrives() {
    let (mut db, _directory) = setup();
    db.add_pre_activity_metadata(&pending_metadata()).unwrap();
    record(&mut db);
    let metadata = db
        .get_pre_activity_metadata(DEFAULT_WALLET_ID, "replacement-tx", false)
        .unwrap()
        .unwrap();
    assert_eq!(metadata.tags, vec!["pending-tag"]);
    assert_eq!(metadata.fee_rate, 25);
    db.upsert_onchain_activity_preserving_fee_rate(&replacement())
        .unwrap();
    assert_eq!(
        db.get_tags(DEFAULT_WALLET_ID, "replacement-activity")
            .unwrap(),
        vec!["pending-tag"]
    );
}

#[test]
fn removed_original_is_not_resurrected_by_late_boost_completion() {
    let (mut db, _directory) = setup();
    let mut removed = original();
    removed.does_exist = false;
    db.update_onchain_activity_by_id(&removed.id, &removed)
        .unwrap();
    record(&mut db);
    let stored = db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "original-tx")
        .unwrap()
        .unwrap();
    assert!(!stored.does_exist);
    assert!(!stored.is_boosted);
    db.upsert_onchain_activity_preserving_fee_rate(&replacement())
        .unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
}

#[test]
fn recording_failure_rolls_back_original_rate_pending_metadata_and_replacement() {
    let (mut db, _directory) = setup();
    db.insert_onchain_activity(&replacement()).unwrap();
    db.add_pre_activity_metadata(&pending_metadata()).unwrap();
    db.conn
        .execute_batch(
            "CREATE TRIGGER fail_replacement_fee BEFORE UPDATE OF fee_rate ON onchain_activity
        WHEN OLD.tx_id = 'replacement-tx' BEGIN SELECT RAISE(ABORT, 'forced write failure'); END;",
        )
        .unwrap();
    assert!(db
        .record_rbf_boost(DEFAULT_WALLET_ID, "original-activity", "replacement-tx", 25)
        .is_err());
    let original = db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "original-tx")
        .unwrap()
        .unwrap();
    assert_eq!(original.fee_rate, 1);
    assert!(!original.is_boosted);
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        1
    );
    assert_eq!(
        db.get_pre_activity_metadata(DEFAULT_WALLET_ID, "replacement-tx", false)
            .unwrap()
            .unwrap()
            .fee_rate,
        3
    );
    assert!(db
        .get_tags(DEFAULT_WALLET_ID, "replacement-activity")
        .unwrap()
        .is_empty());
}

#[test]
fn sync_metadata_failure_rolls_back_activity_creation_and_keeps_pending_rate() {
    let (mut db, _directory) = setup();
    record(&mut db);
    db.conn
        .execute_batch(
            "CREATE TRIGGER fail_metadata_delete BEFORE DELETE ON pre_activity_metadata
        BEGIN SELECT RAISE(ABORT, 'forced delete failure'); END;",
        )
        .unwrap();
    assert!(db
        .upsert_onchain_activity_preserving_fee_rate(&replacement())
        .is_err());
    assert!(db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
        .unwrap()
        .is_none());
    assert_eq!(
        db.get_pre_activity_metadata(DEFAULT_WALLET_ID, "replacement-tx", false)
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
    db.conn
        .execute_batch("DROP TRIGGER fail_metadata_delete")
        .unwrap();
    db.upsert_onchain_activity_preserving_fee_rate(&replacement())
        .unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
}

#[test]
fn recording_and_preserving_rates_are_wallet_scoped() {
    let (mut db, _directory) = setup();
    let mut other_original = original();
    other_original.wallet_id = "hardware-wallet".into();
    let mut other_replacement = replacement();
    other_replacement.wallet_id = other_original.wallet_id.clone();
    other_replacement.fee_rate = 7;
    db.insert_onchain_activity(&other_original).unwrap();
    db.insert_onchain_activity(&other_replacement).unwrap();
    record(&mut db);
    db.upsert_onchain_activity_preserving_fee_rate(&replacement())
        .unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        25
    );
    assert_eq!(
        db.get_activity_by_tx_id("hardware-wallet", "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        7
    );
    assert!(
        !db.get_activity_by_tx_id("hardware-wallet", "original-tx")
            .unwrap()
            .unwrap()
            .is_boosted
    );
    assert!(db
        .get_pre_activity_metadata("hardware-wallet", "replacement-tx", false)
        .unwrap()
        .is_none());
}

#[test]
fn full_updates_and_upserts_still_allow_deliberate_fee_corrections() {
    let (mut db, _directory) = setup();
    db.insert_onchain_activity(&replacement()).unwrap();
    record(&mut db);
    let mut correction = replacement();
    correction.fee_rate = 13;
    db.upsert_activity(&Activity::Onchain(correction.clone()))
        .unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        13
    );
    correction.fee_rate = 7;
    db.update_onchain_activity_by_id(&correction.id, &correction)
        .unwrap();
    db.upsert_onchain_activity_preserving_fee_rate(&replacement())
        .unwrap();
    assert_eq!(
        db.get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
            .unwrap()
            .unwrap()
            .fee_rate,
        7
    );
}

#[test]
fn invalid_record_requests_do_not_write_anything() {
    let (mut db, _directory) = setup();
    for (wallet, original, replacement, rate) in [
        (DEFAULT_WALLET_ID, "original-activity", "replacement-tx", 0),
        (
            DEFAULT_WALLET_ID,
            "original-activity",
            "replacement-tx",
            u64::MAX,
        ),
        (" ", "original-activity", "replacement-tx", 25),
        ("other-wallet", "original-activity", "replacement-tx", 25),
        (DEFAULT_WALLET_ID, "missing", "replacement-tx", 25),
        (DEFAULT_WALLET_ID, "original-activity", "original-tx", 25),
        (DEFAULT_WALLET_ID, "original-activity", " ", 25),
    ] {
        assert!(db
            .record_rbf_boost(wallet, original, replacement, rate)
            .is_err());
    }
    let original = db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "original-tx")
        .unwrap()
        .unwrap();
    assert!(!original.is_boosted);
    assert_eq!(original.fee_rate, 1);
    assert!(db.get_all_pre_activity_metadata().unwrap().is_empty());
}

#[test]
fn sync_id_collision_cannot_overwrite_another_transaction() {
    let (mut db, _directory) = setup();
    let mut collision = replacement();
    collision.id = original().id;
    assert!(db
        .upsert_onchain_activity_preserving_fee_rate(&collision)
        .is_err());
    assert!(db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "original-tx")
        .unwrap()
        .is_some());
    assert!(db
        .get_activity_by_tx_id(DEFAULT_WALLET_ID, "replacement-tx")
        .unwrap()
        .is_none());
}
