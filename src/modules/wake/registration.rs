//! Registration preimage and credentials, ported from wake-proto
//! `registration`, `tokens` and `api`.
//!
//! Every identity signs the same 12-line UTF-8 preimage, lines joined by a
//! single `\n` with no trailing newline:
//!
//! ```text
//! wake-register-v1
//! aud:<server.audience>
//! app:<app>
//! install:<install_id>
//! platform:<apns|fcm>
//! environment:<production|sandbox>
//! push_token:<push_token exactly as sent>
//! encryption_key:<66 hex>
//! secret_sha256:<64 hex>
//! identities:<deduped, sorted bytewise, joined ",">
//! topics:<deduped, sorted bytewise, joined ","; empty gives "topics:">
//! ts:<unix seconds, decimal>
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::PublicKey;
use rand::rngs::OsRng;
use rand::RngCore;

use super::crypto::{os_random, parse_hex_32};
use super::errors::WakeError;
use super::types::{WakeDeviceSecret, WakeEnvironment, WakePlatform, WakeRegistrationRequest};

/// First line of the preimage, and its domain separator.
pub(crate) const PREIMAGE_TAG: &str = "wake-register-v1";

const DEVICE_SECRET_PREFIX: &str = "wkd_";
const MAX_TOPICS: usize = 32;
const MAX_TOPIC_LEN: usize = 64;

/// The fields the registration preimage covers.
pub(crate) struct RegistrationFields<'a> {
    pub(crate) audience: &'a str,
    pub(crate) app: &'a str,
    pub(crate) install_id: &'a str,
    pub(crate) platform: WakePlatform,
    pub(crate) environment: WakeEnvironment,
    pub(crate) push_token: &'a str,
    pub(crate) encryption_key: &'a str,
    pub(crate) secret_sha256: &'a str,
    pub(crate) identities: &'a [String],
    pub(crate) topics: &'a [String],
    pub(crate) ts: u64,
}

impl RegistrationFields<'_> {
    /// Builds the preimage. Fails if any field contains a newline, or any
    /// identity or topic is empty or contains a comma, since each would make
    /// the preimage ambiguous (`[""]` and `[]` both give `topics:`).
    pub(crate) fn preimage(&self) -> Result<String, WakeError> {
        let scalars = [
            self.audience,
            self.app,
            self.install_id,
            self.push_token,
            self.encryption_key,
            self.secret_sha256,
        ];
        if scalars.iter().any(|f| f.contains('\n')) {
            return Err(WakeError::invalid_input(
                "a registration field contains a newline",
            ));
        }
        let identities = sorted_list(self.identities)?;
        let topics = sorted_list(self.topics)?;
        Ok(format!(
            "{PREIMAGE_TAG}\naud:{}\napp:{}\ninstall:{}\nplatform:{}\nenvironment:{}\n\
             push_token:{}\nencryption_key:{}\nsecret_sha256:{}\nidentities:{identities}\n\
             topics:{topics}\nts:{}",
            self.audience,
            self.app,
            self.install_id,
            self.platform.as_str(),
            self.environment.as_str(),
            self.push_token,
            self.encryption_key,
            self.secret_sha256,
            self.ts,
        ))
    }
}

fn sorted_list(items: &[String]) -> Result<String, WakeError> {
    if items
        .iter()
        .any(|i| i.is_empty() || i.contains(',') || i.contains('\n'))
    {
        return Err(WakeError::invalid_input(
            "a list entry is empty or contains a comma or newline",
        ));
    }
    let mut sorted: Vec<&str> = items.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    Ok(sorted.join(","))
}

/// Checks every field the way the gateway does, so a request it would reject
/// as `invalid_request` fails here, before anything is signed.
fn validate(fields: &RegistrationFields<'_>) -> Result<(), WakeError> {
    if fields.audience.is_empty() || fields.app.is_empty() {
        return Err(WakeError::invalid_input(
            "audience and app must not be empty",
        ));
    }
    if !is_valid_install_id(fields.install_id) {
        return Err(WakeError::invalid_input(
            "install_id must be 16 to 64 characters of [A-Za-z0-9_-]",
        ));
    }
    if fields.platform == WakePlatform::Fcm && fields.environment != WakeEnvironment::Production {
        return Err(WakeError::invalid_input(
            "fcm devices are always production",
        ));
    }
    if !is_valid_push_token(fields.platform, fields.push_token) {
        return Err(WakeError::invalid_input(match fields.platform {
            WakePlatform::Apns => "an apns push token is 64 to 200 lowercase hex characters",
            WakePlatform::Fcm => "an fcm push token is 100 to 4096 characters of [0-9A-Za-z_:-]",
        }));
    }
    parse_public_key_hex(fields.encryption_key)?;
    if !is_lower_hex(fields.secret_sha256, 32) {
        return Err(WakeError::invalid_input(
            "secret_sha256 must be 64 lowercase hex characters",
        ));
    }
    let schemes = fields
        .identities
        .iter()
        .map(|identity| identity_scheme(identity))
        .collect::<Result<Vec<_>, _>>()?;
    if !(1..=2).contains(&schemes.len()) || (schemes.len() == 2 && schemes[0] == schemes[1]) {
        return Err(WakeError::invalid_input(
            "identities must be one ln: and/or one pk: identity",
        ));
    }
    if fields.topics.len() > MAX_TOPICS {
        return Err(WakeError::invalid_input("at most 32 topics"));
    }
    if let Some(topic) = fields.topics.iter().find(|t| !is_valid_topic_filter(t)) {
        return Err(WakeError::invalid_input(format!(
            "{topic:?} is not a topic name or a <namespace>.* pattern"
        )));
    }
    Ok(())
}

/// `^[A-Za-z0-9_-]{16,64}$`
fn is_valid_install_id(s: &str) -> bool {
    (16..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// APNs: even-length lowercase hex, 64 to 200 characters. FCM:
/// `^[0-9A-Za-z_:-]{100,4096}$`.
fn is_valid_push_token(platform: WakePlatform, token: &str) -> bool {
    match platform {
        WakePlatform::Apns => {
            // `is_lower_hex` needs exactly twice `len / 2` characters, so odd
            // lengths fail it.
            (64..=200).contains(&token.len()) && is_lower_hex(token, token.len() / 2)
        }
        WakePlatform::Fcm => {
            (100..=4096).contains(&token.len())
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-'))
        }
    }
}

/// Exactly `bytes` bytes written as lowercase hex.
fn is_lower_hex(s: &str, bytes: usize) -> bool {
    s.len() == bytes * 2
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A compressed key written as 66 lowercase hex characters that lies on the
/// curve.
fn parse_public_key_hex(s: &str) -> Result<PublicKey, WakeError> {
    if !is_lower_hex(s, 33) || !(s.starts_with("02") || s.starts_with("03")) {
        return Err(WakeError::invalid_key(
            "a public key is 66 lowercase hex characters starting with 02 or 03",
        ));
    }
    let bytes = hex::decode(s).map_err(|_| WakeError::invalid_key("public key is not hex"))?;
    PublicKey::from_slice(&bytes)
        .map_err(|_| WakeError::invalid_key("public key is not on the curve"))
}

/// The scheme (`"ln"` or `"pk"`) of a canonical identity:
/// - `ln:` + 66 lowercase hex, a compressed secp256k1 key on the curve;
/// - `pk:` + 52 canonical z-base-32 characters, a canonical, non-weak
///   Ed25519 point.
fn identity_scheme(identity: &str) -> Result<&'static str, WakeError> {
    let invalid = || WakeError::invalid_input("identities must be canonical ln: or pk: identities");
    if let Some(key) = identity.strip_prefix("ln:") {
        parse_public_key_hex(key).map_err(|_| invalid())?;
        return Ok("ln");
    }
    let z32 = identity.strip_prefix("pk:").ok_or_else(invalid)?;
    if z32.len() != 52 {
        return Err(invalid());
    }
    let key = pubky::PublicKey::try_from_z32(z32).map_err(|_| invalid())?;
    let point = key.verifying_key();
    let canonical = key.z32() == z32
        && point.to_edwards().compress().to_bytes() == point.to_bytes()
        && !point.is_weak();
    if !canonical {
        return Err(invalid());
    }
    Ok("pk")
}

/// `^[a-z][a-z0-9_-]*(\.[A-Za-z][A-Za-z0-9_-]*)+$` names and `<ns>.*`
/// patterns, at most 64 characters.
fn is_valid_topic_filter(s: &str) -> bool {
    if s.len() > MAX_TOPIC_LEN {
        return false;
    }
    match s.strip_suffix(".*") {
        Some(namespace) => is_namespace(namespace),
        None => is_namespace(s) && s.contains('.'),
    }
}

/// `^[a-z][a-z0-9_-]*(\.[A-Za-z][A-Za-z0-9_-]*)*$`
fn is_namespace(s: &str) -> bool {
    let mut segments = s.split('.');
    let first_ok = segments.next().is_some_and(|first| {
        let mut bytes = first.bytes();
        bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    });
    first_ok
        && segments.all(|segment| {
            let mut bytes = segment.bytes();
            bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
                && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

/// Encodes 32 bytes as a device secret: `wkd_` + 43 base64url characters.
pub(crate) fn device_secret_from_bytes(bytes: &[u8; 32]) -> WakeDeviceSecret {
    let token = format!("{DEVICE_SECRET_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    let sha256_hex = hex::encode(sha256::Hash::hash(token.as_bytes()).to_byte_array());
    WakeDeviceSecret { token, sha256_hex }
}

/// Checks `wkd_` + 43 canonical base64url characters (32 bytes).
pub(crate) fn is_valid_device_secret(token: &str) -> bool {
    token
        .strip_prefix(DEVICE_SECRET_PREFIX)
        .filter(|body| body.len() == 43)
        .and_then(|body| URL_SAFE_NO_PAD.decode(body).ok())
        .is_some_and(|bytes| bytes.len() == 32)
}

/// A fresh install id: 16 random bytes as 22 base64url characters.
pub(crate) fn install_id_from_bytes(bytes: &[u8; 16]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Generates a device secret. Keep the token on the device; register its hash.
pub fn wake_generate_device_secret() -> Result<WakeDeviceSecret, WakeError> {
    let mut bytes = [0u8; 32];
    os_random(&mut bytes)?;
    Ok(device_secret_from_bytes(&bytes))
}

/// Generates an install id, stable for the life of the install.
pub fn wake_generate_install_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    install_id_from_bytes(&bytes)
}

/// Validates a registration and builds the message every identity signs.
/// `timestamp` defaults to now and must be within the gateway's skew window
/// (600 s) when the request is sent.
#[allow(clippy::too_many_arguments)]
pub fn wake_prepare_registration(
    audience: String,
    app: String,
    install_id: String,
    platform: WakePlatform,
    environment: WakeEnvironment,
    push_token: String,
    encryption_public_key: String,
    secret_sha256: String,
    identities: Vec<String>,
    topics: Vec<String>,
    timestamp: Option<u64>,
) -> Result<WakeRegistrationRequest, WakeError> {
    let timestamp = match timestamp {
        Some(ts) => ts,
        None => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| WakeError::invalid_input("the system clock is before 1970"))?
            .as_secs(),
    };
    let fields = RegistrationFields {
        audience: &audience,
        app: &app,
        install_id: &install_id,
        platform,
        environment,
        push_token: &push_token,
        encryption_key: &encryption_public_key,
        secret_sha256: &secret_sha256,
        identities: &identities,
        topics: &topics,
        ts: timestamp,
    };
    let message = fields.preimage()?;
    validate(&fields)?;
    Ok(WakeRegistrationRequest {
        audience,
        app,
        install_id,
        platform,
        environment,
        push_token,
        encryption_public_key,
        secret_sha256,
        identities,
        topics,
        timestamp,
        message,
    })
}

/// Signs a registration message with a pubky Ed25519 secret key and returns
/// the 128-character lowercase hex signature. Only registration messages
/// are signed, so the key cannot be used here to sign anything else.
pub fn wake_sign_pubky_proof(secret_key_hex: String, message: String) -> Result<String, WakeError> {
    if !message
        .strip_prefix(PREIMAGE_TAG)
        .is_some_and(|rest| rest.starts_with('\n'))
    {
        return Err(WakeError::invalid_input(
            "only wake registration messages can be signed",
        ));
    }
    let seed = parse_hex_32(&secret_key_hex)
        .ok_or_else(|| WakeError::invalid_key("secret key must be 32 hex bytes"))?;
    let keypair = pubky::Keypair::from_secret(&seed);
    Ok(hex::encode(keypair.sign(message.as_bytes()).to_bytes()))
}
