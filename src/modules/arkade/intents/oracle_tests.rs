//! Oracle tests: for the throwaway wallet and the real `beta-solver` quotes in
//! `fixtures/oracle.json` (captured by `oracle/capture.mjs` from `@arkade-os/swap`
//! 0.1.0-rc.34), the Rust claim key, preimage, payment hash and lockup addresses equal what the TS
//! package computed, and equal the solver's own `lockup_address`.

use super::bolt11;
use super::contract::*;
use super::preimage::*;
use super::wire::*;
use ark_core::ArkAddress;
use bitcoin::bip32::DerivationPath;
use bitcoin::bip32::Xpriv;
use bitcoin::key::Secp256k1;
use bitcoin::Network;
use serde_json::Value;
use std::str::FromStr;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap()
}

/// `m/86'/0'/1'/0/0`: the TS identity key under Bitkit's Arkade account.
fn identity(mnemonic: &str) -> bitcoin::secp256k1::Keypair {
    let seed = bip39::Mnemonic::parse(mnemonic).unwrap().to_seed("");
    let secp = Secp256k1::new();
    let xpriv = Xpriv::new_master(Network::Bitcoin, &seed)
        .unwrap()
        .derive_priv(&secp, &DerivationPath::from_str("m/86'/0'/1'/0/0").unwrap())
        .unwrap();
    xpriv.to_keypair(&secp)
}

fn terms(f: &Value) -> Terms {
    Terms {
        operator: f["terms"]["operatorPubkey"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        emulator: emulator_pubkey(Network::Bitcoin).unwrap(),
        claim_delay: unilateral_claim_delay(
            f["server"]["unilateralExitDelay"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
        )
        .unwrap(),
        network: Network::Bitcoin,
    }
}

#[test]
fn terms_match_ts() {
    let f = fixture();
    let t = terms(&f);
    assert_eq!(
        t.claim_delay as u64,
        f["terms"]["claimDelay"].as_u64().unwrap()
    );
    assert_eq!(
        t.emulator.to_string(),
        f["terms"]["emulatorPubkey"].as_str().unwrap()
    );
    // ark-rs parses the operator's 605184 s exit delay into a seconds Sequence; Terms reads it back.
    let seq = ark_core::server::parse_sequence_number(605184).unwrap();
    assert_eq!((seq.to_consensus_u32() & 0xffff) * 512, 605184);
}

#[test]
fn receive_secrets_match_ts() {
    let f = fixture();
    let r = &f["receive"];
    let kp = identity(f["mnemonic"].as_str().unwrap());
    assert_eq!(
        kp.x_only_public_key().0.to_string(),
        r["claimPubkey"].as_str().unwrap(),
        "claim key"
    );
    assert_eq!(r["perArtifactDescriptor"], Value::Bool(false));
    let salt = hex_32(r["preimageSalt"].as_str().unwrap(), "salt").unwrap();
    let xonly = kp.x_only_public_key().0.serialize();
    assert_eq!(
        hex::encode(salted_preimage_message(&xonly, &salt)),
        r["preimageMessage"].as_str().unwrap()
    );
    let preimage = salted_preimage(&kp, &salt);
    assert_eq!(
        hex::encode(preimage),
        r["preimage"].as_str().unwrap(),
        "preimage"
    );
    assert_eq!(
        hex::encode(payment_hash(&preimage)),
        r["paymentHash"].as_str().unwrap(),
        "payment hash"
    );
}

#[test]
fn receive_lockup_matches_ts_and_solver() {
    let f = fixture();
    let r = &f["receive"];
    let quote: RfqQuote = serde_json::from_value(r["quote"].clone()).unwrap();
    let kp = identity(f["mnemonic"].as_str().unwrap());
    let payout = ArkAddress::decode(r["payoutAddress"].as_str().unwrap()).unwrap();
    let lockup = derive_receive(
        &quote,
        r["paymentHash"].as_str().unwrap(),
        kp.x_only_public_key().0,
        &payout,
        &terms(&f),
    )
    .unwrap();
    assert_eq!(
        lockup.address.encode(),
        r["lockupAddress"].as_str().unwrap(),
        "TS address"
    );
    assert_eq!(
        lockup.address.encode(),
        quote.profile["lockup_address"].as_str().unwrap(),
        "solver address"
    );
    assert!(!lockup.params.legacy_pre_timelocked_refund);
    assert_eq!(lockup.script.tapscripts().len(), 9);
    // Rebuilding from the persisted params gives the same lockup.
    assert_eq!(
        lockup.params.build().unwrap().address(Network::Bitcoin),
        lockup.address
    );

    let invoice = bolt11::decode(&lockup.invoice).unwrap();
    let now = f["capturedAt"].as_u64().unwrap();
    let deadline = verify_receive(
        &quote,
        1000,
        r["paymentHash"].as_str().unwrap(),
        &invoice,
        now,
    )
    .unwrap();
    assert_eq!(deadline, invoice.expires_at.min(quote.valid_until));
    // A different hash, or a different amount, is refused.
    assert!(verify_receive(&quote, 1000, &"00".repeat(32), &invoice, now).is_err());
    assert!(verify_receive(
        &quote,
        999,
        r["paymentHash"].as_str().unwrap(),
        &invoice,
        now
    )
    .is_err());
    assert!(verify_receive(
        &quote,
        1000,
        r["paymentHash"].as_str().unwrap(),
        &invoice,
        deadline
    )
    .is_err());
}

#[test]
fn send_lockup_matches_ts_and_solver() {
    let f = fixture();
    let s = &f["send"];
    let quote: RfqQuote = serde_json::from_value(s["quote"].clone()).unwrap();
    let invoice = bolt11::decode(s["request"]["profile"]["invoice"].as_str().unwrap()).unwrap();
    let kp = identity(f["mnemonic"].as_str().unwrap());
    assert_eq!(
        kp.x_only_public_key().0.to_string(),
        s["refund"]["pubkey"].as_str().unwrap()
    );
    let refund = ArkAddress::decode(s["refund"]["address"].as_str().unwrap()).unwrap();
    let now = f["capturedAt"].as_u64().unwrap();
    let lockup = derive_send(
        &quote,
        &invoice.payment_hash,
        kp.x_only_public_key().0,
        &refund,
        &terms(&f),
        now,
    )
    .unwrap();
    assert_eq!(
        lockup.address.encode(),
        s["lockupAddress"].as_str().unwrap(),
        "TS address"
    );
    assert_eq!(
        lockup.address.encode(),
        quote.profile["lockup_address"].as_str().unwrap(),
        "solver address"
    );
    // The legacy eight-leaf shape is a different address, which TS computes the same.
    let mut legacy = lockup.params.clone();
    legacy.legacy_pre_timelocked_refund = true;
    assert_eq!(
        legacy.build().unwrap().address(Network::Bitcoin).encode(),
        s["lockupAddressLegacy"].as_str().unwrap()
    );
    verify_send(&quote, &invoice, now).unwrap();
    assert_eq!(quote.give().unwrap() - quote.take().unwrap(), 4);
}
