use bitcoin::hashes::{hmac, sha256, Hash, HashEngine, HmacEngine};
use pubky::Keypair;

use super::errors::PubkyError;

/// Reconstruct a [`Keypair`] from a hex-encoded 32-byte secret key.
pub(super) fn keypair_from_hex(secret_key_hex: &str) -> Result<Keypair, PubkyError> {
    let bytes = hex::decode(secret_key_hex)
        .map_err(|e| PubkyError::KeyError { reason: e.to_string() })?;

    let secret: [u8; 32] = bytes.try_into().map_err(|v: Vec<u8>| PubkyError::KeyError {
        reason: format!("expected 32 bytes, got {}", v.len()),
    })?;

    Ok(Keypair::from_secret(&secret))
}

/// Derive a Pubky Ed25519 secret key from a BIP39 seed using HMAC-SHA256.
pub fn derive_pubky_secret_key(seed: Vec<u8>) -> Result<String, PubkyError> {
    if seed.len() != 64 {
        return Err(PubkyError::KeyError {
            reason: format!("expected 64-byte BIP39 seed, got {}", seed.len()),
        });
    }

    let mut engine = hmac::HmacEngine::<sha256::Hash>::new(b"paykit/pubky");
    engine.input(&seed);
    let result = hmac::Hmac::<sha256::Hash>::from_engine(engine);

    Ok(hex::encode(result.to_byte_array()))
}

/// Derive the z32-encoded public key from a hex-encoded secret key.
pub fn pubky_public_key_from_secret(secret_key_hex: String) -> Result<String, PubkyError> {
    let kp = keypair_from_hex(&secret_key_hex)?;
    Ok(kp.public_key().z32())
}
