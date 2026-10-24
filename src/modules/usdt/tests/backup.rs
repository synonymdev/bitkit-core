use super::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
struct RemoteBackup {
    unavailable: AtomicBool,
    writes: AtomicUsize,
    snapshot: Mutex<Option<String>>,
    after_persist: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[async_trait::async_trait]
impl UsdtBackup for RemoteBackup {
    async fn persist(&self, snapshot: String) -> Result<(), UsdtError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(UsdtError::BackupUnavailable);
        }
        *self.snapshot.lock().unwrap() = Some(snapshot);
        self.writes.fetch_add(1, Ordering::SeqCst);
        if let Some(after_persist) = self.after_persist.lock().unwrap().as_ref() {
            after_persist();
        }
        Ok(())
    }
}

#[tokio::test]
async fn expired_submission_deadline_does_not_create_payment() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    let deadline = std::time::SystemTime::now();
    for invalid in ["not-a-timestamp", "2026-10-12T00:00:60Z"] {
        assert!(matches!(
            wallet
                .send_with_deadline(
                    quote.id.clone(),
                    TEST_PHRASE.into(),
                    None,
                    Some(invalid.into())
                )
                .await,
            Err(UsdtError::InvalidSubmissionDeadline)
        ));
    }
    *wallet.rpc.clock.lock().unwrap() = Some(deadline + std::time::Duration::from_nanos(1));
    assert!(matches!(
        wallet
            .send_with_deadline(
                quote.id,
                TEST_PHRASE.into(),
                None,
                Some(chrono::DateTime::<chrono::Utc>::from(deadline).to_rfc3339())
            )
            .await,
        Err(UsdtError::SubmissionDeadlineExpired)
    ));
    assert!(wallet.history().unwrap().is_empty());
    assert!(chain.state.lock().unwrap().operations.is_empty());
}

#[tokio::test]
async fn submission_deadline_prevents_first_broadcast_after_backup_failure() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let backup = Arc::new(RemoteBackup::default());
    backup.unavailable.store(true, Ordering::SeqCst);
    let wallet = backed_up_wallet(&chain, &directory, backup.clone());
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    let deadline = std::time::SystemTime::now();
    *wallet.rpc.clock.lock().unwrap() = Some(deadline);
    assert!(matches!(
        wallet
            .send_with_deadline(
                quote.id,
                TEST_PHRASE.into(),
                None,
                Some(chrono::DateTime::<chrono::Utc>::from(deadline).to_rfc3339())
            )
            .await,
        Err(UsdtError::BackupUnavailable)
    ));
    drop(wallet);
    let reopened = backed_up_wallet(&chain, &directory, backup.clone());
    *reopened.rpc.clock.lock().unwrap() = Some(deadline + std::time::Duration::from_nanos(1));
    backup.unavailable.store(false, Ordering::SeqCst);
    chain.state.lock().unwrap().tip += 3;
    assert_eq!(
        reopened.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert!(chain.state.lock().unwrap().operations.is_empty());
}

#[tokio::test]
async fn submission_deadline_passing_during_backup_prevents_dispatch() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let backup = Arc::new(RemoteBackup::default());
    let wallet = backed_up_wallet(&chain, &directory, backup.clone());
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    let deadline = std::time::SystemTime::now();
    let clock = wallet.rpc.clock.clone();
    *clock.lock().unwrap() = Some(deadline);
    *backup.after_persist.lock().unwrap() = Some(Box::new(move || {
        *clock.lock().unwrap() = Some(deadline + std::time::Duration::from_nanos(1));
    }));
    assert!(matches!(
        wallet
            .send_with_deadline(
                quote.id,
                TEST_PHRASE.into(),
                None,
                Some(chrono::DateTime::<chrono::Utc>::from(deadline).to_rfc3339())
            )
            .await,
        Err(UsdtError::SubmissionDeadlineExpired)
    ));
    assert_eq!(backup.writes.load(Ordering::SeqCst), 1);
    assert!(chain.state.lock().unwrap().operations.is_empty());
    assert_eq!(
        wallet.history().unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    let snapshot: serde_json::Value =
        serde_json::from_str(&backup.snapshot.lock().unwrap().clone().unwrap()).unwrap();
    assert_eq!(
        snapshot["transfers"][0]["plan"]["submission_deadline"],
        serde_json::to_value(deadline).unwrap()
    );
}

#[tokio::test]
async fn restored_submission_deadline_blocks_rebroadcast_but_allows_settlement() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    let deadline = std::time::SystemTime::now();
    *wallet.rpc.clock.lock().unwrap() = Some(deadline);
    wallet
        .send_with_deadline(
            quote.id.clone(),
            TEST_PHRASE.into(),
            None,
            Some(chrono::DateTime::<chrono::Utc>::from(deadline).to_rfc3339()),
        )
        .await
        .unwrap();
    let snapshot = wallet.export_backup().unwrap();
    wallet
        .send_with_deadline(quote.id.clone(), TEST_PHRASE.into(), None, None)
        .await
        .unwrap();
    assert_eq!(wallet.export_backup().unwrap(), snapshot);
    drop(wallet);

    let reopened = chain.wallet(&directory);
    *reopened.rpc.clock.lock().unwrap() = Some(deadline + std::time::Duration::from_nanos(1));
    chain.state.lock().unwrap().tip += 3;
    assert_eq!(
        reopened.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert_eq!(chain.state.lock().unwrap().operations.len(), 1);

    let restored_directory = tempfile::tempdir().unwrap();
    let restored = chain.wallet(&restored_directory);
    restored.restore_backup(snapshot).await.unwrap();
    *restored.rpc.clock.lock().unwrap() = Some(deadline + std::time::Duration::from_nanos(1));
    assert_eq!(
        restored.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert_eq!(chain.state.lock().unwrap().operations.len(), 1);
    chain.state.lock().unwrap().mined = true;
    assert_eq!(
        restored.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Confirmed
    );
    assert_eq!(chain.state.lock().unwrap().operations.len(), 1);
}

#[tokio::test]
async fn backup_merge_preserves_the_earliest_submission_deadline() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    let deadline = std::time::SystemTime::now();
    *wallet.rpc.clock.lock().unwrap() = Some(deadline);
    wallet
        .send_with_deadline(
            quote.id.clone(),
            TEST_PHRASE.into(),
            None,
            Some(chrono::DateTime::<chrono::Utc>::from(deadline).to_rfc3339()),
        )
        .await
        .unwrap();
    let original: serde_json::Value =
        serde_json::from_str(&wallet.export_backup().unwrap()).unwrap();
    let earlier = deadline - std::time::Duration::from_nanos(1);
    for (incoming, expected) in [
        (
            Some(deadline + std::time::Duration::from_secs(60)),
            deadline,
        ),
        (None, deadline),
        (Some(earlier), earlier),
    ] {
        let mut snapshot = original.clone();
        snapshot["transfers"][0]["plan"]["submission_deadline"] =
            serde_json::to_value(incoming).unwrap();
        wallet.restore_backup(snapshot.to_string()).await.unwrap();
        assert_eq!(
            wallet
                .store
                .pending_plan(&quote.id)
                .unwrap()
                .unwrap()
                .submission_deadline,
            Some(expected)
        );
    }
}

fn backed_up_wallet(
    chain: &MockChain,
    dir: &tempfile::TempDir,
    backup: Arc<RemoteBackup>,
) -> Arc<UsdtWallet> {
    UsdtWallet::new(
        usdt_address(TEST_PHRASE.into(), None).unwrap(),
        dir.path().join("usdt.sqlite").to_string_lossy().into(),
        format!("{}/chain", chain.url),
        format!("{}/bundler", chain.url),
        None,
        backup,
    )
    .unwrap()
}

#[tokio::test]
async fn submission_and_recovery_wait_for_remote_backup() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let backup = Arc::new(RemoteBackup::default());
    backup.unavailable.store(true, Ordering::SeqCst);
    let wallet = backed_up_wallet(&chain, &directory, backup.clone());
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    assert!(matches!(
        wallet
            .send(quote.id.clone(), TEST_PHRASE.into(), None)
            .await,
        Err(UsdtError::BackupUnavailable)
    ));
    assert_eq!(wallet.history().unwrap()[0].id, quote.id);
    assert!(chain.state.lock().unwrap().operations.is_empty());
    chain.state.lock().unwrap().tip += 3;
    assert!(matches!(
        wallet.refresh_transfers().await,
        Err(UsdtError::BackupUnavailable)
    ));
    assert!(chain.state.lock().unwrap().operations.is_empty());
    backup.unavailable.store(false, Ordering::SeqCst);
    wallet.refresh_transfers().await.unwrap();
    let snapshot = backup.snapshot.lock().unwrap().clone().unwrap();
    assert!(!snapshot.contains(TEST_PHRASE));
    let original = serde_json::to_value(&chain.state.lock().unwrap().operations[0]).unwrap();
    backup.unavailable.store(true, Ordering::SeqCst);
    wallet.refresh_transfers().await.unwrap();
    assert_eq!(chain.state.lock().unwrap().operations.len(), 2);
    assert_eq!(backup.writes.load(Ordering::SeqCst), 1);
    drop(wallet);
    let restored_directory = tempfile::tempdir().unwrap();
    let restored = backed_up_wallet(&chain, &restored_directory, backup.clone());
    restored.restore_backup(snapshot.clone()).await.unwrap();
    restored.restore_backup(snapshot).await.unwrap();
    assert_eq!(restored.history().unwrap().len(), 1);
    assert!(matches!(
        restored
            .quote_transfer(RECIPIENT.into(), 1, UsdtDestination::Arbitrum)
            .await,
        Err(UsdtError::PendingTransfer)
    ));
    let submitted = chain.state.lock().unwrap().operations.len();
    assert!(matches!(
        restored.refresh_transfers().await,
        Err(UsdtError::BackupUnavailable)
    ));
    assert_eq!(chain.state.lock().unwrap().operations.len(), submitted);
    backup.unavailable.store(false, Ordering::SeqCst);
    restored.refresh_transfers().await.unwrap();
    assert_eq!(backup.writes.load(Ordering::SeqCst), 2);
    assert_eq!(
        serde_json::to_value(chain.state.lock().unwrap().operations.last().unwrap()).unwrap(),
        original
    );
    chain.state.lock().unwrap().mined = true;
    let history = restored.refresh_transfers().await.unwrap();
    assert_eq!(history[0].id, quote.id);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    chain.state.lock().unwrap().nonce = 1;
    let next = restored
        .quote_transfer(RECIPIENT.into(), 2_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    backup.unavailable.store(true, Ordering::SeqCst);
    let submitted = chain.state.lock().unwrap().operations.len();
    assert!(matches!(
        restored.send(next.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::BackupUnavailable)
    ));
    assert_eq!(chain.state.lock().unwrap().operations.len(), submitted);
}

#[test]
fn foreign_backup_errors_are_recoverable() {
    let result = <Result<(), UsdtError> as uniffi::LiftReturn<crate::UniFfiTag>>::handle_callback_unexpected_error(
        uniffi::UnexpectedUniFFICallbackError { reason: "remote storage unavailable".into() },
    );
    assert!(matches!(result, Err(UsdtError::BackupUnavailable)));
}

#[tokio::test]
async fn restore_matches_operation_hashes_independently_of_hex_case() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let snapshot = wallet.export_backup().unwrap();
    let mut backup: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    let mut duplicate = backup["transfers"][0].clone();
    let hash = duplicate["transfer"]["user_operation_hash"]
        .as_str()
        .unwrap()
        .to_owned();
    duplicate["transfer"]["user_operation_hash"] = json!(format!("0x{}", hash[2..].to_uppercase()));
    backup["transfers"] = json!([duplicate.clone()]);
    wallet.restore_backup(backup.to_string()).await.unwrap();
    assert_eq!(wallet.export_backup().unwrap(), snapshot);
    duplicate["transfer"]["id"] = json!("another-payment");
    backup = serde_json::from_str(&snapshot).unwrap();
    backup["transfers"]
        .as_array_mut()
        .unwrap()
        .push(duplicate.clone());
    let empty_directory = tempfile::tempdir().unwrap();
    let empty = chain.wallet(&empty_directory);
    assert!(matches!(
        empty.restore_backup(backup.to_string()).await,
        Err(UsdtError::InvalidBackup)
    ));
    assert!(empty.history().unwrap().is_empty());
    backup["transfers"] = json!([duplicate]);
    assert!(matches!(
        wallet.restore_backup(backup.to_string()).await,
        Err(UsdtError::InvalidBackup)
    ));
    assert_eq!(wallet.export_backup().unwrap(), snapshot);
}

#[tokio::test]
async fn restore_is_atomic_and_rejects_other_accounts_and_conflicting_payments() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let snapshot = wallet.export_backup().unwrap();
    let target_directory = tempfile::tempdir().unwrap();
    let target = chain.wallet(&target_directory);
    let mut invalid: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    invalid["identity"] = serde_json::json!("42161:another-wallet");
    assert!(matches!(
        target.restore_backup(invalid.to_string()).await,
        Err(UsdtError::InvalidCredentials)
    ));
    invalid = serde_json::from_str(&snapshot).unwrap();
    invalid["transfers"][0]["transfer"]["amount"] = serde_json::json!(2);
    assert!(matches!(
        target.restore_backup(invalid.to_string()).await,
        Err(UsdtError::InvalidBackup)
    ));
    assert!(target.history().unwrap().is_empty());
    target.restore_backup(snapshot.clone()).await.unwrap();
    let mut conflict: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    conflict["transfers"][0]["plan"] = serde_json::Value::Null;
    conflict["transfers"][0]["transfer"]["status"] = serde_json::json!("Failed");
    conflict["transfers"][0]["transfer"]["user_operation_hash"] =
        serde_json::json!(alloy_primitives::B256::repeat_byte(1).to_string());
    let mut extra = conflict["transfers"][0].clone();
    extra["transfer"]["id"] = serde_json::json!("another-id");
    extra["transfer"]["user_operation_hash"] =
        serde_json::json!(alloy_primitives::B256::repeat_byte(2).to_string());
    // Insert a distinct payment before the later ID conflict must roll it back.
    conflict["transfers"]
        .as_array_mut()
        .unwrap()
        .insert(0, extra);
    let empty_directory = tempfile::tempdir().unwrap();
    let empty = chain.wallet(&empty_directory);
    empty.restore_backup(conflict.to_string()).await.unwrap();
    assert_eq!(empty.history().unwrap().len(), 2);
    assert!(matches!(
        target.restore_backup(conflict.to_string()).await,
        Err(UsdtError::InvalidBackup)
    ));
    assert_eq!(target.export_backup().unwrap(), snapshot);
}

#[tokio::test]
async fn stale_backups_preserve_newer_outcomes_and_recover_application_ids() {
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    wallet
        .send(quote.id.clone(), TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let pending_backup = wallet.export_backup().unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
    }
    wallet.refresh_transfers().await.unwrap();
    let settled_backup = wallet.export_backup().unwrap();
    wallet.restore_backup(pending_backup.clone()).await.unwrap();
    assert_eq!(
        wallet.history().unwrap()[0].status,
        UsdtTransferStatus::Confirmed
    );
    let recovered_directory = tempfile::tempdir().unwrap();
    let recovered = chain.wallet(&recovered_directory);
    recovered.sync_history().await.unwrap();
    recovered.restore_backup(pending_backup).await.unwrap();
    let history = recovered.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, quote.id);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    let empty_directory = tempfile::tempdir().unwrap();
    let empty = chain.wallet(&empty_directory);
    empty.restore_backup(settled_backup).await.unwrap();
    assert_eq!(
        empty.history().unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    {
        let mut state = chain.state.lock().unwrap();
        state.tip += history::HISTORY_REVISIT_BLOCKS + 1;
        state.hide_logs = true;
    }
    sync_history_to_tip(&empty).await;
    chain.state.lock().unwrap().hide_logs = false;
    assert_eq!(
        empty.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Confirmed
    );
}

#[tokio::test]
async fn recovery_snapshots_merge_chain_aliases_in_either_order_after_restart() {
    let chain = MockChain::start().await;
    let source_directory = tempfile::tempdir().unwrap();
    let source = chain.wallet(&source_directory);
    let quote = source
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    source
        .send(quote.id.clone(), TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let signed = source.export_backup().unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
    }
    let history_directory = tempfile::tempdir().unwrap();
    let history = chain.wallet(&history_directory);
    history.sync_history().await.unwrap();
    let discovered = history.export_backup().unwrap();
    assert_ne!(history.history().unwrap()[0].id, quote.id);

    for snapshots in [[&signed, &discovered], [&discovered, &signed]] {
        let directory = tempfile::tempdir().unwrap();
        let wallet = chain.wallet(&directory);
        wallet.restore_backup(snapshots[0].clone()).await.unwrap();
        drop(wallet);
        let reopened = chain.wallet(&directory);
        reopened.restore_backup(snapshots[1].clone()).await.unwrap();
        for snapshot in snapshots {
            reopened.restore_backup(snapshot.clone()).await.unwrap();
        }
        let transfers = reopened.refresh_transfers().await.unwrap();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].id, quote.id);
        assert_eq!(transfers[0].status, UsdtTransferStatus::Confirmed);
        assert_eq!(chain.state.lock().unwrap().operations.len(), 1);

        let before = reopened.export_backup().unwrap();
        let mut conflict: serde_json::Value = serde_json::from_str(&signed).unwrap();
        conflict["transfers"][0]["transfer"]["id"] = json!("another-application-id");
        assert!(matches!(
            reopened.restore_backup(conflict.to_string()).await,
            Err(UsdtError::InvalidBackup)
        ));
        assert_eq!(reopened.export_backup().unwrap(), before);
    }
}

#[tokio::test]
async fn restored_receipts_reconcile_with_the_canonical_chain() {
    use alloy_primitives::B256;
    let chain = MockChain::start().await;
    let directory = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&directory);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.incoming_count = 1;
        state.tip += 3;
    }
    let history_directory = tempfile::tempdir().unwrap();
    let history_wallet = chain.wallet(&history_directory);
    sync_history_to_tip(&history_wallet).await;
    let history = history_wallet.history().unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].is_incoming);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    let snapshot = history_wallet.export_backup().unwrap();
    let restored_directory = tempfile::tempdir().unwrap();
    let restored = chain.wallet(&restored_directory);
    restored.restore_backup(snapshot).await.unwrap();
    chain.state.lock().unwrap().incoming_count = 0;
    sync_history_to_tip(&restored).await;
    assert_eq!(restored.history().unwrap()[0].id, history[0].id);
    {
        let mut state = chain.state.lock().unwrap();
        state.block_hashes.insert(20001, B256::repeat_byte(0xaa));
    }
    sync_history_to_tip(&restored).await;
    assert!(restored.history().unwrap().is_empty());
}

#[tokio::test]
async fn orchestra_backup_restores_funded_delivery_after_chain_history() {
    let chain = MockChain::start().await;
    let source = tempfile::tempdir().unwrap();
    let wallet = chain.wallet_with_bridges(&source, true);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Base)
        .await
        .unwrap();
    wallet
        .send(quote.id.clone(), TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
    }
    wallet.refresh_transfers().await.unwrap();
    let mut refunded = wallet.store.transfer(&quote.id).unwrap().unwrap();
    refunded.status = UsdtTransferStatus::BridgeRefunded;
    refunded.received_amount = 0;
    let bridge = refunded.orchestra.as_mut().unwrap();
    bridge.refund_tx = Some(alloy_primitives::B256::repeat_byte(9).to_string());
    bridge.refund_amount = Some(900_000);
    bridge.destination_tx = Some("historical-provider-reference".into());
    wallet
        .store
        .update_delivery(
            &refunded,
            Some((20001, alloy_primitives::B256::repeat_byte(8))),
        )
        .unwrap();
    let snapshot = wallet.export_backup().unwrap();
    let target = tempfile::tempdir().unwrap();
    let restored = chain.wallet_with_bridges(&target, true);
    sync_history_to_tip(&restored).await;
    restored.restore_backup(snapshot.clone()).await.unwrap();
    restored.restore_backup(snapshot).await.unwrap();
    let payment = restored
        .history()
        .unwrap()
        .into_iter()
        .find(|transfer| transfer.id == quote.id)
        .unwrap();
    assert_eq!(payment.destination, UsdtDestination::Base);
    assert_eq!(payment.recipient, RECIPIENT);
    assert_eq!(payment.status, UsdtTransferStatus::Bridging);
    assert_eq!(payment.received_amount, quote.received_amount);
    let bridge = payment.orchestra.unwrap();
    assert!(bridge.refund_tx.is_none());
    assert!(bridge.destination_tx.is_none());
    assert_eq!(
        restored.store.orchestra_plan(&quote.id).unwrap().ticket,
        "quote-ticket"
    );
    restored.refresh_transfers().await.unwrap();
    assert_eq!(chain.state.lock().unwrap().operations.len(), 1);
}

#[tokio::test]
async fn transfer_sender_survives_backup_and_old_snapshots_remain_unknown() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000, UsdtDestination::Arbitrum)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    assert_eq!(
        sent.sender.as_deref(),
        Some(wallet.receive_address().as_str())
    );
    let snapshot = wallet.export_backup().unwrap();
    let restored_dir = tempfile::tempdir().unwrap();
    let restored = chain.wallet(&restored_dir);
    restored.restore_backup(snapshot.clone()).await.unwrap();
    assert_eq!(restored.history().unwrap()[0].sender, sent.sender);
    let mut old: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    old["transfers"][0]["transfer"]
        .as_object_mut()
        .unwrap()
        .remove("sender");
    let old_dir = tempfile::tempdir().unwrap();
    let old_wallet = chain.wallet(&old_dir);
    old_wallet.restore_backup(old.to_string()).await.unwrap();
    assert_eq!(old_wallet.history().unwrap()[0].sender, None);
    restored.restore_backup(old.to_string()).await.unwrap();
    assert_eq!(restored.history().unwrap()[0].sender, sent.sender);
    old["transfers"][0]["transfer"]["sender"] = json!(RECIPIENT);
    assert!(matches!(
        old_wallet.restore_backup(old.to_string()).await,
        Err(UsdtError::InvalidBackup)
    ));
}
