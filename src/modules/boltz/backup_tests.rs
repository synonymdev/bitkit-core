use super::*;
use crate::modules::boltz::tests::{build_test_invoice, fixture_keys, submarine_response_fixture};
use bitcoin::hashes::Hash;

fn legacy_record() -> SwapRecord {
    let (key, preimage, provider) = fixture_keys();
    let response = submarine_response_fixture(preimage.hash160, &key, &provider);
    SwapRecord {
        id: response.id.clone(),
        backend_binding: None,
        swap_type: BoltzSwapType::Submarine,
        status: "invoice.set".into(),
        network: BoltzNetwork::Testnet,
        electrum_url: "ssl://electrum.example:50002".into(),
        swap_index: 0,
        invoice: Some(build_test_invoice(preimage.sha256, 100_000_000)),
        lockup_address: Some(response.address),
        onchain_address: None,
        amount_sat: 100_000,
        recipient_amount_sat: None,
        onchain_amount_sat: None,
        timeout_block_height: 800_000,
        create_response_json: serde_json::to_string(&submarine_response_fixture(
            preimage.hash160,
            &key,
            &provider,
        ))
        .unwrap(),
        claim_tx_id: None,
        refund_tx_id: None,
        created_at: 1,
    }
}

async fn database(path: &Path) -> BoltzDB {
    BoltzDB::new(path.to_str().unwrap()).await.unwrap()
}
fn root(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn logical_backup_roundtrip_keeps_newer_progress_and_counter() {
    let directory = tempfile::tempdir().unwrap();
    let source = database(&directory.path().join("source.db")).await;
    assert_eq!(source.reserve_swap_index().await.unwrap(), 0);
    let record = legacy_record();
    source.insert_swap(&record).await.unwrap();
    let snapshot = export_backup(&source, root(&directory.path().join("source-identities")))
        .await
        .unwrap();
    let target = database(&directory.path().join("target.db")).await;
    restore_backup(
        &target,
        snapshot.clone(),
        root(&directory.path().join("target-identities")),
    )
    .await
    .unwrap();
    let destination = record.lockup_address.clone().unwrap();
    target
        .set_refund_tx(&record.id, &"ab".repeat(32), &destination)
        .await
        .unwrap();
    for _ in 0..6 {
        target.reserve_swap_index().await.unwrap();
    }
    restore_backup(
        &target,
        snapshot,
        root(&directory.path().join("target-identities")),
    )
    .await
    .unwrap();
    let retained = target.get_swap(&record.id).await.unwrap().unwrap();
    assert_eq!(retained.refund_tx_id, Some("ab".repeat(32)));
    assert_eq!(retained.status, "transaction.refunded");
    assert_eq!(target.reserve_swap_index().await.unwrap(), 7);
}

#[tokio::test]
async fn corrupt_restore_is_rejected_before_existing_records_change() {
    let directory = tempfile::tempdir().unwrap();
    let db = database(&directory.path().join("core.db")).await;
    db.reserve_swap_index().await.unwrap();
    let record = legacy_record();
    db.insert_swap(&record).await.unwrap();
    let data_root = root(&directory.path().join("identities"));
    let snapshot = export_backup(&db, data_root.clone()).await.unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    value["core"]["swaps"][0]["amount_sat"] = serde_json::json!(1);
    assert!(restore_backup(&db, value.to_string(), data_root.clone())
        .await
        .is_err());
    value = serde_json::from_str(&snapshot).unwrap();
    value["core"]["next_swap_index"] = serde_json::json!(0);
    assert!(restore_backup(&db, value.to_string(), data_root.clone())
        .await
        .is_err());
    assert!(restore_backup(&db, " ".repeat(MAX_BYTES + 1), data_root)
        .await
        .is_err());
    assert_eq!(
        db.get_swap(&record.id).await.unwrap().unwrap().amount_sat,
        100_000
    );
    assert_eq!(db.list_swaps().await.unwrap().len(), 1);
}

#[tokio::test]
async fn disconnected_identity_stores_and_pending_intent_roundtrip() {
    use pubky_swap_boltz::model::ReverseRequest;
    let directory = tempfile::tempdir().unwrap();
    let source_root = directory.path().join("source-identities");
    let target_root = directory.path().join("target-identities");
    let source = database(&directory.path().join("source.db")).await;
    let identities = [
        pubky_swap_boltz::identity_from_secret(&[1; 32]),
        pubky_swap_boltz::identity_from_secret(&[2; 32]),
    ];
    let provider = pubky_swap_boltz::identity_from_secret(&[3; 32]);
    for identity in &identities {
        let binding = format!("{identity}|{provider}|testnet");
        drop(Store::open(&source_root.join(identity), &binding).unwrap());
    }
    let binding = format!("{}|{provider}|testnet", identities[0]);
    let (key, preimage, _) = fixture_keys();
    let request = CreateRequest::Reverse(ReverseRequest {
        from: "BTC".into(),
        to: "BTC".into(),
        preimage_hash: preimage.sha256.to_string(),
        claim_public_key: key.to_string(),
        invoice_amount: 100_000,
        onchain_amount: 0,
        pair_hash: String::new(),
        referral_id: String::new(),
        error: String::new(),
    });
    let intent = CreationIntent {
        id: uuid::Uuid::new_v4().to_string(),
        request_key: "request-key".into(),
        wallet_fingerprint: sha256::Hash::hash(&[9; 32]).to_string(),
        backend_binding: binding.clone(),
        network: BoltzNetwork::Testnet,
        electrum_url: "ssl://electrum.example:50002".into(),
        swap_index: source.reserve_swap_index().await.unwrap(),
        recipient_amount_sat: None,
        claim_address: legacy_record().lockup_address,
        created_at: 1,
        request: request.clone(),
    };
    source.save_intent(&intent).await.unwrap();
    {
        let store = Store::open(&source_root.join(&identities[0]), &binding).unwrap();
        store
            .reserve(request, preimage.sha256.to_string(), Some(&intent.id))
            .unwrap();
    }
    let snapshot = export_backup(&source, root(&source_root)).await.unwrap();
    assert!(!snapshot.contains("secret_key"));
    assert!(!snapshot.contains("\"preimage\""));
    let target = database(&directory.path().join("target.db")).await;
    let mut extra: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    extra["core"]["intents"][0]["request"]["unknownPrivateField"] = serde_json::json!("rejected");
    assert!(
        restore_backup(&target, extra.to_string(), root(&target_root))
            .await
            .is_err()
    );
    let mut contradictory: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    contradictory["core"]["intents"][0]["request"]["request"]["invoiceAmount"] =
        serde_json::json!(100_001);
    assert!(
        restore_backup(&target, contradictory.to_string(), root(&target_root))
            .await
            .is_err()
    );
    restore_backup(&target, snapshot.clone(), root(&target_root))
        .await
        .unwrap();
    restore_backup(&target, snapshot, root(&target_root))
        .await
        .unwrap();
    let restored = target.creation_intents().await.unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, intent.id);
    assert_eq!(restored[0].swap_index, intent.swap_index);
    assert_eq!(target.reserve_swap_index().await.unwrap(), 1);
    let wrapper = Store::snapshot_directory(&target_root.join(&identities[0])).unwrap();
    assert_eq!(wrapper.idempotency[0].key, intent.id);
    assert_eq!(wrapper.swaps.len(), 1);
    assert!(Store::snapshot_directory(&target_root.join(&identities[1]))
        .unwrap()
        .swaps
        .is_empty());
}

#[tokio::test]
async fn export_fails_for_corrupt_identity_data_and_restore_rejects_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let db = database(&directory.path().join("core.db")).await;
    let data_root = directory.path().join("identities");
    let identity = pubky_swap_boltz::identity_from_secret(&[1; 32]);
    std::fs::create_dir_all(data_root.join(&identity)).unwrap();
    std::fs::write(data_root.join(&identity).join("swaps.sqlite3"), b"corrupt").unwrap();
    assert!(export_backup(&db, root(&data_root)).await.is_err());
    let snapshot = Snapshot {
        version: VERSION,
        core: db.core_snapshot().await.unwrap(),
        identities: vec![],
    };
    assert!(restore_backup(
        &db,
        serde_json::to_string(&snapshot).unwrap(),
        format!("{}/../escape", directory.path().display())
    )
    .await
    .is_err());
    assert!(validate_identity("../../escape").is_err());
}

#[tokio::test]
async fn restore_rejects_an_active_swap_without_waiting_for_network() {
    let directory = tempfile::tempdir().unwrap();
    let db = database(&directory.path().join("core.db")).await;
    let data_root = root(&directory.path().join("identities"));
    let snapshot = export_backup(&db, data_root.clone()).await.unwrap();
    let _active = db.recovery_gate.read().await;
    let export = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        export_backup(&db, data_root.clone()),
    )
    .await;
    assert!(export.unwrap().is_err());
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        restore_backup(&db, snapshot, data_root),
    )
    .await;
    assert!(result.unwrap().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn identity_symlinks_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let db = database(&directory.path().join("core.db")).await;
    let data_root = directory.path().join("identities");
    std::fs::create_dir_all(&data_root).unwrap();
    let identity = pubky_swap_boltz::identity_from_secret(&[1; 32]);
    std::os::unix::fs::symlink(directory.path(), data_root.join(identity)).unwrap();
    assert!(export_backup(&db, root(&data_root)).await.is_err());
    let dangling = pubky_swap_boltz::identity_from_secret(&[2; 32]);
    std::os::unix::fs::symlink(directory.path().join("absent"), data_root.join(&dangling)).unwrap();
    assert!(checked_identity_path(&data_root, &dangling, true).is_err());
}

#[path = "pubky_fixture.rs"]
#[allow(dead_code)]
mod fixture;

#[tokio::test]
async fn accepted_pubky_contracts_and_all_spend_attempts_roundtrip() {
    use super::super::{api::record_from_response, models::derive_swap_keypair};
    use boltz_client::util::secrets::Preimage;
    use pubky_swap_boltz::model::{ReverseRequest, SubmarineRequest};

    let directory = tempfile::tempdir().unwrap();
    let source_root = directory.path().join("source-identities");
    let source = database(&directory.path().join("source.db")).await;
    let identity = fixture::fixture_identity();
    let binding = fixture::fixture_binding();
    let bridge = fixture::fixture_bridge_with_store(
        Store::open(&source_root.join(&identity), &binding).unwrap(),
    )
    .unwrap();
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let mut records = Vec::new();
    let mut intents = Vec::new();
    let mut before_admission = None;
    for refund in [true, false] {
        let index = source.reserve_swap_index().await.unwrap();
        let key = derive_swap_keypair(mnemonic, None, BoltzNetwork::Regtest, index).unwrap();
        let public_key = bitcoin::PublicKey::new(key.public_key());
        let destination = bitcoin::Address::p2tr(
            &bitcoin::secp256k1::Secp256k1::new(),
            key.public_key().x_only_public_key().0,
            None,
            bitcoin::Network::Regtest,
        )
        .to_string();
        let preimage = Preimage::from_swap_key(&key);
        let request = if refund {
            let invoice =
                lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Regtest)
                    .description("backup fixture".into())
                    .payment_hash(preimage.sha256)
                    .payment_secret(lightning_invoice::PaymentSecret([7; 32]))
                    .current_timestamp()
                    .expiry_time(std::time::Duration::from_secs(86_400))
                    .min_final_cltv_expiry_delta(240)
                    .amount_milli_satoshis(100_000_000)
                    .build_signed(|message| {
                        bitcoin::secp256k1::Secp256k1::new()
                            .sign_ecdsa_recoverable(message, &key.secret_key())
                    })
                    .unwrap()
                    .to_string();
            CreateRequest::Submarine(SubmarineRequest {
                from: "BTC".into(),
                to: "BTC".into(),
                invoice,
                refund_public_key: public_key.to_string(),
                pair_hash: String::new(),
                referral_id: String::new(),
                error: String::new(),
            })
        } else {
            CreateRequest::Reverse(ReverseRequest {
                from: "BTC".into(),
                to: "BTC".into(),
                preimage_hash: preimage.sha256.to_string(),
                claim_public_key: public_key.to_string(),
                invoice_amount: 100_000,
                onchain_amount: 0,
                pair_hash: String::new(),
                referral_id: String::new(),
                error: String::new(),
            })
        };
        let intent = CreationIntent {
            id: uuid::Uuid::new_v4().to_string(),
            request_key: format!("fixture-{index}"),
            wallet_fingerprint: sha256::Hash::hash(&key.public_key().serialize()).to_string(),
            backend_binding: binding.clone(),
            network: BoltzNetwork::Regtest,
            electrum_url: "tcp://127.0.0.1:50001".into(),
            swap_index: index,
            recipient_amount_sat: None,
            claim_address: (!refund).then(|| destination.clone()),
            created_at: 1,
            request: request.clone(),
        };
        source.save_intent(&intent).await.unwrap();
        if refund {
            before_admission = Some(
                serde_json::to_string(&Snapshot {
                    version: VERSION,
                    core: source.core_snapshot().await.unwrap(),
                    identities: vec![IdentitySnapshot {
                        identity: identity.clone(),
                        store: bridge.export_snapshot().unwrap(),
                    }],
                })
                .unwrap(),
            );
        }
        let response = bridge
            .create(request, Some(intent.id.clone()))
            .await
            .unwrap();
        let record = record_from_response(&intent, &response, &key).unwrap();
        source.complete_intent(&intent, &record).await.unwrap();
        // More than one signed/broadcast attempt can exist before completion was recorded.
        for txid in ["aa".repeat(32), "bb".repeat(32)] {
            source
                .journal_spend(&record.id, &txid, refund.then_some(destination.as_str()))
                .await
                .unwrap();
        }
        records.push(record);
        intents.push(intent);
    }
    drop(bridge);
    let snapshot = export_backup(&source, root(&source_root)).await.unwrap();
    let failed_path = directory.path().join("failed.db");
    let failed_root = directory.path().join("failed-identities");
    let failed = database(&failed_path).await;
    failed.conn.lock().await.execute_batch("CREATE TRIGGER fail_restore BEFORE INSERT ON swaps BEGIN SELECT RAISE(FAIL,'simulated Core write failure'); END;").unwrap();
    assert!(
        restore_backup(&failed, snapshot.clone(), root(&failed_root))
            .await
            .is_err()
    );
    assert_eq!(
        Store::snapshot_directory(&failed_root.join(&identity))
            .unwrap()
            .swaps
            .len(),
        2
    );
    assert_eq!(failed.core_snapshot().await.unwrap().next_swap_index, 2);
    assert!(export_backup(&failed, root(&failed_root)).await.is_err());
    assert!(failed.reserve_swap_index().await.is_err());
    drop(failed);
    let failed = database(&failed_path).await;
    assert!(export_backup(&failed, root(&failed_root)).await.is_err());
    assert!(restore_backup(
        &failed,
        before_admission.as_ref().unwrap().clone(),
        root(&failed_root)
    )
    .await
    .is_err());
    failed
        .conn
        .lock()
        .await
        .execute_batch("DROP TRIGGER fail_restore;")
        .unwrap();
    restore_backup(&failed, snapshot.clone(), root(&failed_root))
        .await
        .unwrap();
    assert!(export_backup(&failed, root(&failed_root)).await.is_ok());
    assert_eq!(failed.list_swaps().await.unwrap().len(), 2);
    assert_eq!(failed.reserve_swap_index().await.unwrap(), 2);

    let target = database(&directory.path().join("target.db")).await;
    let target_root = directory.path().join("target-identities");
    // The local process may have stopped after provider acceptance but before
    // completing Core's intent. Only that exact retry binding can retire it.
    target.reserve_swap_index().await.unwrap();
    target.save_intent(&intents[0]).await.unwrap();
    restore_backup(&target, snapshot.clone(), root(&target_root))
        .await
        .unwrap();
    for record in &records {
        let restored = target.get_swap(&record.id).await.unwrap().unwrap();
        assert_eq!(restored.create_response_json, record.create_response_json);
        assert_eq!(restored.backend_binding, record.backend_binding);
        assert_eq!(target.pending_spends(&record.id).await.unwrap().len(), 2);
    }
    assert!(target.creation_intents().await.unwrap().is_empty());
    assert_eq!(target.reserve_swap_index().await.unwrap(), 2);
    let wrapper = Store::snapshot_directory(&target_root.join(&identity)).unwrap();
    assert_eq!(wrapper.swaps.len(), 2);
    assert_eq!(wrapper.idempotency.len(), 2);
    // A backup taken before wrapper admission has no retry evidence yet.
    // The newer local store can still prove that this intent already completed.
    restore_backup(&target, before_admission.unwrap(), root(&target_root))
        .await
        .unwrap();
    assert!(target.creation_intents().await.unwrap().is_empty());
    assert_eq!(target.list_swaps().await.unwrap().len(), 2);
    let mut incomplete: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    incomplete["identities"] = serde_json::json!([]);
    assert!(
        restore_backup(&target, incomplete.to_string(), root(&target_root))
            .await
            .is_err()
    );
    assert_eq!(target.list_swaps().await.unwrap().len(), 2);

    let divergent = database(&directory.path().join("divergent.db")).await;
    divergent.reserve_swap_index().await.unwrap();
    let mut other = intents[0].clone();
    other.id = uuid::Uuid::new_v4().to_string();
    if let CreateRequest::Submarine(request) = &mut other.request {
        let keys = derive_swap_keypair(mnemonic, None, BoltzNetwork::Regtest, 0).unwrap();
        request.invoice =
            lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Regtest)
                .description("different funded swap".into())
                .payment_hash(sha256::Hash::hash(&[42; 32]))
                .payment_secret(lightning_invoice::PaymentSecret([7; 32]))
                .current_timestamp()
                .min_final_cltv_expiry_delta(240)
                .amount_milli_satoshis(100_000_000)
                .build_signed(|message| {
                    bitcoin::secp256k1::Secp256k1::new()
                        .sign_ecdsa_recoverable(message, &keys.secret_key())
                })
                .unwrap()
                .to_string();
    }
    divergent.save_intent(&other).await.unwrap();
    assert!(restore_backup(
        &divergent,
        snapshot,
        root(&directory.path().join("divergent-identities"))
    )
    .await
    .is_err());
    assert_eq!(divergent.creation_intents().await.unwrap()[0].id, other.id);
    assert!(divergent.list_swaps().await.unwrap().is_empty());
}

#[tokio::test]
async fn derivation_counter_never_enters_hardened_child_space() {
    let directory = tempfile::tempdir().unwrap();
    let db = database(&directory.path().join("core.db")).await;
    let data_root = root(&directory.path().join("identities"));
    let snapshot = export_backup(&db, data_root.clone()).await.unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    let limit = super::super::models::SWAP_INDEX_LIMIT;
    value["core"]["next_swap_index"] = serde_json::json!(limit + 1);
    assert!(restore_backup(&db, value.to_string(), data_root.clone())
        .await
        .is_err());
    value["core"]["next_swap_index"] = serde_json::json!(limit - 1);
    restore_backup(&db, value.to_string(), data_root.clone())
        .await
        .unwrap();
    assert_eq!(db.reserve_swap_index().await.unwrap(), limit - 1);
    assert!(db.reserve_swap_index().await.is_err());
    // An exhausted counter is valid recovery data for existing swap keys.
    assert!(export_backup(&db, data_root).await.is_ok());
}
