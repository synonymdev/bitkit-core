#![allow(clippy::print_stdout)]

use crate::modules::boltz::api::{get_reverse_limits, get_submarine_limits};
use crate::modules::boltz::claim::{claim_reverse_swap_guarded, ClaimOutcome};
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::guard::lock_swap;
use crate::modules::boltz::models::{derive_swap_keypair, BoltzDB, SwapRecord};
use crate::modules::boltz::refund::refund_submarine_swap_guarded;
use crate::modules::boltz::types::{BoltzNetwork, BoltzSwapStatus, BoltzSwapType};
use crate::modules::boltz::validation::{validate_reverse_response, validate_submarine_response};
use bitcoin::absolute::LockTime;
use bitcoin::hashes::{hash160, sha256, Hash};
use bitcoin::opcodes::all::{
    OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CLTV, OP_EQUALVERIFY, OP_HASH160, OP_SIZE,
};
use bitcoin::script::Builder;
use boltz_client::network::BitcoinChain;
use boltz_client::swaps::bitcoin::BtcSwapScript;
use boltz_client::swaps::boltz::{
    CreateReverseResponse, CreateSubmarineResponse, Leaf, SwapTree, SwapType,
};
use boltz_client::util::secrets::Preimage;
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};

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

/// Block height used for the fixture responses' refund timelock.
const FIXTURE_LOCKTIME: u32 = 800_000;

/// Build a signed BOLT11 testnet invoice with the given payment hash and
/// amount. The signing key is arbitrary; the validators under test only read
/// the payment hash and amount.
fn build_test_invoice(payment_hash: sha256::Hash, amount_msat: u64) -> String {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let key = bitcoin::secp256k1::SecretKey::from_slice(&[41; 32]).unwrap();
    InvoiceBuilder::new(Currency::BitcoinTestnet)
        .description("fixture".to_string())
        .payment_hash(payment_hash)
        .payment_secret(PaymentSecret([42; 32]))
        .duration_since_epoch(std::time::Duration::from_secs(1_700_000_000))
        .min_final_cltv_expiry_delta(80)
        .amount_milli_satoshis(amount_msat)
        .build_signed(|hash| secp.sign_ecdsa_recoverable(hash, &key))
        .unwrap()
        .to_string()
}

fn x_only(pubkey: &bitcoin::PublicKey) -> bitcoin::secp256k1::XOnlyPublicKey {
    pubkey.inner.x_only_public_key().0
}

/// Canonical reverse-swap claim leaf: the hashlock guards the preimage reveal,
/// the key is ours.
fn reverse_claim_leaf(hashlock: hash160::Hash, claim_pubkey: &bitcoin::PublicKey) -> Leaf {
    let script = Builder::new()
        .push_opcode(OP_SIZE)
        .push_int(32)
        .push_opcode(OP_EQUALVERIFY)
        .push_opcode(OP_HASH160)
        .push_slice(hashlock.to_byte_array())
        .push_opcode(OP_EQUALVERIFY)
        .push_x_only_key(&x_only(claim_pubkey))
        .push_opcode(OP_CHECKSIG)
        .into_script();
    Leaf {
        output: format!("{:x}", script),
        version: 192,
    }
}

/// Canonical submarine-swap claim leaf (Boltz's side of the script).
fn submarine_claim_leaf(hashlock: hash160::Hash, claim_pubkey: &bitcoin::PublicKey) -> Leaf {
    let script = Builder::new()
        .push_opcode(OP_HASH160)
        .push_slice(hashlock.to_byte_array())
        .push_opcode(OP_EQUALVERIFY)
        .push_x_only_key(&x_only(claim_pubkey))
        .push_opcode(OP_CHECKSIG)
        .into_script();
    Leaf {
        output: format!("{:x}", script),
        version: 192,
    }
}

/// Canonical refund leaf, shared by both swap directions.
fn refund_leaf(refund_pubkey: &bitcoin::PublicKey) -> Leaf {
    let script = Builder::new()
        .push_x_only_key(&x_only(refund_pubkey))
        .push_opcode(OP_CHECKSIGVERIFY)
        .push_lock_time(LockTime::from_consensus(FIXTURE_LOCKTIME))
        .push_opcode(OP_CLTV)
        .into_script();
    Leaf {
        output: format!("{:x}", script),
        version: 192,
    }
}

/// The taproot lockup address the given script terms commit to. Fixtures use
/// this to build *internally consistent* responses (script and address match,
/// so `boltz-client`'s own validation passes) whose terms are nonetheless
/// wrong for the swap, which is exactly the malicious-server shape the extra
/// validation must catch.
fn taproot_address_for(
    swap_type: SwapType,
    hashlock: hash160::Hash,
    receiver: &bitcoin::PublicKey,
    sender: &bitcoin::PublicKey,
) -> String {
    let script = BtcSwapScript {
        swap_type,
        side: None,
        funding_addrs: None,
        hashlock,
        receiver_pubkey: *receiver,
        locktime: LockTime::from_consensus(FIXTURE_LOCKTIME),
        sender_pubkey: *sender,
    };
    script
        .to_address(BitcoinChain::BitcoinTestnet)
        .unwrap()
        .to_string()
}

/// A reverse-swap creation response whose script hashlock is `hashlock` and
/// whose invoice carries `invoice_msat`. Script, tree and lockup address are
/// kept consistent with each other so only the terms under test vary.
fn reverse_response_fixture(
    hashlock: hash160::Hash,
    our_pubkey: &bitcoin::PublicKey,
    boltz_pubkey: &bitcoin::PublicKey,
    payment_hash: sha256::Hash,
    invoice_msat: u64,
) -> CreateReverseResponse {
    CreateReverseResponse {
        id: "rev-fixture".to_string(),
        invoice: Some(build_test_invoice(payment_hash, invoice_msat)),
        swap_tree: SwapTree {
            claim_leaf: reverse_claim_leaf(hashlock, our_pubkey),
            refund_leaf: refund_leaf(boltz_pubkey),
        },
        lockup_address: taproot_address_for(
            SwapType::ReverseSubmarine,
            hashlock,
            our_pubkey,
            boltz_pubkey,
        ),
        refund_public_key: *boltz_pubkey,
        timeout_block_height: FIXTURE_LOCKTIME,
        onchain_amount: 99_000,
        blinding_key: None,
    }
}

/// A submarine-swap creation response whose script hashlock is `hashlock`.
fn submarine_response_fixture(
    hashlock: hash160::Hash,
    our_pubkey: &bitcoin::PublicKey,
    boltz_pubkey: &bitcoin::PublicKey,
) -> CreateSubmarineResponse {
    let address = taproot_address_for(SwapType::Submarine, hashlock, boltz_pubkey, our_pubkey);
    CreateSubmarineResponse {
        accept_zero_conf: false,
        bip21: format!("bitcoin:{}?amount=0.001", address),
        address,
        claim_public_key: *boltz_pubkey,
        expected_amount: 100_000,
        id: "sub-fixture".to_string(),
        referral_id: None,
        swap_tree: SwapTree {
            claim_leaf: submarine_claim_leaf(hashlock, boltz_pubkey),
            refund_leaf: refund_leaf(our_pubkey),
        },
        timeout_block_height: FIXTURE_LOCKTIME as u64,
        blinding_key: None,
    }
}

/// Our claim keypair/preimage plus a distinct "Boltz" pubkey for fixtures.
fn fixture_keys() -> (bitcoin::PublicKey, Preimage, bitcoin::PublicKey) {
    let keypair = derive_swap_keypair(TEST_MNEMONIC, None, BoltzNetwork::Testnet, 0).unwrap();
    let our_pubkey = bitcoin::PublicKey::new(keypair.public_key());
    let preimage = Preimage::from_swap_key(&keypair);
    let boltz_keypair = derive_swap_keypair(TEST_MNEMONIC, None, BoltzNetwork::Testnet, 1).unwrap();
    let boltz_pubkey = bitcoin::PublicKey::new(boltz_keypair.public_key());
    (our_pubkey, preimage, boltz_pubkey)
}

#[test]
fn reverse_response_with_consistent_terms_validates() {
    let (our_pubkey, preimage, boltz_pubkey) = fixture_keys();
    let amount_sat = 100_000;
    let response = reverse_response_fixture(
        preimage.hash160,
        &our_pubkey,
        &boltz_pubkey,
        preimage.sha256,
        amount_sat * 1000,
    );
    validate_reverse_response(
        &response,
        &preimage,
        &our_pubkey,
        amount_sat,
        BoltzNetwork::Testnet,
    )
    .expect("a consistent response must validate");
}

#[test]
fn reverse_response_with_mismatched_script_hashlock_is_rejected() {
    let (our_pubkey, preimage, boltz_pubkey) = fixture_keys();
    let amount_sat = 100_000;
    // Script and address agree with each other (so the address check passes),
    // but the hashlock is not our preimage's: our script-path claim would be
    // unspendable, leaving the funds claimable only if Boltz cooperates.
    let wrong_hashlock = hash160::Hash::hash(b"not our preimage");
    let response = reverse_response_fixture(
        wrong_hashlock,
        &our_pubkey,
        &boltz_pubkey,
        preimage.sha256,
        amount_sat * 1000,
    );
    let err = validate_reverse_response(
        &response,
        &preimage,
        &our_pubkey,
        amount_sat,
        BoltzNetwork::Testnet,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("hashlock"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn reverse_response_with_mismatched_invoice_amount_is_rejected() {
    let (our_pubkey, preimage, boltz_pubkey) = fixture_keys();
    let amount_sat = 100_000;
    // The invoice commits to our preimage but asks for more than requested.
    let response = reverse_response_fixture(
        preimage.hash160,
        &our_pubkey,
        &boltz_pubkey,
        preimage.sha256,
        (amount_sat + 1) * 1000,
    );
    let err = validate_reverse_response(
        &response,
        &preimage,
        &our_pubkey,
        amount_sat,
        BoltzNetwork::Testnet,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("invoice amount"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn submarine_response_with_consistent_terms_validates() {
    let (our_pubkey, _, boltz_pubkey) = fixture_keys();
    let payment_hash = sha256::Hash::hash(b"submarine payment preimage");
    let invoice = build_test_invoice(payment_hash, 100_000_000);
    // The correct hashlock is hash160(preimage) = ripemd160(payment_hash).
    let hashlock = Preimage::from_sha256_str(&payment_hash.to_string())
        .unwrap()
        .hash160;
    let response = submarine_response_fixture(hashlock, &our_pubkey, &boltz_pubkey);
    validate_submarine_response(&response, &invoice, &our_pubkey, BoltzNetwork::Testnet)
        .expect("a consistent response must validate");
}

#[test]
fn submarine_response_with_mismatched_script_hashlock_is_rejected() {
    let (our_pubkey, _, boltz_pubkey) = fixture_keys();
    let payment_hash = sha256::Hash::hash(b"submarine payment preimage");
    let invoice = build_test_invoice(payment_hash, 100_000_000);
    // Consistent script/address pair whose hashlock does not match the
    // invoice: Boltz could claim the lockup with its own preimage without
    // ever paying the invoice.
    let wrong_hashlock = hash160::Hash::hash(b"a preimage boltz controls");
    let response = submarine_response_fixture(wrong_hashlock, &our_pubkey, &boltz_pubkey);
    let err = validate_submarine_response(&response, &invoice, &our_pubkey, BoltzNetwork::Testnet)
        .unwrap_err();
    assert!(
        err.to_string().contains("hashlock"),
        "unexpected error: {}",
        err
    );
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

    // Recording the refund tx id does the same for the submarine swap, and
    // persists the refund destination as the swap's onchain address.
    db.set_refund_tx("sub-1", "def456", "bc1qrefund")
        .await
        .unwrap();
    let loaded = db.get_swap("sub-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, "transaction.refunded");
    assert_eq!(loaded.refund_tx_id, Some("def456".to_string()));
    assert_eq!(loaded.onchain_address, Some("bc1qrefund".to_string()));
    assert!(db.list_pending_swaps().await.unwrap().is_empty());

    // Terminal swaps are still listed, just no longer pending.
    assert_eq!(db.list_swaps().await.unwrap().len(), 2);
}

#[tokio::test]
async fn settled_reverse_swap_without_local_claim_stays_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    let record = sample_record("rev-settled", BoltzSwapType::Reverse);
    db.insert_swap(&record).await.unwrap();

    // Boltz reporting the invoice settled proves the Lightning leg, not our
    // onchain claim: the cooperative flow discloses the preimage before
    // broadcast, so the claim tx may never have made it out. Without a local
    // claim txid the swap must stay recoverable.
    db.update_status("rev-settled", "invoice.settled")
        .await
        .unwrap();
    let pending = db.list_pending_swaps().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "rev-settled");

    // Once the claim txid is recorded locally, the swap is complete.
    db.set_claim_tx("rev-settled", "claim-txid").await.unwrap();
    assert!(db.list_pending_swaps().await.unwrap().is_empty());
}

#[tokio::test]
async fn local_completion_survives_late_server_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();

    let reverse = sample_record("rev-1", BoltzSwapType::Reverse);
    let submarine = sample_record("sub-1", BoltzSwapType::Submarine);
    db.insert_swap(&reverse).await.unwrap();
    db.insert_swap(&submarine).await.unwrap();

    // A delayed or re-ordered server status (WebSocket lag, reconcile) must
    // not regress a locally recorded completion.
    db.set_claim_tx("rev-1", "claim-txid").await.unwrap();
    db.update_status("rev-1", "transaction.confirmed")
        .await
        .unwrap();
    let loaded = db.get_swap("rev-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, "transaction.claimed");
    assert_eq!(loaded.claim_tx_id, Some("claim-txid".to_string()));

    db.set_refund_tx("sub-1", "refund-txid", "bc1qrefund")
        .await
        .unwrap();
    db.update_status("sub-1", "invoice.failedToPay")
        .await
        .unwrap();
    let loaded = db.get_swap("sub-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, "transaction.refunded");
    assert_eq!(loaded.refund_tx_id, Some("refund-txid".to_string()));
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
    // A valid address for the selected network, only stored locally as the
    // claim destination, not sent to Boltz (we never broadcast in this test).
    let claim_address = match network {
        BoltzNetwork::Mainnet => "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
        BoltzNetwork::Testnet => "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx",
        BoltzNetwork::Regtest => "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080",
    }
    .to_string();
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
