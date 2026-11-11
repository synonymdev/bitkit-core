use pubky::Keypair;

use super::errors::PubkyError;

/// Reconstruct a [`Keypair`] from a hex-encoded 32-byte secret key.
pub(super) fn keypair_from_hex(secret_key_hex: &str) -> Result<Keypair, PubkyError> {
    let bytes = hex::decode(secret_key_hex).map_err(|e| PubkyError::KeyError {
        reason: e.to_string(),
    })?;

    let secret: [u8; 32] = bytes
        .try_into()
        .map_err(|v: Vec<u8>| PubkyError::KeyError {
            reason: format!("expected 32 bytes, got {}", v.len()),
        })?;

    Ok(Keypair::from_secret(&secret))
}
