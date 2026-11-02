use std::str::FromStr;

use base64::{engine::general_purpose::STANDARD, Engine};
use bitcoin::{
    absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, Transaction,
    TxIn, TxOut, Txid, Witness,
};
use prost::Message;
use trezor_connect_rs::protos::{bitcoin as proto, bitcoin::tx_request, MessageType};

use super::implementation::psbt_sign_tx_params;
use super::test_transport::{public_key_reply, Exchange, TestTransport, PUBLIC_KEY, XPUB};
use super::*;

fn signing_params(
    coin: Option<TrezorCoinType>,
    address: Option<String>,
    change: Option<String>,
) -> TrezorSignTxParams {
    let change_script_type = change.as_ref().map(|_| TrezorScriptType::SpendWitness);
    TrezorSignTxParams {
        inputs: vec![TrezorTxInput {
            prev_hash: "aa".repeat(32),
            prev_index: 0,
            path: "m/86'/1'/0'/0/0".into(),
            amount: 100_000,
            script_type: TrezorScriptType::SpendTaproot,
            sequence: None,
            orig_hash: None,
            orig_index: None,
        }],
        outputs: vec![TrezorTxOutput {
            address,
            path: change,
            amount: 90_000,
            script_type: change_script_type,
            op_return_data: None,
            orig_hash: None,
            orig_index: None,
        }],
        coin,
        lock_time: Some(0),
        version: Some(2),
        prev_txs: vec![],
    }
}

fn unsigned_transaction(script_pubkey: ScriptBuf) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(Txid::from_str(&"aa".repeat(32)).unwrap(), 0),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(90_000),
            script_pubkey,
        }],
    }
}

fn output_script() -> ScriptBuf {
    let key = bitcoin::CompressedPublicKey::from_str(PUBLIC_KEY).unwrap();
    bitcoin::Address::p2wpkh(&key, bitcoin::Network::Bitcoin).script_pubkey()
}

fn signed_transaction_script() -> Vec<Exchange> {
    let mut transaction = unsigned_transaction(output_script());
    transaction.input[0].witness = Witness::from_slice(&[vec![0xaa; 64]]);
    let mut exchanges = Vec::new();
    for (request_type, request, index) in [
        (
            tx_request::RequestType::Txinput,
            MessageType::SignTx,
            Some(0),
        ),
        (
            tx_request::RequestType::Txoutput,
            MessageType::TxAck,
            Some(0),
        ),
        (
            tx_request::RequestType::Txfinished,
            MessageType::TxAck,
            None,
        ),
    ] {
        exchanges.push(Exchange::new(
            request,
            MessageType::TxRequest,
            proto::TxRequest {
                request_type: Some(request_type as i32),
                details: index.map(|request_index| tx_request::TxRequestDetailsType {
                    request_index: Some(request_index),
                    tx_hash: None,
                    extra_data_len: None,
                    extra_data_offset: None,
                }),
                serialized: index
                    .is_none()
                    .then(|| tx_request::TxRequestSerializedType {
                        signature_index: Some(0),
                        signature: Some(vec![0xaa, 0xbb]),
                        serialized_tx: Some(bitcoin::consensus::serialize(&transaction)),
                    }),
            },
        ));
    }
    exchanges
}

#[tokio::test]
async fn direct_signing_infers_testnet_and_preserves_transaction_data() {
    let address =
        bitcoin::Address::from_script(&output_script(), bitcoin::Network::Testnet).unwrap();
    let params = signing_params(None, Some(address.to_string()), None);
    let (manager, transport) = TestTransport::manager(signed_transaction_script());
    let response: TrezorSignedTx = manager.sign_tx(params).await.unwrap();
    let request = proto::SignTx::decode(transport.calls()[0].1.as_slice()).unwrap();
    assert_eq!(request.coin_name.as_deref(), Some("Testnet"));
    assert_eq!(response.signatures, vec!["aabb"]);
    let transaction: Transaction =
        bitcoin::consensus::deserialize(&hex::decode(&response.serialized_tx).unwrap()).unwrap();
    assert_eq!(response.txid, Some(transaction.compute_txid().to_string()));
    assert_eq!(transaction.output[0].script_pubkey, output_script());
    transport.assert_finished();
}

#[tokio::test]
async fn inferred_testnet_rejects_mainnet_outputs_before_signing() {
    let address =
        bitcoin::Address::from_script(&output_script(), bitcoin::Network::Bitcoin).unwrap();
    let (manager, transport) = TestTransport::manager(vec![]);
    let error = manager
        .sign_tx(signing_params(None, Some(address.to_string()), None))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Network mismatch"));
    assert!(transport.calls().is_empty());
}

#[tokio::test]
async fn change_key_and_signing_requests_use_the_same_network() {
    for (coin, expected) in [
        (None, "Testnet"),
        (Some(TrezorCoinType::Regtest), "Regtest"),
        (Some(TrezorCoinType::Signet), "Testnet"),
    ] {
        let params = signing_params(coin, None, Some("m/84'/1'/0'/1/0".into()));
        let mut script = vec![Exchange::new(
            MessageType::GetPublicKey,
            MessageType::PublicKey,
            public_key_reply(XPUB.into(), None),
        )];
        script.extend(signed_transaction_script());
        let (manager, transport) = TestTransport::manager(script);
        manager.sign_tx(params).await.unwrap();
        let calls = transport.calls();
        let key = proto::GetPublicKey::decode(calls[0].1.as_slice()).unwrap();
        let sign = proto::SignTx::decode(calls[1].1.as_slice()).unwrap();
        assert_eq!(key.coin_name.as_deref(), Some(expected));
        assert_eq!(sign.coin_name.as_deref(), Some(expected));
        transport.assert_finished();
    }
}

fn psbt_base64(change: bool) -> String {
    let mut psbt =
        bitcoin::psbt::Psbt::from_unsigned_tx(unsigned_transaction(output_script())).unwrap();
    let key = bitcoin::secp256k1::PublicKey::from_str(PUBLIC_KEY).unwrap();
    let origin = (
        bitcoin::bip32::Fingerprint::from([0; 4]),
        bitcoin::bip32::DerivationPath::from_str("m/86'/1'/0'/0/0").unwrap(),
    );
    psbt.inputs[0]
        .tap_key_origins
        .insert(key.x_only_public_key().0, (vec![], origin));
    psbt.inputs[0].witness_utxo = Some(TxOut {
        value: Amount::from_sat(100_000),
        script_pubkey: output_script(),
    });
    if change {
        let origin = (
            bitcoin::bip32::Fingerprint::from([0; 4]),
            bitcoin::bip32::DerivationPath::from_str("m/84'/1'/0'/1/0").unwrap(),
        );
        psbt.outputs[0].bip32_derivation.insert(key, origin);
    }
    STANDARD.encode(psbt.serialize())
}

#[tokio::test]
async fn psbt_signing_preserves_explicit_network_selection_and_bitcoin_default() {
    for (coin, expected) in [
        (None, "Bitcoin"),
        (Some(TrezorCoinType::Testnet), "Testnet"),
        (Some(TrezorCoinType::Signet), "Testnet"),
        (Some(TrezorCoinType::Regtest), "Regtest"),
    ] {
        for change in [false, true] {
            let params = psbt_sign_tx_params(&psbt_base64(change), coin).unwrap();
            assert_eq!(params.coin.unwrap().coin_name(), expected);
            let mut script = Vec::new();
            if change {
                script.push(Exchange::new(
                    MessageType::GetPublicKey,
                    MessageType::PublicKey,
                    public_key_reply(XPUB.into(), None),
                ));
            }
            script.extend(signed_transaction_script());
            let (manager, transport) = TestTransport::manager(script);
            manager
                .sign_tx_from_psbt(psbt_base64(change), coin)
                .await
                .unwrap();
            let calls = transport.calls();
            if change {
                assert_eq!(
                    proto::GetPublicKey::decode(calls[0].1.as_slice())
                        .unwrap()
                        .coin_name
                        .as_deref(),
                    Some(expected)
                );
            }
            assert_eq!(
                proto::SignTx::decode(calls[usize::from(change)].1.as_slice())
                    .unwrap()
                    .coin_name
                    .as_deref(),
                Some(expected)
            );
            transport.assert_finished();
        }
    }
}
