//! Envelope decryption, ported from wake-proto `crypto`.
//!
//! The gateway encrypts every attempt with a fresh ephemeral key and IV:
//! - `S33 = compressed(d_eph * P_device)`, i.e. `(0x02 | y&1) || x`;
//! - `key = SHA256(SHA256(S33 || label))`;
//! - AES-256-GCM with no AAD and a 16-byte tag.
//!
//! v0 (the legacy bitkit-notification-server envelope) uses the label
//! `bitkit-notifications` and a 16-byte IV; v1 uses `wake-v1` and a 12-byte IV.

use aes_gcm::aead::consts::U16;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::aes::Aes256;
use aes_gcm::{Aes256Gcm, AesGcm};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{ecdh, PublicKey, Secp256k1, SecretKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::errors::{json_error, WakeError};
use super::types::{WakeEnvelope, WakeKeyPair};

const TAG_LEN: usize = 16;

/// Draws after which key generation reports the random source as degenerate.
/// A working generator's draw is out of range with probability about 2^-128.
const MAX_REJECTED_DRAWS: usize = 64;

/// AES-256-GCM with a 16-byte nonce, as the legacy server used.
type Aes256Gcm16 = AesGcm<Aes256, U16>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    V0,
    V1,
}

impl Scheme {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Scheme::V0 => "bitkit-notifications",
            Scheme::V1 => "wake-v1",
        }
    }
}

/// An encrypted envelope: `cipher` is padded standard base64 of the
/// ciphertext, `iv`, `tag` and the ephemeral `publicKey` are hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Container {
    scheme: Scheme,
    cipher: Vec<u8>,
    iv: Vec<u8>,
    tag: [u8; TAG_LEN],
    public_key: [u8; 33],
}

impl Container {
    /// Builds a container from its wire strings. Hex may be upper or lower
    /// case; the IV must be 12 or 16 bytes whatever the scheme, because that
    /// is what the apps accept.
    pub(crate) fn from_fields(
        scheme: Scheme,
        cipher: &str,
        iv: &str,
        tag: &str,
        public_key: &str,
    ) -> Result<Self, WakeError> {
        let cipher = STANDARD
            .decode(cipher)
            .map_err(|_| WakeError::invalid_input("cipher is not padded standard base64"))?;
        let iv = hex::decode(iv).map_err(|_| WakeError::invalid_input("iv is not hex"))?;
        let tag = hex::decode(tag)
            .ok()
            .and_then(|t| <[u8; TAG_LEN]>::try_from(t).ok())
            .ok_or_else(|| WakeError::invalid_input("tag must be 16 hex bytes"))?;
        let public_key = hex::decode(public_key)
            .ok()
            .and_then(|k| <[u8; 33]>::try_from(k).ok())
            .ok_or_else(|| WakeError::invalid_input("publicKey must be 33 hex bytes"))?;
        if iv.len() != 12 && iv.len() != 16 {
            return Err(WakeError::invalid_input("iv must be 12 or 16 bytes"));
        }
        Ok(Container {
            scheme,
            cipher,
            iv,
            tag,
            public_key,
        })
    }

    /// Parses container JSON. A missing `v` means v0, `"v":1` means v1, and
    /// any other `v` (`null` included) is unsupported. Unknown keys are
    /// ignored, as the apps ignore them.
    pub(crate) fn from_json(json: &str) -> Result<Self, WakeError> {
        let wire: ContainerWire = serde_json::from_str(json)
            .map_err(|_| WakeError::invalid_input("malformed container json"))?;
        let scheme = match wire.v {
            None => Scheme::V0,
            Some(v) if v.as_u64() == Some(1) => Scheme::V1,
            Some(_) => return Err(WakeError::unsupported("unsupported envelope version")),
        };
        Container::from_fields(scheme, &wire.cipher, &wire.iv, &wire.tag, &wire.public_key)
    }

    pub(crate) fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// Decrypts with the device's secret key and returns the plaintext bytes.
    pub(crate) fn decrypt(&self, device_secret: &SecretKey) -> Result<Vec<u8>, WakeError> {
        let point = shared_point(device_secret, &self.public_key)?;
        let key = derive_key(&point, self.scheme.label());
        open(&key, &self.iv, &self.cipher, &self.tag)
    }

    /// Decrypts and parses the plaintext of this container's scheme.
    pub(crate) fn open_envelope(
        &self,
        device_secret: &SecretKey,
    ) -> Result<WakeEnvelope, WakeError> {
        let plaintext = self.decrypt(device_secret)?;
        match self.scheme {
            Scheme::V0 => legacy_envelope(&plaintext),
            Scheme::V1 => v1_envelope(&plaintext),
        }
    }
}

#[derive(Deserialize)]
struct ContainerWire {
    /// `None` only when the key is absent; `"v":null` is `Some(Null)`.
    #[serde(default, deserialize_with = "present")]
    v: Option<serde_json::Value>,
    cipher: String,
    iv: String,
    tag: String,
    #[serde(rename = "publicKey")]
    public_key: String,
}

fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(deserializer).map(Some)
}

/// `S33 = compressed(secret * public)`, the shared point both sides hash
/// into the AES key.
pub(crate) fn shared_point(secret: &SecretKey, public: &[u8; 33]) -> Result<[u8; 33], WakeError> {
    if public[0] != 0x02 && public[0] != 0x03 {
        return Err(WakeError::invalid_key("public key must be compressed"));
    }
    let point = PublicKey::from_slice(public)
        .map_err(|_| WakeError::invalid_key("public key is not on the curve"))?;
    let xy = ecdh::shared_secret_point(&point, secret);
    let mut out = [0u8; 33];
    out[0] = 0x02 | (xy[63] & 1);
    out[1..].copy_from_slice(&xy[..32]);
    Ok(out)
}

/// `SHA256(SHA256(S33 || label))`, the AES-256 key of both schemes.
///
/// Two plain SHA-256 passes. Not the `sha256d` hash type, whose hex display
/// is byte-reversed.
pub(crate) fn derive_key(shared_point: &[u8; 33], label: &str) -> [u8; 32] {
    let mut input = Vec::with_capacity(shared_point.len() + label.len());
    input.extend_from_slice(shared_point);
    input.extend_from_slice(label.as_bytes());
    let first = sha256::Hash::hash(&input);
    sha256::Hash::hash(first.as_byte_array()).to_byte_array()
}

fn open(
    key: &[u8; 32],
    iv: &[u8],
    ciphertext: &[u8],
    tag: &[u8; TAG_LEN],
) -> Result<Vec<u8>, WakeError> {
    let mut buffer = ciphertext.to_vec();
    let tag = GenericArray::from_slice(tag);
    match iv.len() {
        12 => Aes256Gcm::new(GenericArray::from_slice(key)).decrypt_in_place_detached(
            GenericArray::from_slice(iv),
            b"",
            &mut buffer,
            tag,
        ),
        16 => Aes256Gcm16::new(GenericArray::from_slice(key)).decrypt_in_place_detached(
            GenericArray::from_slice(iv),
            b"",
            &mut buffer,
            tag,
        ),
        _ => return Err(WakeError::invalid_input("iv must be 12 or 16 bytes")),
    }
    .map_err(|_| WakeError::DecryptionFailed {
        reason: "authentication failed".to_string(),
    })?;
    Ok(buffer)
}

/// The legacy plaintext:
/// `{"source":S,"type":T,"payload":<raw>,"createdAt":"..."}`.
#[derive(Deserialize)]
struct LegacyPlaintext {
    source: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Option<Box<RawValue>>,
    #[serde(rename = "createdAt")]
    created_at: String,
}

/// The v1 plaintext:
/// `{"v":1,"id":"<uuid>","topic":T,"urgency":U,"deadline":N,"payload":<raw|null>}`.
#[derive(Deserialize)]
struct V1Plaintext {
    v: u64,
    id: String,
    topic: String,
    urgency: Urgency,
    deadline: u64,
    #[serde(default)]
    payload: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Urgency {
    Background,
    Alert,
    TimeSensitive,
}

impl Urgency {
    const fn as_str(&self) -> &'static str {
        match self {
            Urgency::Background => "background",
            Urgency::Alert => "alert",
            Urgency::TimeSensitive => "time_sensitive",
        }
    }
}

fn malformed_plaintext(e: serde_json::Error) -> WakeError {
    WakeError::invalid_input(format!("malformed plaintext, {}", json_error(&e)))
}

fn legacy_envelope(plaintext: &[u8]) -> Result<WakeEnvelope, WakeError> {
    let legacy: LegacyPlaintext = serde_json::from_slice(plaintext).map_err(malformed_plaintext)?;
    Ok(WakeEnvelope {
        version: 0,
        id: None,
        topic: format!("{}.{}", legacy.source, legacy.kind),
        urgency: None,
        deadline: None,
        payload_json: legacy.payload.map(|p| p.get().to_string()),
        created_at: Some(legacy.created_at),
        fallback: false,
    })
}

fn v1_envelope(plaintext: &[u8]) -> Result<WakeEnvelope, WakeError> {
    let wire: V1Plaintext = serde_json::from_slice(plaintext).map_err(malformed_plaintext)?;
    let id = parse_wake_id(&wire.id)?;
    if wire.v != 1 {
        return Err(WakeError::unsupported("unsupported plaintext version"));
    }
    Ok(WakeEnvelope {
        version: 1,
        id: Some(id),
        topic: wire.topic,
        urgency: Some(wire.urgency.as_str().to_string()),
        deadline: Some(wire.deadline),
        payload_json: wire.payload.map(|p| p.get().to_string()),
        created_at: None,
        fallback: false,
    })
}

/// A wake id in its canonical hyphenated form.
pub(crate) fn parse_wake_id(id: &str) -> Result<String, WakeError> {
    uuid::Uuid::parse_str(id)
        .map(|id| id.to_string())
        .map_err(|_| WakeError::invalid_input("wake id is not a uuid"))
}

/// Parses a 32-byte secp256k1 secret key written as hex.
pub(crate) fn parse_secret_key(secret_key_hex: &str) -> Result<SecretKey, WakeError> {
    let bytes = parse_hex_32(secret_key_hex)
        .ok_or_else(|| WakeError::invalid_key("secret key must be 32 hex bytes"))?;
    SecretKey::from_slice(&bytes).map_err(|_| WakeError::invalid_key("secret key out of range"))
}

pub(crate) fn parse_hex_32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok()?.try_into().ok()
}

/// Fills `dest` from the operating system's random number generator.
pub(crate) fn os_random(dest: &mut [u8]) -> Result<(), WakeError> {
    OsRng
        .try_fill_bytes(dest)
        .map_err(|_| WakeError::invalid_key("the system random number generator failed"))
}

/// Draws secret keys from `fill` until one is in range, at most
/// `MAX_REJECTED_DRAWS` times, so a broken source fails instead of looping.
pub(crate) fn generate_keypair_with(
    mut fill: impl FnMut(&mut [u8]) -> Result<(), WakeError>,
) -> Result<WakeKeyPair, WakeError> {
    for _ in 0..MAX_REJECTED_DRAWS {
        let mut candidate = [0u8; 32];
        fill(&mut candidate)?;
        if let Ok(secret) = SecretKey::from_slice(&candidate) {
            let public = PublicKey::from_secret_key(&Secp256k1::signing_only(), &secret);
            return Ok(WakeKeyPair {
                secret_key_hex: hex::encode(secret.secret_bytes()),
                public_key_hex: hex::encode(public.serialize()),
            });
        }
    }
    Err(WakeError::invalid_key("entropy source is degenerate"))
}

/// Generates the device's push encryption key pair.
pub fn wake_generate_keypair() -> Result<WakeKeyPair, WakeError> {
    generate_keypair_with(os_random)
}

/// Decrypts a legacy (v0) envelope given its four wire fields.
pub fn wake_decrypt_v0(
    secret_key_hex: String,
    cipher: String,
    iv: String,
    tag: String,
    public_key: String,
) -> Result<WakeEnvelope, WakeError> {
    let secret = parse_secret_key(&secret_key_hex)?;
    Container::from_fields(Scheme::V0, &cipher, &iv, &tag, &public_key)?.open_envelope(&secret)
}

/// Decrypts a v1 container given as JSON.
pub fn wake_decrypt_v1(
    secret_key_hex: String,
    container_json: String,
) -> Result<WakeEnvelope, WakeError> {
    let secret = parse_secret_key(&secret_key_hex)?;
    let container = Container::from_json(&container_json)?;
    if container.scheme() != Scheme::V1 {
        return Err(WakeError::unsupported("expected a v1 container"));
    }
    container.open_envelope(&secret)
}

/// Encrypts like the gateway, with an explicit ephemeral key and IV, so tests
/// can rebuild the golden envelopes. Reusing a (key, IV) pair breaks GCM.
#[cfg(test)]
pub(crate) fn seal_deterministic(
    scheme: Scheme,
    device_public: &[u8; 33],
    plaintext: &[u8],
    ephemeral: &SecretKey,
    iv: &[u8],
) -> Container {
    let point = shared_point(ephemeral, device_public).unwrap();
    let key = derive_key(&point, scheme.label());
    let mut buffer = plaintext.to_vec();
    let tag = match iv.len() {
        12 => Aes256Gcm::new(GenericArray::from_slice(&key)).encrypt_in_place_detached(
            GenericArray::from_slice(iv),
            b"",
            &mut buffer,
        ),
        16 => Aes256Gcm16::new(GenericArray::from_slice(&key)).encrypt_in_place_detached(
            GenericArray::from_slice(iv),
            b"",
            &mut buffer,
        ),
        other => panic!("iv of {other} bytes"),
    }
    .unwrap();
    Container {
        scheme,
        cipher: buffer,
        iv: iv.to_vec(),
        tag: tag.into(),
        public_key: PublicKey::from_secret_key(&Secp256k1::signing_only(), ephemeral).serialize(),
    }
}

#[cfg(test)]
impl Container {
    /// Compact container JSON in the wire key order.
    pub(crate) fn to_json(&self) -> String {
        let version = match self.scheme {
            Scheme::V0 => "",
            Scheme::V1 => "\"v\":1,",
        };
        format!(
            "{{{version}\"cipher\":\"{}\",\"iv\":\"{}\",\"tag\":\"{}\",\"publicKey\":\"{}\"}}",
            STANDARD.encode(&self.cipher),
            hex::encode(&self.iv),
            hex::encode(self.tag),
            hex::encode(self.public_key)
        )
    }

    pub(crate) fn tag_mut(&mut self) -> &mut [u8; TAG_LEN] {
        &mut self.tag
    }

    pub(crate) fn public_key_mut(&mut self) -> &mut [u8; 33] {
        &mut self.public_key
    }
}
