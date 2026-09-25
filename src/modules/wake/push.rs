//! Finds the wake in a delivered push, ported from wake-proto
//! `push::parse_delivered`.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::crypto::{parse_secret_key, parse_wake_id, Container, Scheme};
use super::errors::{json_error, WakeError};
use super::types::WakeEnvelope;

/// A wake as the device receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Delivered {
    /// A legacy envelope: APNs `aps.alert.payload`, or the flat FCM data
    /// fields `cipher`, `iv`, `tag` and `publicKey`.
    V0(Container),
    /// A v1 envelope: the APNs `wake` object or the FCM `wake` JSON string.
    V1(Container),
    /// A fallback alert's `wake_fallback` marker; there is nothing to decrypt.
    Fallback { id: String, topic: String },
}

/// Finds the wake in a push.
///
/// `push` is the JSON the app receives:
/// - APNs: the notification dictionary, carrying `aps.alert.payload` (v0),
///   a `wake` object (v1) or a `wake_fallback` object;
/// - FCM: the data map, carrying flat `cipher`, `iv`, `tag` and `publicKey`
///   strings (v0), or `wake` or `wake_fallback` as JSON strings.
///
/// A whole FCM `messages:send` body is accepted too: the map read is
/// `message.android.data` when present, else `message.data`.
pub(crate) fn parse_delivered(push: &str) -> Result<Delivered, WakeError> {
    let root: Value =
        serde_json::from_str(push).map_err(|e| WakeError::invalid_input(json_error(&e)))?;
    let mut map = root
        .as_object()
        .ok_or_else(|| WakeError::invalid_input("push is not a JSON object"))?;
    // The legacy FCM data map has a string `message` (the alert text), so
    // only an object marks a `messages:send` body.
    if let Some(message) = map.get("message").filter(|m| m.is_object()) {
        map = message
            .pointer("/android/data")
            .or_else(|| message.get("data"))
            .and_then(Value::as_object)
            .ok_or_else(|| WakeError::unsupported("FCM message carries no data"))?;
    }
    if let Some(marker) = map.get("wake_fallback") {
        return parse_fallback(marker);
    }
    if let Some(wake) = map.get("wake") {
        let container = match wake {
            Value::String(text) => Container::from_json(text)?,
            Value::Object(_) => Container::from_json(&wake.to_string())?,
            _ => return Err(WakeError::invalid_input("wake is not a container")),
        };
        expect_scheme(&container, Scheme::V1)?;
        return Ok(Delivered::V1(container));
    }
    if let Some(payload) = map.get("aps").and_then(|aps| aps.pointer("/alert/payload")) {
        if !payload.is_object() {
            return Err(WakeError::invalid_input(
                "aps.alert.payload is not a container",
            ));
        }
        let container = Container::from_json(&payload.to_string())?;
        expect_scheme(&container, Scheme::V0)?;
        return Ok(Delivered::V0(container));
    }
    if map.contains_key("cipher") {
        return legacy_fields(map).map(Delivered::V0);
    }
    Err(WakeError::unsupported("push carries no wake"))
}

fn parse_fallback(marker: &Value) -> Result<Delivered, WakeError> {
    #[derive(Deserialize)]
    struct Marker {
        v: Value,
        id: String,
        topic: String,
    }
    let marker: Marker = match marker {
        Value::String(text) => serde_json::from_str(text),
        other => Marker::deserialize(other),
    }
    .map_err(|e| WakeError::invalid_input(json_error(&e)))?;
    let id = parse_wake_id(&marker.id)?;
    if marker.v.as_u64() != Some(1) {
        return Err(WakeError::unsupported("unsupported envelope version"));
    }
    Ok(Delivered::Fallback {
        id,
        topic: marker.topic,
    })
}

fn legacy_fields(map: &Map<String, Value>) -> Result<Container, WakeError> {
    let field = |key: &str| {
        map.get(key).and_then(Value::as_str).ok_or_else(|| {
            WakeError::invalid_input("legacy data needs cipher, iv, tag and publicKey strings")
        })
    };
    Container::from_fields(
        Scheme::V0,
        field("cipher")?,
        field("iv")?,
        field("tag")?,
        field("publicKey")?,
    )
}

fn expect_scheme(container: &Container, scheme: Scheme) -> Result<(), WakeError> {
    if container.scheme() == scheme {
        Ok(())
    } else {
        Err(WakeError::unsupported(
            "container version does not match where it was found",
        ))
    }
}

/// Finds and decrypts the wake in a delivered push. A fallback alert returns
/// `fallback = true` without decrypting, so the key is only read for
/// envelopes.
pub fn wake_decrypt_push(
    secret_key_hex: String,
    push_json: String,
) -> Result<WakeEnvelope, WakeError> {
    match parse_delivered(&push_json)? {
        Delivered::V0(container) | Delivered::V1(container) => {
            container.open_envelope(&parse_secret_key(&secret_key_hex)?)
        }
        Delivered::Fallback { id, topic } => Ok(WakeEnvelope {
            version: 1,
            id: Some(id),
            topic,
            urgency: None,
            deadline: None,
            payload_json: None,
            created_at: None,
            fallback: true,
        }),
    }
}
