use super::*;
use bitcoin::{consensus, Amount, OutPoint, TxOut, Txid};
use boltz_client::swaps::bitcoin::{BtcSwapScript, BtcSwapTx};
use boltz_client::swaps::boltz::{CreateReverseRequest, CreateSubmarineRequest, SwapTxKind};
use boltz_client::util::fees::Fee;
use pubky_swap_boltz::model::{CreateRequest, ReverseRequest, SubmarineRequest};
use std::str::FromStr;

#[path = "pubky_fixture.rs"]
mod fixture;

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn intent(request: CreateRequest, claim_address: Option<String>) -> CreationIntent {
    CreationIntent {
        id: uuid::Uuid::new_v4().to_string(),
        request_key: "same-intent".into(),
        wallet_fingerprint: wallet_fingerprint(MNEMONIC, None, BoltzNetwork::Regtest).unwrap(),
        backend_binding: "fixture-binding".into(),
        network: BoltzNetwork::Regtest,
        electrum_url: "tcp://127.0.0.1:1".into(),
        swap_index: 0,
        recipient_amount_sat: None,
        claim_address,
        created_at: now_secs(),
        request,
    }
}

fn destination(keys: &boltz_client::Keypair) -> bitcoin::Address {
    bitcoin::Address::p2tr(
        &bitcoin::secp256k1::Secp256k1::new(),
        keys.x_only_public_key().0,
        None,
        bitcoin::Network::Regtest,
    )
}

fn assert_signed_spends(
    transaction: &bitcoin::Transaction,
    previous_outputs: &[TxOut],
    keys: &boltz_client::Keypair,
) {
    use bitcoin::sighash::{Prevouts, SighashCache};
    use bitcoin::taproot::{ControlBlock, LeafVersion, Signature, TapLeafHash};
    assert_eq!(transaction.input.len(), previous_outputs.len());
    let secp = bitcoin::secp256k1::Secp256k1::new();
    for (index, (input, previous_output)) in
        transaction.input.iter().zip(previous_outputs).enumerate()
    {
        let witness: Vec<_> = input.witness.iter().collect();
        let signature = Signature::from_slice(witness[0]).unwrap();
        let script = bitcoin::ScriptBuf::from_bytes(witness[witness.len() - 2].to_vec());
        let control = ControlBlock::decode(witness[witness.len() - 1]).unwrap();
        let output_key = bitcoin::secp256k1::XOnlyPublicKey::from_slice(
            &previous_output.script_pubkey.as_bytes()[2..],
        )
        .unwrap();
        assert!(control.verify_taproot_commitment(&secp, output_key, &script));
        let hash = SighashCache::new(transaction)
            .taproot_script_spend_signature_hash(
                index,
                &Prevouts::All(previous_outputs),
                TapLeafHash::from_script(&script, LeafVersion::TapScript),
                signature.sighash_type,
            )
            .unwrap();
        secp.verify_schnorr(
            &signature.signature,
            &bitcoin::secp256k1::Message::from_digest(hash.to_byte_array()),
            &keys.x_only_public_key().0,
        )
        .unwrap();
    }
    let input_total: u64 = previous_outputs
        .iter()
        .map(|output| output.value.to_sat())
        .sum();
    assert!(transaction.output[0].value.to_sat() < input_total);
    assert_eq!(
        consensus::deserialize::<bitcoin::Transaction>(&consensus::serialize(transaction)).unwrap(),
        *transaction
    );
}

async fn assert_wrong_amount_refunds(
    record: &SwapRecord,
    keys: &boltz_client::Keypair,
    source: &TxOut,
) {
    use crate::modules::boltz::claim::pubky_transaction_from_outputs;
    for amounts in [vec![1_000], vec![150_000], vec![1_000, 150_000, 25_000]] {
        let outputs: Vec<_> = amounts
            .iter()
            .map(|amount| TxOut {
                value: Amount::from_sat(*amount),
                script_pubkey: source.script_pubkey.clone(),
            })
            .collect();
        let utxos: Vec<_> = outputs
            .iter()
            .enumerate()
            .map(|(index, output)| {
                (
                    OutPoint {
                        txid: Txid::from_str(&"33".repeat(32)).unwrap(),
                        vout: index as u32,
                    },
                    output.clone(),
                )
            })
            .collect();
        let spend = pubky_transaction_from_outputs(
            record,
            keys,
            &destination(keys).to_string(),
            true,
            utxos.clone(),
        )
        .unwrap();
        let signed = spend
            .sign_refund(keys, Fee::Absolute(500), None)
            .await
            .unwrap();
        assert_eq!(
            signed.output[0].value.to_sat(),
            amounts.iter().sum::<u64>() - 500
        );
        assert_eq!(signed.lock_time.to_consensus_u32(), 244);
        assert_signed_spends(&signed, &outputs, keys);
        let duplicate = vec![utxos[0].clone(), utxos[0].clone()];
        assert!(pubky_transaction_from_outputs(
            record,
            keys,
            &destination(keys).to_string(),
            true,
            duplicate
        )
        .is_err());
        let mut foreign = utxos;
        foreign[0].1.script_pubkey = bitcoin::ScriptBuf::new();
        assert!(pubky_transaction_from_outputs(
            record,
            keys,
            &destination(keys).to_string(),
            true,
            foreign
        )
        .is_err());
    }
}

#[tokio::test]
async fn rust_sdk_creates_validates_and_unilaterally_spends_both_bridge_directions() {
    let bridge = fixture::fixture_bridge().unwrap();
    let submarine_pairs = bridge.pairs(SwapDirection::Submarine).await.unwrap();
    let pair: SubmarinePair =
        serde_json::from_value(submarine_pairs["BTC"]["BTC"].clone()).unwrap();
    assert_eq!(pair.limits.minimal, 10_000);
    let reverse_pairs = bridge.pairs(SwapDirection::Reverse).await.unwrap();
    let pair: ReversePair = serde_json::from_value(reverse_pairs["BTC"]["BTC"].clone()).unwrap();
    assert_eq!(pair.fees.miner_fees.lockup, 0);
    let keys = derive_swap_keypair(MNEMONIC, None, BoltzNetwork::Regtest, 0).unwrap();
    let public_key = bitcoin::PublicKey::new(keys.public_key());
    let preimage = Preimage::from_swap_key(&keys);
    let invoice = lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Regtest)
        .description("compatibility fixture".into())
        .payment_hash(sha256::Hash::hash(&[3; 32]))
        .payment_secret(lightning_invoice::PaymentSecret([7; 32]))
        .current_timestamp()
        .expiry_time(std::time::Duration::from_secs(86_400))
        .min_final_cltv_expiry_delta(240)
        .amount_milli_satoshis(100_000_000)
        .build_signed(|message| {
            bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(message, &keys.secret_key())
        })
        .unwrap()
        .to_string();
    let submarine = CreateSubmarineRequest {
        from: "BTC".into(),
        to: "BTC".into(),
        invoice,
        refund_public_key: public_key,
        pair_hash: None,
        referral_id: None,
        webhook: None,
    };
    let reverse = CreateReverseRequest {
        from: "BTC".into(),
        to: "BTC".into(),
        claim_public_key: public_key,
        invoice: None,
        invoice_amount: Some(100_000),
        preimage_hash: Some(preimage.sha256),
        description: None,
        description_hash: None,
        address: None,
        address_signature: None,
        referral_id: None,
        webhook: None,
    };
    let requests = [
        CreateRequest::Submarine(
            serde_json::from_value::<SubmarineRequest>(serde_json::to_value(submarine).unwrap())
                .unwrap(),
        ),
        CreateRequest::Reverse(
            serde_json::from_value::<ReverseRequest>(serde_json::to_value(reverse).unwrap())
                .unwrap(),
        ),
    ];
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    for (index, request) in requests.into_iter().enumerate() {
        let refund = matches!(request, CreateRequest::Submarine(_));
        let mut pending = intent(
            request.clone(),
            if refund {
                None
            } else {
                Some(destination(&keys).to_string())
            },
        );
        pending.request_key = index.to_string();
        pending.recipient_amount_sat = if refund { None } else { Some(90_000) };
        let pending = db.save_intent(&pending).await.unwrap();
        let response = bridge
            .create(request.clone(), Some(pending.id.clone()))
            .await
            .unwrap();
        let replay = bridge
            .create(request, Some(pending.id.clone()))
            .await
            .unwrap();
        assert_eq!(response, replay);
        let record = record_from_response(&pending, &response, &keys).unwrap();
        assert_eq!(record.swap_index, 0);
        assert_eq!(record.backend_binding.as_deref(), Some("fixture-binding"));
        let script = if refund {
            BtcSwapScript::submarine_from_swap_resp(
                &record.submarine_response().unwrap(),
                public_key,
            )
            .unwrap()
        } else {
            BtcSwapScript::reverse_from_swap_resp(&record.reverse_response().unwrap(), public_key)
                .unwrap()
        };
        let output = TxOut {
            value: Amount::from_sat(100_000),
            script_pubkey: script
                .to_address(record.network.as_bitcoin_chain())
                .unwrap()
                .script_pubkey(),
        };
        let spend = BtcSwapTx {
            kind: if refund {
                SwapTxKind::Refund
            } else {
                SwapTxKind::Claim
            },
            swap_script: script,
            output_address: destination(&keys),
            utxos: vec![(
                OutPoint {
                    txid: Txid::from_str(&"11".repeat(32)).unwrap(),
                    vout: 0,
                },
                output.clone(),
            )],
        };
        let signed = if refund {
            spend
                .sign_refund(&keys, Fee::Absolute(500), None)
                .await
                .unwrap()
        } else {
            spend
                .sign_claim(
                    &keys,
                    &preimage,
                    crate::modules::boltz::send::claim_fee(&record, Some(100.0)).unwrap(),
                    None,
                )
                .await
                .unwrap()
        };
        assert_eq!(signed.input[0].witness.len(), if refund { 3 } else { 4 });
        if refund {
            assert_eq!(signed.lock_time.to_consensus_u32(), 244);
        } else {
            assert_eq!(
                sha256::Hash::hash(signed.input[0].witness.nth(1).unwrap()),
                preimage.sha256
            );
        }
        crate::modules::boltz::send::validate_claim_output(&record, &signed).unwrap();
        assert_signed_spends(&signed, std::slice::from_ref(&output), &keys);
        if refund {
            assert_wrong_amount_refunds(&record, &keys, &output).await;
        } else {
            for wrong_amount in [90_000, 110_001] {
                let mut wrong_output = output.clone();
                wrong_output.value = Amount::from_sat(wrong_amount);
                assert!(
                    crate::modules::boltz::claim::pubky_transaction_from_outputs(
                        &record,
                        &keys,
                        &destination(&keys).to_string(),
                        false,
                        vec![(
                            OutPoint {
                                txid: Txid::from_str(&"44".repeat(32)).unwrap(),
                                vout: 0
                            },
                            wrong_output
                        )],
                    )
                    .is_err()
                );
            }
        }
        db.complete_intent(&pending, &record).await.unwrap();
        db.complete_intent(&pending, &record).await.unwrap();
    }
    assert!(db.creation_intents().await.unwrap().is_empty());
    drop(db);
    let restored = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    assert_eq!(restored.list_swaps().await.unwrap().len(), 2);
}

#[tokio::test]
async fn interrupted_creation_keeps_its_key_and_destination_after_restart() {
    let keys = derive_swap_keypair(MNEMONIC, None, BoltzNetwork::Regtest, 0).unwrap();
    let request = CreateRequest::Reverse(ReverseRequest {
        from: "BTC".into(),
        to: "BTC".into(),
        invoice_amount: 100_000,
        onchain_amount: 0,
        preimage_hash: Preimage::from_swap_key(&keys).sha256.to_string(),
        claim_public_key: bitcoin::PublicKey::new(keys.public_key()).to_string(),
        pair_hash: String::new(),
        referral_id: String::new(),
        error: String::new(),
    });
    let original = intent(request, Some(destination(&keys).to_string()));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("boltz.db");
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    db.save_intent(&original).await.unwrap();
    let bridge = fixture::fixture_bridge().unwrap();
    // The provider accepted, but the application died before inserting its record.
    let accepted = bridge
        .create(original.request.clone(), Some(original.id.clone()))
        .await
        .unwrap();
    drop(db);
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    let mut retry = original.clone();
    retry.id = uuid::Uuid::new_v4().to_string();
    retry.swap_index = 123;
    retry.request_key = "new-key-after-server-change".into();
    retry.electrum_url = "ssl://replacement.example:50002".into();
    let persisted = db.save_intent(&retry).await.unwrap();
    assert_eq!(persisted.id, original.id);
    assert_eq!(persisted.swap_index, 0);
    assert_eq!(persisted.claim_address, original.claim_address);
    assert_eq!(
        persisted.request.fingerprint().unwrap(),
        original.request.fingerprint().unwrap()
    );
    assert_eq!(db.creation_intents().await.unwrap().len(), 1);
    let restored_response = db
        .negotiate_intent(&persisted, &keys, &bridge)
        .await
        .unwrap();
    assert_eq!(restored_response, accepted);
    assert!(db.creation_intents().await.unwrap().is_empty());
    let swaps = db.list_swaps().await.unwrap();
    assert_eq!(swaps.len(), 1);
    assert_eq!(swaps[0].swap_index, original.swap_index);
    assert_eq!(swaps[0].onchain_address, original.claim_address);
}

#[test]
fn reverse_preimage_reveal_requires_confirmations_and_remaining_claim_window() {
    use crate::modules::boltz::claim::validate_spend_window;
    assert!(validate_spend_window(false, 0, 1, 100, 244).is_err());
    assert!(validate_spend_window(false, 1, 2, 100, 244).is_err());
    assert!(validate_spend_window(false, 1, 1, 226, 244).is_ok());
    assert!(validate_spend_window(false, 1, 1, 227, 244).is_err());
    assert!(validate_spend_window(false, 1, 1, u32::MAX, u32::MAX).is_err());
    assert!(validate_spend_window(true, 1, 1, 243, 244).is_err());
    assert!(validate_spend_window(true, 1, 1, 244, 244).is_ok());
}

#[tokio::test]
async fn database_migration_keeps_legacy_routing_and_reserves_fresh_indices() {
    use crate::modules::boltz::models::CREATE_SWAPS_TABLE;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            &CREATE_SWAPS_TABLE.replace(",\n    backend_binding TEXT", ""),
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO swaps VALUES ('legacy','reverse','swap.created','regtest','tcp://127.0.0.1:1',42,NULL,NULL,NULL,1000,900,244,'{}',NULL,NULL,1)", []).unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    drop(connection);
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    let legacy = db.get_swap("legacy").await.unwrap().unwrap();
    assert_eq!(legacy.backend_binding, None);
    assert_eq!(legacy.swap_index, 42);
    assert_eq!(db.reserve_swap_index().await.unwrap(), 43);
    drop(db);
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    assert_eq!(db.reserve_swap_index().await.unwrap(), 44);
}

#[tokio::test]
async fn broadcast_journal_survives_restart_and_keeps_terminal_swaps_recoverable() {
    use crate::modules::boltz::models::CREATE_SWAPS_TABLE;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute(CREATE_SWAPS_TABLE, []).unwrap();
    connection.execute("INSERT INTO swaps VALUES ('journal','reverse','transaction.claimed','regtest','tcp://127.0.0.1:1',0,NULL,NULL,NULL,1000,900,244,'{}',NULL,NULL,1,'fixture-binding')", []).unwrap();
    drop(connection);
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    assert!(db.list_pending_swaps().await.unwrap().is_empty());
    let transaction = bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: OutPoint::null(),
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(900),
            script_pubkey: bitcoin::ScriptBuf::new(),
        }],
    };
    let txid = transaction.compute_txid().to_string();
    let replacement = "22".repeat(32);
    db.journal_spend("journal", &txid, None).await.unwrap();
    db.journal_spend("journal", &replacement, None)
        .await
        .unwrap();
    drop(db);
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    assert_eq!(
        db.pending_spends("journal").await.unwrap(),
        vec![(txid.clone(), None), (replacement, None)]
    );
    assert_eq!(db.list_pending_swaps().await.unwrap().len(), 1);
    let record = db.get_swap("journal").await.unwrap().unwrap();
    let recovered = crate::modules::boltz::claim::recover_broadcast_on_chain(
        &db,
        &record,
        &ObservedAttempt(transaction),
    )
    .await
    .unwrap();
    assert_eq!(recovered.as_deref(), Some(txid.as_str()));
    assert_eq!(
        db.get_swap("journal")
            .await
            .unwrap()
            .unwrap()
            .claim_tx_id
            .as_deref(),
        Some(txid.as_str())
    );
    assert!(db.pending_spends("journal").await.unwrap().is_empty());
    assert!(db.list_pending_swaps().await.unwrap().is_empty());
}

struct ObservedAttempt(bitcoin::Transaction);

#[async_trait::async_trait]
impl pubky_swap_boltz::chain::Chain for ObservedAttempt {
    async fn tip(&self) -> pubky_swap_boltz::Result<u32> {
        Ok(100)
    }
    async fn fee(&self) -> pubky_swap_boltz::Result<u64> {
        Ok(1)
    }
    async fn observe(
        &self,
        _: &pubky_swap_boltz::swap_common::messages::SwapAccept,
    ) -> pubky_swap_boltz::Result<Option<pubky_swap_boltz::chain::Observation>> {
        Ok(None)
    }
    async fn transaction(
        &self,
        id: Txid,
    ) -> pubky_swap_boltz::Result<pubky_swap_boltz::model::TransactionInfo> {
        if self.0.compute_txid() == id {
            Ok(pubky_swap_boltz::chain::info(&self.0))
        } else {
            Err(pubky_swap_boltz::Error::NotFound)
        }
    }
    async fn broadcast(&self, _: bitcoin::Transaction) -> pubky_swap_boltz::Result<Txid> {
        Err(pubky_swap_boltz::Error::Unsupported)
    }
}

#[tokio::test]
async fn version_two_journal_migration_retains_the_original_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old-journal.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute("CREATE TABLE pending_spends (swap_id TEXT PRIMARY KEY, txid TEXT NOT NULL, refund_address TEXT)", []).unwrap();
    connection
        .execute(
            "INSERT INTO pending_spends VALUES ('swap','original','destination')",
            [],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);
    let db = BoltzDB::new(path.to_str().unwrap()).await.unwrap();
    db.journal_spend("swap", "replacement", Some("destination"))
        .await
        .unwrap();
    assert_eq!(
        db.pending_spends("swap").await.unwrap(),
        vec![
            ("original".into(), Some("destination".into())),
            ("replacement".into(), Some("destination".into()))
        ]
    );
}

#[tokio::test]
async fn external_send_preserves_old_intent_when_reviewed_terms_change() {
    let keys = derive_swap_keypair(MNEMONIC, None, BoltzNetwork::Regtest, 0).unwrap();
    let request = CreateRequest::Reverse(ReverseRequest {
        from: "BTC".into(),
        to: "BTC".into(),
        invoice_amount: 100_000,
        onchain_amount: 0,
        preimage_hash: "11".repeat(32),
        claim_public_key: bitcoin::PublicKey::new(keys.public_key()).to_string(),
        pair_hash: "old-terms".into(),
        referral_id: String::new(),
        error: String::new(),
    });
    let mut first = intent(request, Some(destination(&keys).to_string()));
    first.recipient_amount_sat = Some(96_000);
    let mut next = first.clone();
    next.id = uuid::Uuid::new_v4().to_string();
    next.request_key = "new-terms-intent".into();
    next.swap_index = 1;
    if let CreateRequest::Reverse(request) = &mut next.request {
        request.pair_hash = "new-terms".into();
    }
    assert!(!first.same_operation(&next));
    let directory = tempfile::tempdir().unwrap();
    let db = BoltzDB::new(directory.path().join("retry.db").to_str().unwrap())
        .await
        .unwrap();
    db.save_intent(&first).await.unwrap();
    let saved = db.save_intent(&next).await.unwrap();
    assert_eq!(saved.id, next.id);
    assert_eq!(db.creation_intents().await.unwrap().len(), 2);
    assert_eq!(db.save_intent(&next).await.unwrap().id, next.id);
}
