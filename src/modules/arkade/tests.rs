//! Key derivation, the LNURL session token and the LNURL encoding, checked against values Ark Labs' TypeScript SDK
//! produced for one throwaway mnemonic (`fixtures/oracle.json`, written by `oracle/oracle.mjs`).
//!
//! The `#[ignore]`d tests run against Ark Labs' mainnet operator, delegate and lnurl-server and move no money:
//! `cargo test modules::arkade -- --ignored --test-threads 1`.

use super::keys::Keys;
use super::lnurl::{claim_public_key, encode_lnurl, session_token};
use super::*;
use ark_core::server::parse_sequence_number;
use ark_core::{BoardingOutput, Vtxo};
use bitcoin::key::Secp256k1;
use bitcoin::{Network, PublicKey, XOnlyPublicKey};
use rand::RngCore;
use serde_json::Value;
use serial_test::serial;
use std::str::FromStr;

fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap()
}

fn keys(o: &Value) -> Keys {
    Keys::from_mnemonic(o["mnemonic"].as_str().unwrap(), None, Network::Bitcoin).unwrap()
}

fn xonly(hex: &str) -> XOnlyPublicKey {
    PublicKey::from_str(hex).unwrap().into()
}

#[test]
fn identity_key_matches_seed_identity() {
    let o = oracle();
    let k = keys(&o);
    assert_eq!(k.account_path.to_string(), "86'/0'/1'");
    assert_eq!(
        k.identity.x_only_public_key().0.to_string(),
        o["xOnlyPublicKey"].as_str().unwrap()
    );
}

#[test]
fn claim_public_key_matches() {
    let o = oracle();
    assert_eq!(
        claim_public_key(&keys(&o).identity),
        o["claimPublicKey"].as_str().unwrap()
    );
}

#[test]
fn session_token_is_byte_identical() {
    let o = oracle();
    let token = session_token(&keys(&o).identity, o["lnurlDomain"].as_str().unwrap()).unwrap();
    assert_eq!(token, o["token"].as_str().unwrap());
    // Case and whitespace in the domain don't change the token, as in the TypeScript client.
    assert_eq!(
        session_token(&keys(&o).identity, " LNURL.arkade.sh ").unwrap(),
        token
    );
}

#[test]
fn lnurl_encoding_matches() {
    let o = oracle();
    let handle = o["lightningAddress"]["handle"].as_str().unwrap();
    let lnurl = encode_lnurl(&format!(
        "https://lnurl.arkade.sh/.well-known/lnurlp/{handle}"
    ))
    .unwrap();
    assert_eq!(lnurl, o["lnurlOfHandle"].as_str().unwrap());
    assert_eq!(lnurl, o["lightningAddress"]["lnurl"].as_str().unwrap());
}

/// Rebuilds the bridge's Arkade and boarding addresses offline from the operator and delegate keys it saw. This
/// holds only with the vendored ark-core's owner-first 2-of-2 leaf (see `vendor/ark-core`).
#[test]
fn addresses_match_the_bridge() {
    let o = oracle();
    let owner = keys(&o).identity.x_only_public_key().0;
    let server = xonly(o["server"]["signerPubkey"].as_str().unwrap());
    let delegate = xonly(o["delegatePubkey"].as_str().unwrap());
    let exit = parse_sequence_number(o["server"]["unilateralExitDelay"].as_i64().unwrap()).unwrap();
    let boarding_exit =
        parse_sequence_number(o["server"]["boardingExitDelay"].as_i64().unwrap()).unwrap();
    let secp = Secp256k1::new();

    let vtxo =
        Vtxo::new_with_delegator(&secp, server, owner, delegate, exit, Network::Bitcoin).unwrap();
    assert_eq!(
        vtxo.to_ark_address().encode(),
        o["info"]["arkAddress"].as_str().unwrap()
    );

    let boarding =
        BoardingOutput::new(&secp, server, owner, boarding_exit, Network::Bitcoin).unwrap();
    assert_eq!(
        boarding.address().to_string(),
        o["info"]["boardingAddress"].as_str().unwrap()
    );
}

#[test]
fn state_dump_round_trips() {
    let from = tempfile::tempdir().unwrap();
    let to = tempfile::tempdir().unwrap();
    let dump = export_state(from.path().to_str().unwrap()).unwrap();
    assert_eq!(import_state(to.path().to_str().unwrap(), &dump).unwrap(), 0);
}

fn params(
    dir: &tempfile::TempDir,
    mnemonic: &str,
    restore: bool,
    backup: Option<String>,
) -> ArkadeStartParams {
    ArkadeStartParams {
        data_dir: dir.path().to_string_lossy().into_owned(),
        mnemonic: mnemonic.into(),
        passphrase: None,
        network: ArkadeNetwork::Bitcoin,
        restore,
        backup,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
#[ignore]
async fn fresh_wallet_finds_its_address_after_restore() {
    let mut entropy = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy).unwrap().to_string();

    let first = tempfile::tempdir().unwrap();
    let info = start(params(&first, &mnemonic, false, None)).await.unwrap();
    assert!(info.ark_address.starts_with("ark1"));
    assert!(info.boarding_address.starts_with("bc1p"));
    assert!(info.delegate);
    assert_eq!(super::info().await.unwrap().ark_address, info.ark_address);
    assert_eq!(balance().await.unwrap().total, 0);
    assert!(vtxos().await.unwrap().is_empty());
    assert!(history().await.unwrap().is_empty());
    let address = lightning_address().await.unwrap();
    assert!(address.lnurl.starts_with("LNURL1"));
    let backup = export_state(first.path().to_str().unwrap()).unwrap();
    stop().await;

    let second = tempfile::tempdir().unwrap();
    let restored = start(params(&second, &mnemonic, true, Some(backup)))
        .await
        .unwrap();
    assert_eq!(restored.ark_address, info.ark_address);
    assert_eq!(restored.boarding_address, info.boarding_address);
    assert_eq!(lightning_address().await.unwrap(), address);
    wipe(second.path().to_str().unwrap()).await.unwrap();
}

/// The fixture wallet the TypeScript bridge started: the module gets the same addresses live and finds the
/// Lightning address the bridge claimed.
#[tokio::test(flavor = "multi_thread")]
#[serial]
#[ignore]
async fn fixture_wallet_matches_the_bridge() {
    let o = oracle();
    let dir = tempfile::tempdir().unwrap();
    let info = start(params(&dir, o["mnemonic"].as_str().unwrap(), true, None))
        .await
        .unwrap();
    assert_eq!(info.ark_address, o["info"]["arkAddress"].as_str().unwrap());
    assert_eq!(
        info.boarding_address,
        o["info"]["boardingAddress"].as_str().unwrap()
    );
    assert_eq!(
        info.unilateral_exit_delay,
        o["info"]["unilateralExitDelay"].as_u64().unwrap()
    );
    assert_eq!(
        info.boarding_exit_delay,
        o["info"]["boardingExitDelay"].as_u64().unwrap()
    );
    assert_eq!(info.dust, o["info"]["dust"].as_u64().unwrap());
    let address = lightning_address().await.unwrap();
    assert_eq!(
        address.handle,
        o["lightningAddress"]["handle"].as_str().unwrap()
    );
    assert_eq!(
        address.lightning_address.as_deref(),
        o["lightningAddress"]["lightningAddress"].as_str()
    );
    assert_eq!(
        address.lnurl,
        o["lightningAddress"]["lnurl"].as_str().unwrap()
    );
    wipe(dir.path().to_str().unwrap()).await.unwrap();
}
