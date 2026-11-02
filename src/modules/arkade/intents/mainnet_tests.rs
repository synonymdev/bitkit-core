//! Real RFQs against `beta-solver` on mainnet, with the oracle fixture's throwaway wallet. Moves
//! no money: it asks for a receive quote and a send quote (for the receive's own hold invoice)
//! and funds, pays and claims nothing.
//!
//!     cargo test --lib modules::arkade::intents::mainnet_tests -- --ignored --nocapture

use super::bolt11;
use super::contract::LockupParams;
use super::preimage::payment_hash;
use super::preimage::salted_preimage;
use super::*;
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
use bitcoin::Psbt;
use bitcoin::ScriptBuf;
use bitcoin::Txid;
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;

const ARK_SERVER: &str = "https://arkade.computer";

/// Quotes only: server info and keys, no spending.
struct QuoteOnlyWallet {
    keypair: Keypair,
    address: ArkAddress,
}

fn refuse<T>() -> Result<T, WalletError> {
    Err(WalletError("quote-only test wallet moves no money".into()))
}

#[async_trait]
impl ArkWallet for QuoteOnlyWallet {
    async fn server_info(&self) -> Result<Info, WalletError> {
        let v: Value = reqwest::get(format!("{ARK_SERVER}/v1/info"))
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| WalletError(e.to_string()))?
            .json()
            .await
            .map_err(|e| WalletError(e.to_string()))?;
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let n = |k: &str| s(k).parse::<i64>().unwrap_or_default();
        assert_eq!(s("network"), "bitcoin");
        Ok(Info {
            version: s("version"),
            signer_pk: s("signerPubkey")
                .parse()
                .map_err(|e| WalletError(format!("{e}")))?,
            forfeit_pk: s("forfeitPubkey")
                .parse()
                .map_err(|e| WalletError(format!("{e}")))?,
            forfeit_address: bitcoin::Address::from_str(&s("forfeitAddress"))
                .map_err(|e| WalletError(e.to_string()))?
                .assume_checked(),
            checkpoint_tapscript: ScriptBuf::from_hex(&s("checkpointTapscript"))
                .unwrap_or_default(),
            network: Network::Bitcoin,
            session_duration: n("sessionDuration") as u64,
            unilateral_exit_delay: ark_core::server::parse_sequence_number(n(
                "unilateralExitDelay",
            ))
            .map_err(|e| WalletError(e.to_string()))?,
            boarding_exit_delay: ark_core::server::parse_sequence_number(n("boardingExitDelay"))
                .map_err(|e| WalletError(e.to_string()))?,
            utxo_min_amount: None,
            utxo_max_amount: None,
            vtxo_min_amount: None,
            vtxo_max_amount: None,
            dust: Amount::from_sat(n("dust") as u64),
            fees: None,
            scheduled_session: None,
            deprecated_signers: vec![],
            service_status: Default::default(),
            digest: s("digest"),
            max_tx_weight: 0,
            max_op_return_outputs: 0,
        })
    }
    async fn address(&self) -> Result<ArkAddress, WalletError> {
        Ok(self.address)
    }
    fn identity_keypair(&self) -> Result<Keypair, WalletError> {
        Ok(self.keypair)
    }
    async fn vtxos(&self, _: &ArkAddress) -> Result<Vec<VirtualTxOutPoint>, WalletError> {
        refuse()
    }
    async fn virtual_tx(&self, _: Txid) -> Result<Option<Psbt>, WalletError> {
        refuse()
    }
    async fn submit_offchain(&self, _: Psbt, _: Vec<Psbt>) -> Result<Vec<Psbt>, WalletError> {
        refuse()
    }
    async fn finalize_offchain(&self, _: Txid, _: Vec<Psbt>) -> Result<(), WalletError> {
        refuse()
    }
    async fn send(&self, _: &ArkAddress, _: Amount) -> Result<Txid, WalletError> {
        refuse()
    }
}

#[tokio::test]
#[ignore = "talks to beta-solver on mainnet"]
async fn mainnet_receive_and_send_quotes() {
    super::super::install_crypto_provider();
    let fixture: Value = serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap();
    let seed = bip39::Mnemonic::parse(fixture["mnemonic"].as_str().unwrap())
        .unwrap()
        .to_seed("");
    let secp = Secp256k1::new();
    let keypair = Xpriv::new_master(Network::Bitcoin, &seed)
        .unwrap()
        .derive_priv(&secp, &DerivationPath::from_str("m/86'/0'/1'/0/0").unwrap())
        .unwrap()
        .to_keypair(&secp);
    // The TS wallet's own address for this key (ark-core 0.12.0 orders the default leaf
    // differently; the binding patches that).
    let address =
        ArkAddress::decode(fixture["receive"]["payoutAddress"].as_str().unwrap()).unwrap();
    let wallet = Arc::new(QuoteOnlyWallet { keypair, address });
    let client = IntentsClient::new(
        wallet,
        Arc::new(JsonStore::memory()),
        SolverCard::beta_solver(),
    );

    let amount = 1000;
    let record = client.receive(amount).await.expect("receive quote");
    let invoice = bolt11::decode(&record.bolt11).unwrap();
    let salt: [u8; 32] = hex::decode(record.preimage_salt.as_deref().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let ours = hex::encode(payment_hash(&salted_preimage(&keypair, &salt)));
    let quote = &record.quote;
    println!(
        "receive: give {} take {} fee {} sat",
        record.give_sat, record.take_sat, record.fee_sat
    );
    println!(
        "  invoice amount {} sat, expires in {}s",
        invoice.amount_sats,
        invoice.expires_at as i64 - record.created_at as i64
    );
    println!(
        "  invoice hash {}\n  our hash     {}",
        invoice.payment_hash, ours
    );
    println!(
        "  quote valid {}s, refund_locktime in {}s",
        quote["valid_until"].as_u64().unwrap() - record.created_at,
        quote["refund_locktime"].as_u64().unwrap() - record.created_at
    );
    println!(
        "  lockup {} (solver quoted {})",
        record.lockup_address, quote["profile"]["lockup_address"]
    );
    assert_eq!(invoice.amount_sats, amount);
    assert_eq!(invoice.payment_hash, ours);
    assert_eq!(record.payment_hash, ours);
    assert_eq!(
        record.lockup_address,
        quote["profile"]["lockup_address"].as_str().unwrap()
    );
    assert_eq!(record.outcome, Outcome::Accepted);
    let rebuilt: &LockupParams = &record.lockup;
    assert_eq!(
        rebuilt.build().unwrap().address(Network::Bitcoin).encode(),
        record.lockup_address
    );

    let pay = client.quote_pay(&record.bolt11).await.expect("send quote");
    println!(
        "send quote for that invoice: give {} take {} fee {} sat, valid {}s",
        pay.give_sat,
        pay.take_sat,
        pay.fee_sat,
        pay.expires_at as i64 - record.created_at as i64
    );
    assert_eq!(pay.take_sat, amount);
    assert_eq!(pay.fee_sat, pay.give_sat - pay.take_sat);
}
