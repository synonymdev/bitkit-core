//! The receive preimage, derived from the wallet key exactly as `@arkade-os/sdk` 0.5.0-rc.31's
//! `provisionClaimSecret` does, so a restored wallet re-derives every swap's `P`.
//!
//! Bitkit's TS wallet runs in the default `walletMode: "auto"` (static), whose descriptor is the
//! bare identity key `tr(<xonly>)`. That takes the salted arm: a fresh public 32-byte salt per
//! swap, persisted in the clear with the record, and
//!
//! ```text
//! P = sha256(bip340_sign(sha256(TAG || xonly || salt), key, aux_rand = 0^32))
//! ```
//!
//! with `TAG = "Arkade-Contract-Preimage-Salted-v1"`. The HD arm (`"Arkade-RFQ-Preimage-v1"`,
//! `xonly || u32le(0)` over a fresh HD child) applies only to `walletMode: "hd"` and is not used.

use bitcoin::hashes::sha256;
use bitcoin::hashes::Hash;
use bitcoin::key::Secp256k1;
use bitcoin::secp256k1::Keypair;
use bitcoin::secp256k1::Message;

pub const ARKADE_SWAP_PREIMAGE_TAG: &str = "Arkade-RFQ-Preimage-v1";
pub const ARKADE_SALTED_PREIMAGE_TAG: &str = "Arkade-Contract-Preimage-Salted-v1";

/// `TAG || xonly(32) || salt(32)`, the message whose hash gets signed.
pub fn salted_preimage_message(xonly: &[u8; 32], salt: &[u8; 32]) -> Vec<u8> {
    let mut message = ARKADE_SALTED_PREIMAGE_TAG.as_bytes().to_vec();
    message.extend_from_slice(xonly);
    message.extend_from_slice(salt);
    message
}

/// `sha256(sign_det(sha256(message)))`: BIP-340 with all-zero aux randomness, so reproducible.
pub fn preimage_from_message(keypair: &Keypair, message: &[u8]) -> [u8; 32] {
    let digest = sha256::Hash::hash(message).to_byte_array();
    let signature = Secp256k1::new().sign_schnorr_with_aux_rand(
        &Message::from_digest(digest),
        keypair,
        &[0u8; 32],
    );
    sha256::Hash::hash(signature.as_ref()).to_byte_array()
}

/// The swap's `P` from the claim key and its salt.
pub fn salted_preimage(keypair: &Keypair, salt: &[u8; 32]) -> [u8; 32] {
    let xonly = keypair.x_only_public_key().0.serialize();
    preimage_from_message(keypair, &salted_preimage_message(&xonly, salt))
}

pub fn payment_hash(preimage: &[u8; 32]) -> [u8; 32] {
    sha256::Hash::hash(preimage).to_byte_array()
}
