use crate::modules::boltz::api::{get_reverse_limits, get_submarine_limits};
use crate::modules::boltz::claim::{claim_reverse_swap_guarded, ClaimOutcome};
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::guard::lock_swap;
use crate::modules::boltz::models::{derive_swap_keypair, BoltzDB, SwapRecord};
use crate::modules::boltz::refund::refund_submarine_swap_guarded;
use crate::modules::boltz::types::{BoltzNetwork, BoltzSwapStatus, BoltzSwapType};
use boltz_client::util::secrets::Preimage;

/// A throwaway BIP39 mnemonic used only by the offline tests.
const TEST_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn sample_record(id: &str, swap_type: BoltzSwapType) -> SwapRecord {
    SwapRecord {
        id: id.to_string(),
        swap_type,
        status: "swap.created".to_string(),
        network: BoltzNetwork::Testnet,
        electrum_url: "ssl://electrum.example.com:50002".to_string(),
        swap_index: 0,
        invoice: Some("lnbc1...".to_string()),
        lockup_address: Some("bc1qexample".to_string()),
        onchain_address: Some("bc1qclaim".to_string()),
        amount_sat: 100_000,
        onchain_amount_sat: Some(99_000),
        timeout_block_height: 800_000,
        create_response_json: "{}".to_string(),
        claim_tx_id: None,
        refund_tx_id: None,
        created_at: 1_700_000_000,
    }
}

#[test]
fn status_round_trips_through_raw() {
    let cases = [
        ("swap.created", BoltzSwapStatus::SwapCreated),
        ("transaction.mempool", BoltzSwapStatus::TransactionMempool),
        (
            "transaction.claim.pending",
            BoltzSwapStatus::TransactionClaimPending,
        ),
        ("invoice.failedToPay", BoltzSwapStatus::InvoiceFailedToPay),
        ("transaction.claimed", BoltzSwapStatus::TransactionClaimed),
    ];
    for (raw, expected) in cases {
        let mapped = BoltzSwapStatus::from_raw(raw);
        assert_eq!(mapped, expected);
        assert_eq!(mapped.as_raw(), raw);
    }
}

#[test]
fn unknown_status_preserves_raw() {
    let mapped = BoltzSwapStatus::from_raw("some.future.state");
    assert_eq!(
        mapped,
        BoltzSwapStatus::Unknown {
            raw: "some.future.state".to_string()
        }
    );
    assert_eq!(mapped.as_raw(), "some.future.state");
}

#[test]
fn terminal_states_are_terminal() {
    assert!(BoltzSwapStatus::TransactionClaimed.is_terminal());
    assert!(BoltzSwapStatus::TransactionRefunded.is_terminal());
    assert!(BoltzSwapStatus::SwapExpired.is_terminal());
    assert!(!BoltzSwapStatus::TransactionMempool.is_terminal());
    assert!(!BoltzSwapStatus::SwapCreated.is_terminal());
}

#[test]
fn derivation_is_deterministic_and_index_unique() {
    // Same (mnemonic, index) reproduces the same key and preimage — the
    // property that makes swaps recoverable from the seed alone.
    let k0a = derive_swap_keypair(TEST_MNEMONIC, None, BoltzNetwork::Mainnet, 0).unwrap();
    let k0b = derive_swap_keypair(TEST_MNEMONIC, None, BoltzNetwork::Mainnet, 0).unwrap();
    assert_eq!(k0a.secret_bytes(), k0b.secret_bytes());
    assert_eq!(
        Preimage::from_swap_key(&k0a).bytes,
        Preimage::from_swap_key(&k0b).bytes
    );

    // Different indices yield different keys (no key reuse across swaps).
    let k1 = derive_swap_keypair(TEST_MNEMONIC, None, BoltzNetwork::Mainnet, 1).unwrap();
    assert_ne!(k0a.secret_bytes(), k1.secret_bytes());

    // A different passphrase yields a different key (passphrase must match).
    let k0p = derive_swap_keypair(TEST_MNEMONIC, Some("pass"), BoltzNetwork::Mainnet, 0).unwrap();
    assert_ne!(k0a.secret_bytes(), k0p.secret_bytes());
}

#[test]
fn rejects_invalid_mnemonic() {
    assert!(derive_swap_keypair("not a valid mnemonic", None, BoltzNetwork::Mainnet, 0).is_err());
}

#[tokio::test]
async fn reserve_swap_index_is_monotonic_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    assert_eq!(db.reserve_swap_index().await.unwrap(), 0);
    assert_eq!(db.reserve_swap_index().await.unwrap(), 1);
    assert_eq!(db.reserve_swap_index().await.unwrap(), 2);
    drop(db);

    // The counter survives reopening the database — no index is ever reused.
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    assert_eq!(db.reserve_swap_index().await.unwrap(), 3);
}

#[tokio::test]
async fn db_round_trip_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    let mut reverse = sample_record("rev-1", BoltzSwapType::Reverse);
    reverse.swap_index = 3;
    let submarine = sample_record("sub-1", BoltzSwapType::Submarine);
    db.insert_swap(&reverse).await.unwrap();
    db.insert_swap(&submarine).await.unwrap();

    // get_swap reconstructs the record faithfully (no secrets are stored, only
    // the derivation index needed to reconstruct them from the seed).
    let loaded = db.get_swap("rev-1").await.unwrap().expect("swap exists");
    assert_eq!(loaded.swap_type, BoltzSwapType::Reverse);
    assert_eq!(loaded.swap_index, 3);
    assert_eq!(loaded.amount_sat, 100_000);
    assert_eq!(loaded.onchain_amount_sat, Some(99_000));

    // Both swaps are pending until they reach a terminal state.
    assert_eq!(db.list_swaps().await.unwrap().len(), 2);
    assert_eq!(db.list_pending_swaps().await.unwrap().len(), 2);

    // A non-terminal status persists and leaves the swap in the pending set.
    db.update_status("rev-1", "transaction.mempool")
        .await
        .unwrap();
    let loaded = db.get_swap("rev-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, "transaction.mempool");
    assert_eq!(db.list_pending_swaps().await.unwrap().len(), 2);

    // Recording the claim tx id stores it and advances the swap to the terminal
    // claimed status, which drops it from the pending set.
    db.set_claim_tx("rev-1", "abc123").await.unwrap();
    let loaded = db.get_swap("rev-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, "transaction.claimed");
    assert_eq!(loaded.claim_tx_id, Some("abc123".to_string()));
    let pending = db.list_pending_swaps().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "sub-1");

    // Recording the refund tx id does the same for the submarine swap.
    db.set_refund_tx("sub-1", "def456").await.unwrap();
    let loaded = db.get_swap("sub-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, "transaction.refunded");
    assert_eq!(loaded.refund_tx_id, Some("def456".to_string()));
    assert!(db.list_pending_swaps().await.unwrap().is_empty());

    // Terminal swaps are still listed, just no longer pending.
    assert_eq!(db.list_swaps().await.unwrap().len(), 2);
}

#[tokio::test]
async fn guarded_claim_returns_recorded_txid_without_rebroadcasting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    let mut record = sample_record("rev-claimed", BoltzSwapType::Reverse);
    record.claim_tx_id = Some("already-broadcast".to_string());
    db.insert_swap(&record).await.unwrap();

    // The record's electrum_url is unreachable, so reaching the broadcast path at
    // all would fail the call rather than return the recorded txid.
    let outcome = claim_reverse_swap_guarded(&db, "rev-claimed", TEST_MNEMONIC, None, None)
        .await
        .unwrap();
    assert!(matches!(outcome, ClaimOutcome::AlreadyClaimed(_)));
    assert_eq!(outcome.txid(), "already-broadcast");
}

#[tokio::test]
async fn guarded_refund_returns_recorded_txid_without_rebroadcasting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    let mut record = sample_record("sub-refunded", BoltzSwapType::Submarine);
    record.refund_tx_id = Some("already-refunded".to_string());
    db.insert_swap(&record).await.unwrap();

    let txid = refund_submarine_swap_guarded(
        &db,
        "sub-refunded",
        "bc1qrefund".to_string(),
        TEST_MNEMONIC,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(txid, "already-refunded");
}

#[tokio::test]
async fn guarded_claim_reports_a_missing_swap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    let err = claim_reverse_swap_guarded(&db, "nope", TEST_MNEMONIC, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, BoltzError::NotFound { .. }));
}

/// The lock is what stops an auto-claim and a manual claim of the same swap from
/// both broadcasting, so assert it actually excludes: holders of one swap's lock
/// never overlap, while a different swap's lock stays free.
#[tokio::test]
async fn swap_lock_excludes_per_swap_and_not_across_swaps() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let live = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let live = live.clone();
        let peak = peak.clone();
        handles.push(tokio::spawn(async move {
            let _guard = lock_swap("contended").await;
            let in_section = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(in_section, Ordering::SeqCst);
            // Yield while holding the lock: a claim awaits here, and this is
            // exactly where an unguarded second caller would slip through.
            tokio::task::yield_now().await;
            live.fetch_sub(1, Ordering::SeqCst);
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    assert_eq!(
        peak.load(Ordering::SeqCst),
        1,
        "two claims of the same swap must never run concurrently"
    );

    // Holding one swap's lock must not stall an unrelated swap.
    let held = lock_swap("swap-a").await;
    let other = tokio::time::timeout(std::time::Duration::from_secs(5), lock_swap("swap-b")).await;
    assert!(other.is_ok(), "locks must be independent across swaps");
    drop(held);
}

/// Returns true if a Boltz API error indicates the endpoint is temporarily
/// unreachable (gateway/timeout), so the live test can skip rather than fail.
fn api_unavailable(err: &crate::modules::boltz::BoltzError) -> bool {
    let msg = err.to_string();
    msg.contains("502")
        || msg.contains("503")
        || msg.contains("504")
        || msg.contains("Bad Gateway")
        || msg.contains("timed out")
        || msg.contains("error sending request")
}

/// Live end-to-end test against the Boltz API. Ignored by default (needs
/// network); run with:
///   `cargo test modules::boltz -- --ignored --nocapture`
///
/// Targets testnet by default; set `BOLTZ_LIVE_NETWORK=mainnet` (or `regtest`)
/// to override. A reverse swap is ideal for an unattended live test: Boltz
/// generates the hold invoice itself, so no Lightning node or onchain funds are
/// required, and an unpaid swap simply expires. The test asserts the
/// locally-derived redeem script and invoice match what Boltz returned — the
/// core cryptographic guarantee that a later claim/refund will be valid. If the
/// endpoint is temporarily down it skips instead of failing.
#[tokio::test]
#[ignore = "hits the live Boltz API"]
async fn live_reverse_swap() {
    let network = match std::env::var("BOLTZ_LIVE_NETWORK").as_deref() {
        Ok("mainnet") => BoltzNetwork::Mainnet,
        Ok("regtest") => BoltzNetwork::Regtest,
        _ => BoltzNetwork::Testnet,
    };
    println!("live test network: {}", network.as_str());

    // 1. Real fees/limits round-trip for both directions.
    let reverse = match get_reverse_limits(network).await {
        Ok(limits) => limits,
        Err(e) if api_unavailable(&e) => {
            println!("SKIP: Boltz API unavailable ({e})");
            return;
        }
        Err(e) => panic!("fetch reverse limits: {e:?}"),
    };
    let submarine = get_submarine_limits(network)
        .await
        .expect("fetch submarine limits");
    println!(
        "submarine: min={} max={} fee%={}",
        submarine.minimal_sat, submarine.maximal_sat, submarine.fee_percentage
    );
    println!(
        "reverse:   min={} max={} fee%={}",
        reverse.minimal_sat, reverse.maximal_sat, reverse.fee_percentage
    );
    assert!(reverse.maximal_sat > reverse.minimal_sat);

    // 2. Create a real reverse swap at the minimum amount.
    let dir = tempfile::tempdir().unwrap();
    let db = BoltzDB::new(dir.path().join("boltz.db").to_str().unwrap())
        .await
        .unwrap();
    // Any valid testnet address — only stored locally as the claim destination,
    // not sent to Boltz (we never broadcast in this test).
    let claim_address = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx".to_string();
    let response = db
        .create_reverse_swap(
            network,
            "ssl://blockstream.info:993".to_string(),
            reverse.minimal_sat,
            claim_address,
            TEST_MNEMONIC.to_string(),
            None,
        )
        .await
        .expect("create reverse swap");

    println!("swap id: {}", response.id);
    println!("hold invoice: {}", response.invoice);
    println!("lockup address: {}", response.lockup_address);
    assert!(
        response.invoice.starts_with("lnbc") || response.invoice.starts_with("lntb"),
        "expected a BOLT11 invoice, got: {}",
        response.invoice
    );
    assert!(!response.lockup_address.is_empty());

    // 3. Reconstruct the swap from persisted secrets and verify the invoice's
    //    payment hash and the redeem script match — proving our claim key,
    //    preimage and locally-built swap script are correct.
    let record = db.get_swap(&response.id).await.unwrap().expect("persisted");
    let keypair = record.keypair(TEST_MNEMONIC, None).unwrap();
    let preimage = record.preimage(TEST_MNEMONIC, None).unwrap();
    let our_pubkey = bitcoin::PublicKey::new(keypair.public_key());
    let reverse_resp = record.reverse_response().unwrap();
    reverse_resp
        .validate(&preimage, &our_pubkey, network.as_chain())
        .expect("invoice + redeem script validate against our keys");

    // The swap is left to expire on testnet; nothing is broadcast.
    println!("reverse swap created and cryptographically validated ✅");
}

#[tokio::test]
async fn get_missing_swap_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    assert!(db.get_swap("nope").await.unwrap().is_none());
}
