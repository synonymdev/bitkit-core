//! Drive and store tests against a mock wallet, using the oracle fixture's real quotes so the
//! lockups are the ones the solver quoted. No network.

use super::client::IntentsClient;
use super::contract::*;
use super::error::WalletError;
use super::preimage::*;
use super::spend::preimage_in;
use super::store::*;
use super::transport::SolverCard;
use super::wallet::ArkWallet;
use super::wire::RfqQuote;
use ark_core::server::Info;
use ark_core::server::VirtualTxOutPoint;
use ark_core::ArkAddress;
use async_trait::async_trait;
use bitcoin::bip32::DerivationPath;
use bitcoin::bip32::Xpriv;
use bitcoin::key::Secp256k1;
use bitcoin::secp256k1::Keypair;
use bitcoin::Amount;
use bitcoin::Network;
use bitcoin::OutPoint;
use bitcoin::Psbt;
use bitcoin::ScriptBuf;
use bitcoin::Txid;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap()
}

#[derive(Default)]
struct Mock {
    keypair: Option<Keypair>,
    address: Option<ArkAddress>,
    vtxos: Mutex<Vec<VirtualTxOutPoint>>,
    virtual_txs: Mutex<HashMap<Txid, Psbt>>,
    submitted: Mutex<Vec<Psbt>>,
    finalized: Mutex<Vec<Txid>>,
    sent: Mutex<Vec<(ArkAddress, Amount)>>,
}

fn info() -> Info {
    let f = fixture();
    let signer =
        bitcoin::secp256k1::PublicKey::from_str(f["server"]["signerPubkey"].as_str().unwrap())
            .unwrap();
    let checkpoint_tapscript = ScriptBuf::builder()
        .push_int(i64::from(
            bitcoin::Sequence::from_512_second_intervals(1).to_consensus_u32(),
        ))
        .push_opcode(bitcoin::opcodes::all::OP_CSV)
        .push_opcode(bitcoin::opcodes::all::OP_DROP)
        .push_x_only_key(&signer.x_only_public_key().0)
        .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
        .into_script();
    Info {
        version: "test".into(),
        signer_pk: signer,
        forfeit_pk: signer,
        forfeit_address: bitcoin::Address::from_str("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq")
            .unwrap()
            .assume_checked(),
        checkpoint_tapscript,
        network: Network::Bitcoin,
        session_duration: 60,
        unilateral_exit_delay: ark_core::server::parse_sequence_number(605184).unwrap(),
        boarding_exit_delay: ark_core::server::parse_sequence_number(605184).unwrap(),
        utxo_min_amount: None,
        utxo_max_amount: None,
        vtxo_min_amount: None,
        vtxo_max_amount: None,
        dust: Amount::from_sat(330),
        fees: None,
        scheduled_session: None,
        deprecated_signers: vec![],
        service_status: Default::default(),
        digest: String::new(),
        max_tx_weight: 0,
        max_op_return_outputs: 0,
    }
}

#[async_trait]
impl ArkWallet for Mock {
    async fn server_info(&self) -> Result<Info, WalletError> {
        Ok(info())
    }
    async fn address(&self) -> Result<ArkAddress, WalletError> {
        Ok(self.address.unwrap())
    }
    fn identity_keypair(&self) -> Result<Keypair, WalletError> {
        Ok(self.keypair.unwrap())
    }
    async fn vtxos(&self, _: &ArkAddress) -> Result<Vec<VirtualTxOutPoint>, WalletError> {
        Ok(self.vtxos.lock().unwrap().clone())
    }
    async fn virtual_tx(&self, txid: Txid) -> Result<Option<Psbt>, WalletError> {
        Ok(self.virtual_txs.lock().unwrap().get(&txid).cloned())
    }
    async fn submit_offchain(
        &self,
        ark_tx: Psbt,
        checkpoints: Vec<Psbt>,
    ) -> Result<Vec<Psbt>, WalletError> {
        self.submitted.lock().unwrap().push(ark_tx);
        Ok(checkpoints)
    }
    async fn finalize_offchain(&self, txid: Txid, _: Vec<Psbt>) -> Result<(), WalletError> {
        self.finalized.lock().unwrap().push(txid);
        Ok(())
    }
    async fn send(&self, address: &ArkAddress, amount: Amount) -> Result<Txid, WalletError> {
        self.sent.lock().unwrap().push((*address, amount));
        Ok(Txid::from_str(&"11".repeat(32)).unwrap())
    }
}

fn identity(mnemonic: &str) -> Keypair {
    let seed = bip39::Mnemonic::parse(mnemonic).unwrap().to_seed("");
    let secp = Secp256k1::new();
    Xpriv::new_master(Network::Bitcoin, &seed)
        .unwrap()
        .derive_priv(&secp, &DerivationPath::from_str("m/86'/0'/1'/0/0").unwrap())
        .unwrap()
        .to_keypair(&secp)
}

fn vtxo(script: ScriptBuf, sats: u64, vout: u32) -> VirtualTxOutPoint {
    VirtualTxOutPoint {
        outpoint: OutPoint {
            txid: Txid::from_str(&"22".repeat(32)).unwrap(),
            vout,
        },
        created_at: 0,
        expires_at: i64::MAX,
        amount: Amount::from_sat(sats),
        script,
        is_preconfirmed: true,
        is_swept: false,
        is_unrolled: false,
        is_spent: false,
        spent_by: None,
        commitment_txids: vec![],
        settled_by: None,
        ark_txid: None,
        assets: vec![],
        depth: 1,
    }
}

/// A receive record and a client over a mock wallet, as `receive()` would have persisted it.
fn receive_setup(
    refund_locktime_shift: i64,
) -> (
    Arc<Mock>,
    Arc<JsonStore>,
    IntentsClient<Mock, JsonStore>,
    SwapRecord,
) {
    let f = fixture();
    let r = &f["receive"];
    let keypair = identity(f["mnemonic"].as_str().unwrap());
    let payout = ArkAddress::decode(r["payoutAddress"].as_str().unwrap()).unwrap();
    let mut quote: RfqQuote = serde_json::from_value(r["quote"].clone()).unwrap();
    let terms = Terms::from_server_info(&info()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut lockup = derive_receive(
        &quote,
        r["paymentHash"].as_str().unwrap(),
        keypair.x_only_public_key().0,
        &payout,
        &terms,
    )
    .unwrap();
    // Move the clock: rebuild the lockup around a locktime relative to now (the address changes,
    // which is fine for the drive, which reads the persisted params).
    lockup.params.refund_locktime = (now as i64 + refund_locktime_shift) as u32;
    quote.refund_locktime = Some(u64::from(lockup.params.refund_locktime));
    let address = lockup.params.build().unwrap().address(Network::Bitcoin);
    let record = SwapRecord {
        id: quote.rfq_id.clone(),
        direction: Direction::Receive,
        outcome: Outcome::Open,
        give_sat: 1000,
        take_sat: 995,
        fee_sat: 5,
        bolt11: lockup.invoice.clone(),
        payment_hash: r["paymentHash"].as_str().unwrap().into(),
        created_at: now,
        updated_at: now,
        blocked_reason: None,
        expires_at: now + 600,
        solver: "beta-solver".into(),
        rfq_id: quote.rfq_id.clone(),
        quote: serde_json::to_value(&quote).unwrap(),
        lockup: lockup.params.clone(),
        lockup_address: address.encode(),
        preimage_salt: Some(r["preimageSalt"].as_str().unwrap().into()),
        preimage: None,
        payout_address: payout.encode(),
        funding_txid: None,
        claim_txid: None,
        refund_txid: None,
    };
    let wallet = Arc::new(Mock {
        keypair: Some(keypair),
        address: Some(payout),
        ..Default::default()
    });
    let store = Arc::new(JsonStore::memory());
    store.put(&record).unwrap();
    let client = IntentsClient::new(wallet.clone(), store.clone(), SolverCard::beta_solver());
    (wallet, store, client, record)
}

#[tokio::test]
async fn receive_waits_then_claims_with_the_preimage() {
    let (wallet, store, client, record) = receive_setup(3600);
    let updates = Arc::new(Mutex::new(Vec::new()));
    let seen = updates.clone();
    client.on_update(Arc::new(move |r: &SwapRecord| {
        seen.lock().unwrap().push(r.outcome)
    }));

    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Open
    );

    // Underfunded: the preimage must not go out.
    let script = record.lockup.build().unwrap().script_pubkey();
    *wallet.vtxos.lock().unwrap() = vec![vtxo(script.clone(), 994, 0)];
    client.drive_once().await.unwrap();
    let held = store.get(&record.id).unwrap().unwrap();
    assert_eq!(held.outcome, Outcome::NeedsRecovery);
    assert!(held
        .blocked_reason
        .unwrap()
        .contains("below the agreed 995"));
    assert!(wallet.submitted.lock().unwrap().is_empty());

    // Funded in full (split over two outputs): claimed through the claim leaf.
    *wallet.vtxos.lock().unwrap() = vec![vtxo(script.clone(), 500, 0), vtxo(script, 495, 1)];
    client.drive_once().await.unwrap();
    let claimed = store.get(&record.id).unwrap().unwrap();
    assert_eq!(claimed.outcome, Outcome::Claimed);
    let submitted = wallet.submitted.lock().unwrap();
    assert_eq!(submitted.len(), 1);
    let ark_tx = &submitted[0];
    assert_eq!(ark_tx.inputs.len(), 2);
    let hash = hex_32(&record.payment_hash, "hash").unwrap();
    assert!(preimage_in(ark_tx, &hash).is_some(), "the claim carries P");
    let me = wallet.keypair.unwrap().x_only_public_key().0;
    let claim_leaf = record.lockup.build().unwrap().claim_script().clone();
    for input in &ark_tx.inputs {
        assert!(input.tap_script_sigs.keys().any(|(pk, _)| *pk == me));
        assert!(input.tap_scripts.values().any(|(s, _)| *s == claim_leaf));
    }
    let payout = ArkAddress::decode(&record.payout_address).unwrap();
    assert_eq!(
        ark_tx.unsigned_tx.output[0].script_pubkey,
        payout.to_p2tr_script_pubkey()
    );
    assert_eq!(ark_tx.unsigned_tx.output[0].value, Amount::from_sat(995));
    assert_eq!(
        claimed.claim_txid,
        Some(ark_tx.unsigned_tx.compute_txid().to_string())
    );
    assert_eq!(
        *wallet.finalized.lock().unwrap(),
        vec![ark_tx.unsigned_tx.compute_txid()]
    );
    assert_eq!(
        *updates.lock().unwrap(),
        vec![Outcome::NeedsRecovery, Outcome::Claimed]
    );
}

#[tokio::test]
async fn unpaid_receive_lapses_after_the_window() {
    let (_, store, client, record) = receive_setup(-60);
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::NeedsRecovery
    );

    let (_, store, client, record) = receive_setup(-(REFUND_MTP_LAG_SECONDS as i64) - 60);
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Lapsed
    );
}

#[tokio::test]
async fn receive_reclaimed_by_the_solver_is_lapsed() {
    let (wallet, store, client, record) = receive_setup(3600);
    let script = record.lockup.build().unwrap().script_pubkey();
    let mut spent = vtxo(script, 995, 0);
    spent.is_spent = true;
    let spender = Txid::from_str(&"33".repeat(32)).unwrap();
    spent.spent_by = Some(spender);
    *wallet.vtxos.lock().unwrap() = vec![spent];
    // Unknown spender: nothing learned.
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Open
    );
    wallet
        .virtual_txs
        .lock()
        .unwrap()
        .insert(spender, empty_psbt());
    // A spend with neither our preimage nor a payout to the solver's refund script: surfaced.
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::NeedsRecovery
    );
    // The solver's reclaim pays its refund script: lapsed.
    let mut reclaim = empty_psbt();
    reclaim.unsigned_tx.output.push(bitcoin::TxOut {
        value: Amount::from_sat(995),
        script_pubkey: record.lockup.sender_pk_script.clone(),
    });
    reclaim.outputs.push(Default::default());
    wallet.virtual_txs.lock().unwrap().insert(spender, reclaim);
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Lapsed
    );
}

fn empty_psbt() -> Psbt {
    Psbt::from_unsigned_tx(bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn::default()],
        output: vec![],
    })
    .unwrap()
}

/// A funded send record over the fixture's send quote.
fn send_setup(
    refund_locktime_shift: i64,
) -> (
    Arc<Mock>,
    Arc<JsonStore>,
    IntentsClient<Mock, JsonStore>,
    SwapRecord,
) {
    let f = fixture();
    let s = &f["send"];
    let keypair = identity(f["mnemonic"].as_str().unwrap());
    let refund = ArkAddress::decode(s["refund"]["address"].as_str().unwrap()).unwrap();
    let quote: RfqQuote = serde_json::from_value(s["quote"].clone()).unwrap();
    let bolt11 = s["request"]["profile"]["invoice"].as_str().unwrap();
    let invoice = super::bolt11::decode(bolt11).unwrap();
    let terms = Terms::from_server_info(&info()).unwrap();
    let lockup = derive_send(
        &quote,
        &invoice.payment_hash,
        keypair.x_only_public_key().0,
        &refund,
        &terms,
        f["capturedAt"].as_u64().unwrap(),
    )
    .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut params = lockup.params.clone();
    params.refund_locktime = (now as i64 + refund_locktime_shift) as u32;
    let record = SwapRecord {
        id: quote.rfq_id.clone(),
        direction: Direction::Send,
        outcome: Outcome::Funded,
        give_sat: 1004,
        take_sat: 1000,
        fee_sat: 4,
        bolt11: bolt11.into(),
        payment_hash: invoice.payment_hash.clone(),
        created_at: now,
        updated_at: now,
        blocked_reason: None,
        expires_at: now + 600,
        solver: "beta-solver".into(),
        rfq_id: quote.rfq_id.clone(),
        quote: serde_json::to_value(&quote).unwrap(),
        lockup: params.clone(),
        lockup_address: params.build().unwrap().address(Network::Bitcoin).encode(),
        preimage_salt: None,
        preimage: None,
        payout_address: String::new(),
        funding_txid: Some("11".repeat(32)),
        claim_txid: None,
        refund_txid: None,
    };
    let wallet = Arc::new(Mock {
        keypair: Some(keypair),
        address: Some(refund),
        ..Default::default()
    });
    let store = Arc::new(JsonStore::memory());
    store.put(&record).unwrap();
    let client = IntentsClient::new(wallet.clone(), store.clone(), SolverCard::beta_solver());
    (wallet, store, client, record)
}

#[tokio::test]
async fn send_is_paid_when_the_solver_claim_reveals_the_preimage() {
    let (wallet, store, client, record) = send_setup(3600);
    let script = record.lockup.build().unwrap().script_pubkey();
    *wallet.vtxos.lock().unwrap() = vec![vtxo(script.clone(), 1004, 0)];
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Funded
    );

    // The solver's claim checkpoint carries the preimage in its condition witness.
    let preimage = [42u8; 32];
    let mut record = record;
    record.payment_hash = hex::encode(payment_hash(&preimage));
    store.put(&record).unwrap();
    let mut checkpoint = empty_psbt();
    checkpoint.inputs[0].unknown.insert(
        bitcoin::psbt::raw::Key {
            type_value: 222,
            key: ark_core::VTXO_CONDITION_KEY.to_vec(),
        },
        [vec![1u8, 32], preimage.to_vec()].concat(),
    );
    let spender = Txid::from_str(&"44".repeat(32)).unwrap();
    wallet
        .virtual_txs
        .lock()
        .unwrap()
        .insert(spender, checkpoint);
    let mut spent = vtxo(script, 1004, 0);
    spent.is_spent = true;
    spent.spent_by = Some(spender);
    *wallet.vtxos.lock().unwrap() = vec![spent];
    client.drive_once().await.unwrap();
    let paid = store.get(&record.id).unwrap().unwrap();
    assert_eq!(paid.outcome, Outcome::Paid);
    assert_eq!(paid.preimage, Some(hex::encode(preimage)));
}

#[tokio::test]
async fn send_refunds_after_the_locktime() {
    let (wallet, store, client, record) = send_setup(-60);
    let script = record.lockup.build().unwrap().script_pubkey();
    *wallet.vtxos.lock().unwrap() = vec![vtxo(script, 1004, 0)];
    client.drive_once().await.unwrap();
    let refunded = store.get(&record.id).unwrap().unwrap();
    assert_eq!(refunded.outcome, Outcome::Refunded);
    let submitted = wallet.submitted.lock().unwrap();
    let ark_tx = &submitted[0];
    let leaf = record
        .lockup
        .build()
        .unwrap()
        .refund_without_receiver_script()
        .clone();
    assert!(ark_tx.inputs[0]
        .tap_scripts
        .values()
        .any(|(s, _)| *s == leaf));
    assert!(preimage_in(ark_tx, &[0; 32]).is_none());
    assert_eq!(
        ark_tx.unsigned_tx.lock_time.to_consensus_u32(),
        record.lockup.refund_locktime
    );
    assert_eq!(ark_tx.unsigned_tx.output[0].value, Amount::from_sat(1004));
    assert_eq!(
        ark_tx.unsigned_tx.output[0].script_pubkey,
        wallet.address.unwrap().to_p2tr_script_pubkey()
    );
}

#[test]
fn store_persists_and_round_trips_backups() {
    let (_, store, _, record) = receive_setup(3600);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("swaps.json");
    let file = JsonStore::open(&path).unwrap();
    assert_eq!(file.import(&store.export().unwrap()).unwrap(), 1);
    drop(file);
    let reopened = JsonStore::open(&path).unwrap();
    assert_eq!(reopened.get(&record.id).unwrap().unwrap(), record);
    // Re-importing the same backup writes nothing; a newer copy wins.
    assert_eq!(reopened.import(&store.export().unwrap()).unwrap(), 0);
    let mut newer = record.clone();
    newer.outcome = Outcome::Claimed;
    newer.updated_at += 1;
    store.put(&newer).unwrap();
    assert_eq!(reopened.import(&store.export().unwrap()).unwrap(), 1);
    assert_eq!(
        reopened.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Claimed
    );
    // The record carries no secret: only the public salt.
    let json = reopened.export().unwrap();
    let preimage = salted_preimage(
        &identity(fixture()["mnemonic"].as_str().unwrap()),
        &hex_32(record.preimage_salt.as_deref().unwrap(), "salt").unwrap(),
    );
    assert!(!json.contains(&hex::encode(preimage)));
}

/// Each swap goes through the Arkade backup (`arkade_export_state` into a fresh `arkade_import_state`) whole, and
/// the restored record still drives: a funded receive claims, an expired send refunds.
#[tokio::test]
async fn swaps_survive_the_arkade_backup_and_still_drive() {
    use super::super::store::{export, import, SqliteSwaps};

    let restore = |record: &SwapRecord| {
        let from = tempfile::tempdir().unwrap();
        SqliteSwaps::open(from.path()).unwrap().put(record).unwrap();
        let dump = export(from.path()).unwrap();
        let to = tempfile::tempdir().unwrap();
        assert_eq!(import(to.path(), &dump).unwrap(), 1);
        let store = Arc::new(SqliteSwaps::open(to.path()).unwrap());
        assert_eq!(store.get(&record.id).unwrap().as_ref(), Some(record));
        (to, store)
    };

    let (wallet, _, _, record) = receive_setup(3600);
    assert!(record.preimage_salt.is_some());
    let (_dir, store) = restore(&record);
    let client = IntentsClient::new(wallet.clone(), store.clone(), SolverCard::beta_solver());
    let script = record.lockup.build().unwrap().script_pubkey();
    *wallet.vtxos.lock().unwrap() = vec![vtxo(script, 995, 0)];
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Claimed
    );
    let claim = &wallet.submitted.lock().unwrap()[0];
    let hash = hex_32(&record.payment_hash, "hash").unwrap();
    assert!(preimage_in(claim, &hash).is_some(), "the salt re-derives P");

    let (wallet, _, _, record) = send_setup(-60);
    let (_dir, store) = restore(&record);
    let client = IntentsClient::new(wallet.clone(), store.clone(), SolverCard::beta_solver());
    let script = record.lockup.build().unwrap().script_pubkey();
    *wallet.vtxos.lock().unwrap() = vec![vtxo(script, 1004, 0)];
    client.drive_once().await.unwrap();
    assert_eq!(
        store.get(&record.id).unwrap().unwrap().outcome,
        Outcome::Refunded
    );
    let refund = &wallet.submitted.lock().unwrap()[0];
    assert_eq!(
        refund.unsigned_tx.lock_time.to_consensus_u32(),
        record.lockup.refund_locktime
    );
}
