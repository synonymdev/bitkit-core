//! Swap keys derived from the wallet seed, so a swap can be rebuilt from the seed and its index.
//!
//! The provider sees only public keys and the payment hash. Each key is
//! HMAC-SHA512(seed, domain || index), truncated to 32 bytes, under its own domain string, so
//! nothing here can collide with the Boltz module's BIP85 keys or with each other.

use bitcoin::hashes::{hmac, sha256, sha512, Hash, HashEngine};
use bitcoin::secp256k1::{Secp256k1, SecretKey};

use super::errors::PubkySwapError;

const IDENTITY_DOMAIN: &[u8] = b"bitkit/pubky-swaps/identity/v1";
const CLAIM_DOMAIN: &[u8] = b"bitkit/pubky-swaps/claim/v1/";
const PREIMAGE_DOMAIN: &[u8] = b"bitkit/pubky-swaps/preimage/v1/";

pub(crate) struct SwapKeys {
    pub claim_key: SecretKey,
    pub claim_pubkey: bitcoin::PublicKey,
    pub preimage: [u8; 32],
    pub payment_hash: [u8; 32],
}

fn seed(mnemonic: &str, passphrase: Option<&str>) -> Result<[u8; 64], PubkySwapError> {
    let mnemonic = bip39::Mnemonic::parse(mnemonic)
        .map_err(|e| PubkySwapError::invalid(format!("mnemonic: {e}")))?;
    Ok(mnemonic.to_seed(passphrase.unwrap_or("")))
}

fn derive(seed: &[u8; 64], domain: &[u8], index: Option<u64>) -> [u8; 32] {
    let mut engine = hmac::HmacEngine::<sha512::Hash>::new(seed);
    engine.input(domain);
    if let Some(index) = index {
        engine.input(&index.to_be_bytes());
    }
    let out = hmac::Hmac::<sha512::Hash>::from_engine(engine).to_byte_array();
    let mut key = [0u8; 32];
    key.copy_from_slice(&out[..32]);
    key
}

/// The Ed25519 secret the wallet uses as its identity towards swap providers. It is not the
/// user's Pubky identity, so providers cannot link swaps to the user's profile.
pub(crate) fn identity_secret(
    mnemonic: &str,
    passphrase: Option<&str>,
) -> Result<[u8; 32], PubkySwapError> {
    Ok(derive(&seed(mnemonic, passphrase)?, IDENTITY_DOMAIN, None))
}

pub(crate) fn swap_keys(
    mnemonic: &str,
    passphrase: Option<&str>,
    index: u64,
) -> Result<SwapKeys, PubkySwapError> {
    let seed = seed(mnemonic, passphrase)?;
    let claim_key = SecretKey::from_slice(&derive(&seed, CLAIM_DOMAIN, Some(index)))
        .map_err(|e| PubkySwapError::invalid(format!("claim key: {e}")))?;
    let claim_pubkey = bitcoin::PublicKey::new(claim_key.public_key(&Secp256k1::new()));
    let preimage = derive(&seed, PREIMAGE_DOMAIN, Some(index));
    let payment_hash = sha256::Hash::hash(&preimage).to_byte_array();
    Ok(SwapKeys {
        claim_key,
        claim_pubkey,
        preimage,
        payment_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MNEMONIC: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn keys_are_deterministic_and_distinct_per_index() {
        let a = swap_keys(MNEMONIC, None, 0).unwrap();
        let again = swap_keys(MNEMONIC, None, 0).unwrap();
        let b = swap_keys(MNEMONIC, None, 1).unwrap();
        assert_eq!(a.claim_key, again.claim_key);
        assert_eq!(a.preimage, again.preimage);
        assert_ne!(a.claim_key, b.claim_key);
        assert_ne!(a.preimage, b.preimage);
        assert_ne!(a.preimage[..], a.claim_key.secret_bytes()[..]);
    }

    #[test]
    fn passphrase_changes_every_key() {
        let plain = swap_keys(MNEMONIC, None, 0).unwrap();
        let with = swap_keys(MNEMONIC, Some("x"), 0).unwrap();
        assert_ne!(plain.claim_key, with.claim_key);
        assert_ne!(
            identity_secret(MNEMONIC, None).unwrap(),
            identity_secret(MNEMONIC, Some("x")).unwrap()
        );
    }
}
