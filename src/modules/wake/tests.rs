use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use reqwest::Method;
use serde_json::{json, Value};

use super::client::*;
use super::crypto::*;
use super::push::*;
use super::registration::*;
use super::*;

// The golden vectors of wake-proto, copied byte for byte (see README.md).
const V0_SERVER: &str = include_str!("test_vectors/v0_server.json");
const V0_CLIENT: &str = include_str!("test_vectors/v0_client.json");
const V1_ENVELOPE: &str = include_str!("test_vectors/v1_envelope.json");
const REGISTRATION_V1: &str = include_str!("test_vectors/registration_v1.json");
const LEGACY_REGISTRATION: &str = include_str!("test_vectors/legacy_registration.json");
const PUSHES: &str = include_str!("test_vectors/pushes.json");

const GATEWAY: &str = "https://wake.example";
const DEVICE_SECRET: &str = "wkd_d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3c";
const WAKE_ID: &str = "01a0c450-6c00-7000-8000-000000000001";

fn load(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string {key}"))
}

fn secret(v: &Value, key: &str) -> SecretKey {
    parse_secret_key(s(v, key)).unwrap()
}

fn public_hex(secret: &SecretKey) -> String {
    hex::encode(PublicKey::from_secret_key(&Secp256k1::signing_only(), secret).serialize())
}

fn bytes33(hex_str: &str) -> [u8; 33] {
    hex::decode(hex_str).unwrap().try_into().unwrap()
}

fn invalid_input(reason: &str) -> WakeError {
    WakeError::InvalidInput {
        reason: reason.to_string(),
    }
}

fn unsupported(reason: &str) -> WakeError {
    WakeError::UnsupportedEnvelope {
        reason: reason.to_string(),
    }
}

fn decryption_failed() -> WakeError {
    WakeError::DecryptionFailed {
        reason: "authentication failed".to_string(),
    }
}

/// `SHA256(SHA256("Lightning Signed Message:" || message))`.
fn lightning_message_hash(message: &[u8]) -> [u8; 32] {
    let mut input = b"Lightning Signed Message:".to_vec();
    input.extend_from_slice(message);
    sha256::Hash::hash(sha256::Hash::hash(&input).as_byte_array()).to_byte_array()
}

/// A local stand-in for ldk-node `signMessage`: a recoverable RFC 6979
/// signature, `[31 + recid] || r || s`, in zbase32.
fn lightning_sign(secret: &SecretKey, message: &[u8]) -> String {
    let digest = Message::from_digest(lightning_message_hash(message));
    let (recovery_id, compact) = Secp256k1::signing_only()
        .sign_ecdsa_recoverable(&digest, secret)
        .serialize_compact();
    let mut bytes = vec![31 + recovery_id.to_i32() as u8];
    bytes.extend_from_slice(&compact);
    zbase32(&bytes)
}

fn zbase32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Checks an envelope vector's intermediates from the file alone: the
/// device's view (S33 from its secret and the ephemeral key), the key, the
/// container round trip and the plaintext.
fn check_envelope_vector(v: &Value, scheme: Scheme) {
    let device = secret(v, "device_secret_key");
    assert_eq!(public_hex(&device), s(v, "device_public_key"));
    let shared = shared_point(&device, &bytes33(s(v, "ephemeral_public_key"))).unwrap();
    assert_eq!(hex::encode(shared), s(v, "shared_point"));
    assert_eq!(s(v, "label"), scheme.label());
    assert_eq!(
        hex::encode(derive_key(&shared, scheme.label())),
        s(v, "key")
    );
    if v.get("ephemeral_secret_key").is_some() {
        let ephemeral = secret(v, "ephemeral_secret_key");
        assert_eq!(public_hex(&ephemeral), s(v, "ephemeral_public_key"));
        let from_gateway = shared_point(&ephemeral, &bytes33(s(v, "device_public_key"))).unwrap();
        assert_eq!(from_gateway, shared);
    }
    let container = Container::from_json(s(v, "container")).unwrap();
    assert_eq!(container.scheme(), scheme);
    assert_eq!(container.to_json(), s(v, "container"));
    let plaintext = String::from_utf8(container.decrypt(&device).unwrap()).unwrap();
    assert_eq!(plaintext, s(v, "plaintext"));
}

// ----- (a) v0 server envelope -----

#[test]
fn v0_server_vector() {
    let v = load(V0_SERVER);
    check_envelope_vector(&v, Scheme::V0);
    assert_eq!(
        s(&v, "shared_point"),
        "0277e0510d5042e2f5e9e59c977b81eeed590cf7d20c1c51da451a8eaa9fdc45ff"
    );
    assert_eq!(
        s(&v, "key"),
        "954b48c50f1c6bf3a6cd20a6d7354775cb0d712a3e332333c87dcd52b83f42f9"
    );

    // Encrypting the plaintext the way the gateway does gives the same bytes.
    let rebuilt = seal_deterministic(
        Scheme::V0,
        &bytes33(s(&v, "device_public_key")),
        s(&v, "plaintext").as_bytes(),
        &secret(&v, "ephemeral_secret_key"),
        &hex::decode(s(&v, "iv")).unwrap(),
    );
    assert_eq!(rebuilt.to_json(), s(&v, "container"));
    assert_eq!(rebuilt.to_json().len(), 323);
    assert_eq!(v["container_len"], 323);

    let envelope = wake_decrypt_v0(
        s(&v, "device_secret_key").to_string(),
        s(&v, "cipher").to_string(),
        s(&v, "iv").to_string(),
        s(&v, "tag").to_string(),
        s(&v, "ephemeral_public_key").to_string(),
    )
    .unwrap();
    assert_eq!(
        envelope,
        WakeEnvelope {
            version: 0,
            id: None,
            topic: s(&v, "topic").to_string(),
            urgency: None,
            deadline: None,
            payload_json: Some(s(&v, "payload").to_string()),
            created_at: Some(s(&v, "created_at").to_string()),
            fallback: false,
        }
    );
}

// ----- (b) v0 client vector from the apps -----

#[test]
fn v0_client_vector() {
    let v = load(V0_CLIENT);
    check_envelope_vector(&v, Scheme::V0);
    assert_eq!(
        s(&v, "shared_point"),
        "028ce542975d6d7b2307c92e527d507b03ffb3d897eb2e0830d29f40d5efd80ee3"
    );
    assert_eq!(
        s(&v, "key"),
        "3a9d552cb16dfae40feae644254c4ca46cab82e570de5662aacc4018e33b609b"
    );

    let expected = WakeEnvelope {
        version: 0,
        id: None,
        topic: "blocktank.incomingHtlc".to_string(),
        urgency: None,
        deadline: None,
        payload_json: Some(r#"{"secretMessage":"hello"}"#.to_string()),
        created_at: Some("2024-09-18T13:33:52.555Z".to_string()),
        fallback: false,
    };
    let envelope = wake_decrypt_v0(
        s(&v, "device_secret_key").to_string(),
        s(&v, "cipher").to_string(),
        s(&v, "iv").to_string(),
        s(&v, "tag").to_string(),
        s(&v, "ephemeral_public_key").to_string(),
    )
    .unwrap();
    assert_eq!(envelope, expected);

    // The same envelope as each platform delivers it.
    let container: Value = serde_json::from_str(s(&v, "container")).unwrap();
    let apns = json!({"aps": {"alert": {"title": "t", "body": "b", "payload": container}}});
    let mut fcm = container.clone();
    fcm["title"] = json!("Background Operation Request");
    fcm["message"] = json!("Please open your wallet.");
    fcm["sound"] = json!("default");
    for push in [apns, fcm] {
        let envelope =
            wake_decrypt_push(s(&v, "device_secret_key").to_string(), push.to_string()).unwrap();
        assert_eq!(envelope, expected, "{push}");
    }
}

// ----- (d) v1 envelope -----

#[test]
fn v1_envelope_vector() {
    let v = load(V1_ENVELOPE);
    check_envelope_vector(&v, Scheme::V1);

    let rebuilt = seal_deterministic(
        Scheme::V1,
        &bytes33(s(&v, "device_public_key")),
        s(&v, "plaintext").as_bytes(),
        &secret(&v, "ephemeral_secret_key"),
        &hex::decode(s(&v, "iv")).unwrap(),
    );
    assert_eq!(rebuilt.to_json(), s(&v, "container"));
    assert_eq!(rebuilt.to_json().len(), 501);

    let envelope = wake_decrypt_v1(
        s(&v, "device_secret_key").to_string(),
        s(&v, "container").to_string(),
    )
    .unwrap();
    assert_eq!(
        envelope,
        WakeEnvelope {
            version: 1,
            id: Some(WAKE_ID.to_string()),
            topic: "blocktank.incomingHtlc".to_string(),
            urgency: Some("background".to_string()),
            deadline: Some(1790000060),
            payload_json: Some(
                r#"{"lspId":"blocktank-lsp","paymentHash":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}"#
                    .to_string()
            ),
            created_at: None,
            fallback: false,
        }
    );
}

// ----- (e) registration preimage and proofs -----

fn vector_registration(v: &Value) -> WakeRegistrationRequest {
    let request = &v["request"];
    let strings = |key: &str| -> Vec<String> {
        request[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_str().unwrap().to_string())
            .collect()
    };
    wake_prepare_registration(
        s(v, "audience").to_string(),
        s(request, "app").to_string(),
        s(request, "install_id").to_string(),
        WakePlatform::Apns,
        WakeEnvironment::Sandbox,
        s(request, "push_token").to_string(),
        s(request, "encryption_key").to_string(),
        s(request, "secret_sha256").to_string(),
        strings("identities"),
        strings("topics"),
        request["ts"].as_u64(),
    )
    .unwrap()
}

#[test]
fn registration_v1_vector() {
    let v = load(REGISTRATION_V1);
    let registration = vector_registration(&v);
    assert_eq!(registration.message, s(&v, "preimage"));
    assert_eq!(registration.message.lines().count(), 12);
    assert_eq!(
        hex::encode(sha256::Hash::hash(registration.message.as_bytes()).to_byte_array()),
        s(&v, "preimage_sha256")
    );
    assert_eq!(
        hex::encode(lightning_message_hash(registration.message.as_bytes())),
        s(&v, "ln_message_hash")
    );

    // The credentials the request carries.
    let device_secret = device_secret_from_bytes(
        &hex::decode(s(&v, "device_secret_bytes"))
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert_eq!(device_secret.token, s(&v, "device_secret"));
    assert_eq!(device_secret.sha256_hex, registration.secret_sha256);
    assert_eq!(
        public_hex(&secret(&v, "encryption_secret_key")),
        registration.encryption_public_key
    );
    let ln_secret = secret(&v, "ln_secret_key");
    assert_eq!(
        format!("ln:{}", public_hex(&ln_secret)),
        s(&v, "ln_identity")
    );
    let pk_seed: [u8; 32] = hex::decode(s(&v, "pk_seed")).unwrap().try_into().unwrap();
    let pk_public = pubky::Keypair::from_secret(&pk_seed).public_key();
    assert_eq!(hex::encode(pk_public.to_bytes()), s(&v, "pk_public_key"));
    assert_eq!(format!("pk:{}", pk_public.z32()), s(&v, "pk_identity"));

    // Both proofs, byte for byte.
    let pk_proof =
        wake_sign_pubky_proof(s(&v, "pk_seed").to_string(), registration.message.clone()).unwrap();
    assert_eq!(pk_proof, s(&v, "pk_proof"));
    assert_eq!(
        lightning_sign(&ln_secret, registration.message.as_bytes()),
        s(&v, "ln_proof")
    );

    // The request body, proofs included, is the vector's request.
    let proofs = vec![
        WakeIdentityProof {
            identity: s(&v, "ln_identity").to_string(),
            signature: s(&v, "ln_proof").to_string(),
        },
        WakeIdentityProof {
            identity: s(&v, "pk_identity").to_string(),
            signature: pk_proof,
        },
    ];
    let request = build_register(GATEWAY, &registration, &proofs).unwrap();
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.url.as_str(), "https://wake.example/v1/devices");
    assert!(request.bearer.is_none());
    let body = request.body.unwrap();
    assert!(body.starts_with(r#"{"app":"bitkit","install_id":"AAAAAAAAAAAAAAAAAAAAAA","#));
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), v["request"]);

    // Fields changed after the message was prepared are refused.
    for tweak in 0..3 {
        let mut changed = registration.clone();
        match tweak {
            0 => changed.topics.push("paykit.*".to_string()),
            1 => changed.audience = "wake.example".to_string(),
            _ => changed.timestamp += 1,
        }
        assert_eq!(
            build_register(GATEWAY, &changed, &proofs).err(),
            Some(invalid_input(
                "the registration changed after its message was prepared"
            ))
        );
    }

    // A different audience changes the message.
    let other = vector_registration(&json!({"audience": "wake.example", "request": v["request"]}));
    assert_ne!(other.message, registration.message);
}

#[test]
fn legacy_registration_vector_checks_the_test_signer() {
    let v = load(LEGACY_REGISTRATION);
    let message = format!(
        "bitkit-notifications{}{}",
        s(&v, "device_token"),
        s(&v, "iso_timestamp")
    );
    assert_eq!(message, s(&v, "message"));
    assert_eq!(
        hex::encode(lightning_message_hash(message.as_bytes())),
        s(&v, "message_hash")
    );
    let node = secret(&v, "node_secret_key");
    assert_eq!(public_hex(&node), s(&v, "node_id"));
    assert_eq!(
        lightning_sign(&node, message.as_bytes()),
        s(&v, "signature_zbase32")
    );
}

// ----- (f) every push shape -----

#[test]
fn pushes_vector() {
    let v = load(PUSHES);
    let cases = v["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let name = s(case, "name");
        let body_text = s(case, "body");
        assert_eq!(
            case["body_len"].as_u64().unwrap() as usize,
            body_text.len(),
            "{name}"
        );
        let body: Value = serde_json::from_str(body_text).unwrap();
        let delivered = parse_delivered(body_text).unwrap();
        // An FCM device sees only the data map.
        let mut views = vec![body_text.to_string()];
        if s(case, "provider") == "fcm" {
            let message = &body["message"];
            let data = message.pointer("/android/data").unwrap_or(&message["data"]);
            for map in [data, &message["data"]] {
                assert_eq!(
                    parse_delivered(&map.to_string()).unwrap(),
                    delivered,
                    "{name}"
                );
                views.push(map.to_string());
            }
        }
        match &delivered {
            Delivered::V0(container) | Delivered::V1(container) => {
                let scheme = if name.ends_with("v0") {
                    Scheme::V0
                } else {
                    Scheme::V1
                };
                assert_eq!(container.scheme(), scheme, "{name}");
                assert_eq!(
                    container.to_json(),
                    s(&case["inputs"], "container"),
                    "{name}"
                );
                let device = secret(case, "device_secret_key");
                let plaintext = String::from_utf8(container.decrypt(&device).unwrap()).unwrap();
                assert_eq!(plaintext, s(case, "plaintext"), "{name}");
                for view in views {
                    let envelope =
                        wake_decrypt_push(s(case, "device_secret_key").to_string(), view.clone())
                            .unwrap();
                    check_envelope_matches_plaintext(&envelope, &plaintext, case, name);
                }
            }
            Delivered::Fallback { id, topic } => {
                assert!(case.get("device_secret_key").is_none(), "{name}");
                assert_eq!(id, s(&case["inputs"], "wake_id"), "{name}");
                assert_eq!(topic, s(&case["inputs"], "topic"), "{name}");
                for view in views {
                    // A fallback needs no key, so an unusable one is fine.
                    let envelope = wake_decrypt_push(String::new(), view).unwrap();
                    assert_eq!(
                        envelope,
                        WakeEnvelope {
                            version: 1,
                            id: Some(id.clone()),
                            topic: topic.clone(),
                            urgency: None,
                            deadline: None,
                            payload_json: None,
                            created_at: None,
                            fallback: true,
                        },
                        "{name}"
                    );
                }
            }
        }
    }
}

fn check_envelope_matches_plaintext(
    envelope: &WakeEnvelope,
    plaintext: &str,
    case: &Value,
    name: &str,
) {
    let parsed: Value = serde_json::from_str(plaintext).unwrap();
    assert!(!envelope.fallback, "{name}");
    let payload = envelope.payload_json.as_deref().unwrap_or("null");
    assert_eq!(
        serde_json::from_str::<Value>(payload).unwrap(),
        parsed["payload"],
        "{name}"
    );
    if envelope.version == 0 {
        let topic = format!("{}.{}", s(&parsed, "source"), s(&parsed, "type"));
        assert_eq!(envelope.topic, topic, "{name}");
        assert_eq!(
            envelope.created_at.as_deref(),
            parsed["createdAt"].as_str(),
            "{name}"
        );
        assert!(
            plaintext.contains(&format!("\"payload\":{payload},\"createdAt\"")),
            "{name}"
        );
        assert_eq!(envelope.id, None, "{name}");
    } else {
        assert_eq!(envelope.version, 1, "{name}");
        assert_eq!(envelope.id.as_deref(), Some(s(&case["inputs"], "wake_id")));
        assert_eq!(envelope.topic, s(&case["inputs"], "topic"), "{name}");
        assert_eq!(envelope.urgency.as_deref(), parsed["urgency"].as_str());
        assert_eq!(envelope.deadline, parsed["deadline"].as_u64(), "{name}");
        assert!(
            plaintext.ends_with(&format!("\"payload\":{payload}}}")),
            "{name}"
        );
        assert_eq!(envelope.created_at, None, "{name}");
    }
}

// ----- Negative crypto cases -----

fn v1_vector_container() -> (SecretKey, Container) {
    let v = load(V1_ENVELOPE);
    (
        secret(&v, "device_secret_key"),
        Container::from_json(s(&v, "container")).unwrap(),
    )
}

#[test]
fn tampered_tag_fails() {
    let (device, mut container) = v1_vector_container();
    container.tag_mut()[0] ^= 1;
    assert_eq!(container.decrypt(&device), Err(decryption_failed()));
    assert_eq!(
        wake_decrypt_v1(hex::encode(device.secret_bytes()), container.to_json()),
        Err(decryption_failed())
    );
}

#[test]
fn tampered_cipher_fails() {
    let v = load(V0_SERVER);
    let cipher = s(&v, "cipher").replacen('m', "n", 1);
    let result = wake_decrypt_v0(
        s(&v, "device_secret_key").to_string(),
        cipher,
        s(&v, "iv").to_string(),
        s(&v, "tag").to_string(),
        s(&v, "ephemeral_public_key").to_string(),
    );
    assert_eq!(result, Err(decryption_failed()));
}

#[test]
fn wrong_key_fails() {
    let (_, container) = v1_vector_container();
    assert_eq!(
        wake_decrypt_v1("23".repeat(32), container.to_json()),
        Err(decryption_failed())
    );
}

#[test]
fn wrong_label_fails() {
    // A v1 container read as a legacy one derives the key with the v0 label.
    let v = load(V1_ENVELOPE);
    let result = wake_decrypt_v0(
        s(&v, "device_secret_key").to_string(),
        s(&v, "cipher").to_string(),
        s(&v, "iv").to_string(),
        s(&v, "tag").to_string(),
        s(&v, "ephemeral_public_key").to_string(),
    );
    assert_eq!(result, Err(decryption_failed()));
}

#[test]
fn eleven_byte_iv_is_rejected() {
    let (device, container) = v1_vector_container();
    let json = container
        .to_json()
        .replace("000102030405060708090a0b", &"00".repeat(11));
    let iv_error = invalid_input("iv must be 12 or 16 bytes");
    assert_eq!(Container::from_json(&json), Err(iv_error.clone()));
    assert_eq!(
        wake_decrypt_v1(hex::encode(device.secret_bytes()), json),
        Err(iv_error.clone())
    );
    let v = load(V0_SERVER);
    let result = wake_decrypt_v0(
        s(&v, "device_secret_key").to_string(),
        s(&v, "cipher").to_string(),
        "00".repeat(11),
        s(&v, "tag").to_string(),
        s(&v, "ephemeral_public_key").to_string(),
    );
    assert_eq!(result, Err(iv_error));
}

#[test]
fn unknown_version_is_unsupported() {
    let (device, container) = v1_vector_container();
    for other in ["2", "\"1\"", "null", "1.0", "0", "{}"] {
        let json = container
            .to_json()
            .replacen("\"v\":1", &format!("\"v\":{other}"), 1);
        assert_eq!(
            Container::from_json(&json),
            Err(unsupported("unsupported envelope version")),
            "{other}"
        );
        assert_eq!(
            wake_decrypt_v1(hex::encode(device.secret_bytes()), json.clone()),
            Err(unsupported("unsupported envelope version")),
            "{other}"
        );
        let push = format!("{{\"wake\":{json}}}");
        assert_eq!(
            wake_decrypt_push(hex::encode(device.secret_bytes()), push),
            Err(unsupported("unsupported envelope version")),
            "{other}"
        );
    }
    // A container without `v` is a legacy one, which the v1 entry point refuses.
    let v0 = load(V0_SERVER);
    assert_eq!(
        wake_decrypt_v1(
            s(&v0, "device_secret_key").to_string(),
            s(&v0, "container").to_string()
        ),
        Err(unsupported("expected a v1 container"))
    );
}

#[test]
fn unsupported_plaintext_version_and_malformed_plaintexts() {
    let v = load(V1_ENVELOPE);
    let device = secret(&v, "device_secret_key");
    let seal = |plaintext: &str| {
        seal_deterministic(
            Scheme::V1,
            &bytes33(s(&v, "device_public_key")),
            plaintext.as_bytes(),
            &secret(&v, "ephemeral_secret_key"),
            &[7; 12],
        )
        .open_envelope(&device)
    };
    let v2 = s(&v, "plaintext").replacen("\"v\":1", "\"v\":2", 1);
    assert_eq!(seal(&v2), Err(unsupported("unsupported plaintext version")));
    let secret_payload = "paymentHash=e3b0c44298fc1c14";
    for bad in [
        s(&v, "plaintext").replacen("\"background\"", "\"urgent\"", 1),
        s(&v, "plaintext").replacen(WAKE_ID, "not-a-uuid", 1),
        format!(
            r#"{{"v":1,"id":"{WAKE_ID}","topic":"a.b","urgency":"alert","deadline":"{secret_payload}"}}"#
        ),
    ] {
        let err = seal(&bad).unwrap_err();
        assert!(matches!(err, WakeError::InvalidInput { .. }), "{err:?}");
        assert!(!err.to_string().contains("e3b0c442"), "{err}");
    }
    // A null or absent payload is no payload.
    let null_payload = format!(
        r#"{{"v":1,"id":"{WAKE_ID}","topic":"paykit.sync","urgency":"background","deadline":1,"payload":null}}"#
    );
    assert_eq!(seal(&null_payload).unwrap().payload_json, None);
    let absent = null_payload.replace(",\"payload\":null", "");
    assert_eq!(seal(&absent).unwrap().payload_json, None);
}

#[test]
fn unpadded_base64_is_rejected() {
    let v = load(V0_SERVER);
    let json = s(&v, "container").replacen("O9h\"", "O9h=\"", 1);
    assert!(Container::from_json(&json).is_err());
    let unpadded = load(V0_CLIENT);
    let json = s(&unpadded, "container").replacen("A=\"", "A\"", 1);
    assert_eq!(
        Container::from_json(&json),
        Err(invalid_input("cipher is not padded standard base64"))
    );
}

#[test]
fn off_curve_ephemeral_key_fails_decryption() {
    let (device, mut container) = v1_vector_container();
    let key = container.public_key_mut();
    key[0] = 0x02;
    key[1..].copy_from_slice(&[0xff; 32]);
    assert!(matches!(
        container.decrypt(&device),
        Err(WakeError::InvalidKey { .. })
    ));
}

#[test]
fn malformed_secret_keys_are_invalid_keys() {
    let (_, container) = v1_vector_container();
    for bad in [
        String::new(),
        "zz".repeat(32),
        "11".repeat(31),
        "00".repeat(32),
        "ff".repeat(32),
    ] {
        let result = wake_decrypt_v1(bad.clone(), container.to_json());
        assert!(
            matches!(result, Err(WakeError::InvalidKey { .. })),
            "{bad}: {result:?}"
        );
    }
}

// ----- Push shape detection -----

#[test]
fn parse_delivered_rejects_what_it_cannot_place() {
    let v0 = load(V0_SERVER);
    let v0 = s(&v0, "container");
    let v1 = load(V1_ENVELOPE);
    let v1 = s(&v1, "container");
    let v2 = v1.replacen("\"v\":1", "\"v\":2", 1);
    let misplaced = Err(unsupported(
        "container version does not match where it was found",
    ));
    assert_eq!(parse_delivered(&format!("{{\"wake\":{v0}}}")), misplaced);
    assert_eq!(
        parse_delivered(&format!(
            "{{\"wake\":{}}}",
            serde_json::to_string(v0).unwrap()
        )),
        misplaced
    );
    assert_eq!(
        parse_delivered(&format!("{{\"aps\":{{\"alert\":{{\"payload\":{v1}}}}}}}")),
        misplaced
    );
    assert_eq!(
        parse_delivered(&format!("{{\"wake\":{v2}}}")),
        Err(unsupported("unsupported envelope version"))
    );
    assert_eq!(
        parse_delivered(&format!(
            "{{\"wake\":{}}}",
            serde_json::to_string(&v2).unwrap()
        )),
        Err(unsupported("unsupported envelope version"))
    );
    assert_eq!(
        parse_delivered(&format!(
            r#"{{"wake_fallback":{{"v":2,"id":"{WAKE_ID}","topic":"a.b"}}}}"#
        )),
        Err(unsupported("unsupported envelope version"))
    );
    assert!(matches!(
        parse_delivered(r#"{"wake_fallback":"{\"v\":1,\"topic\":\"a.b\"}"}"#),
        Err(WakeError::InvalidInput { .. })
    ));
    assert_eq!(
        parse_delivered(r#"{"wake_fallback":{"v":1,"id":"x","topic":"a.b"}}"#),
        Err(invalid_input("wake id is not a uuid"))
    );
    assert_eq!(
        parse_delivered(r#"{"cipher":"AA==","iv":"00"}"#),
        Err(invalid_input(
            "legacy data needs cipher, iv, tag and publicKey strings"
        ))
    );
    assert_eq!(
        parse_delivered(r#"{"wake":7}"#),
        Err(invalid_input("wake is not a container"))
    );
    assert_eq!(
        parse_delivered(r#"{"aps":{"alert":{"payload":"x"}}}"#),
        Err(invalid_input("aps.alert.payload is not a container"))
    );
    assert_eq!(
        parse_delivered(r#"{"aps":{"content-available":1}}"#),
        Err(unsupported("push carries no wake"))
    );
    assert_eq!(
        parse_delivered(r#"{"message":{"token":"t"}}"#),
        Err(unsupported("FCM message carries no data"))
    );
    assert_eq!(
        parse_delivered("[]"),
        Err(invalid_input("push is not a JSON object"))
    );
    assert!(matches!(
        parse_delivered("{"),
        Err(WakeError::InvalidInput { .. })
    ));
    // The legacy FCM data map's string `message` is alert text, not a body.
    let flat: Value = serde_json::from_str(v0).unwrap();
    let mut flat = flat.as_object().unwrap().clone();
    flat.insert("message".to_string(), json!("Please open your wallet."));
    assert!(matches!(
        parse_delivered(&Value::Object(flat).to_string()),
        Ok(Delivered::V0(_))
    ));
}

#[test]
fn fallback_markers_in_both_encodings() {
    let marker = json!({"v": 1, "id": WAKE_ID.to_uppercase(), "topic": "blocktank.incomingHtlc"});
    let expected = Delivered::Fallback {
        id: WAKE_ID.to_string(),
        topic: "blocktank.incomingHtlc".to_string(),
    };
    let apns = json!({"aps": {"alert": {"title": "t", "body": "b"}}, "wake_fallback": marker});
    let fcm = json!({"wake_fallback": marker.to_string()});
    assert_eq!(parse_delivered(&apns.to_string()).unwrap(), expected);
    assert_eq!(parse_delivered(&fcm.to_string()).unwrap(), expected);
}

// ----- Registration -----

fn fields<'a>(identities: &'a [String], topics: &'a [String]) -> RegistrationFields<'a> {
    RegistrationFields {
        audience: "wake.localhost",
        app: "bitkit",
        install_id: "AAAAAAAAAAAAAAAAAAAAAA",
        platform: WakePlatform::Apns,
        environment: WakeEnvironment::Sandbox,
        push_token: "00ff",
        encryption_key: "02aa",
        secret_sha256: "bb",
        identities,
        topics,
        ts: 1790000000,
    }
}

#[test]
fn preimage_sorts_and_dedupes() {
    let ids = vec!["pk:b".to_string(), "ln:a".to_string(), "pk:b".to_string()];
    let topics = vec!["paykit.sync".to_string(), "blocktank.*".to_string()];
    let pre = fields(&ids, &topics).preimage().unwrap();
    assert_eq!(
        pre,
        "wake-register-v1\naud:wake.localhost\napp:bitkit\ninstall:AAAAAAAAAAAAAAAAAAAAAA\n\
         platform:apns\nenvironment:sandbox\npush_token:00ff\nencryption_key:02aa\n\
         secret_sha256:bb\nidentities:ln:a,pk:b\ntopics:blocktank.*,paykit.sync\nts:1790000000"
    );
    assert_eq!(pre.lines().count(), 12);
    assert!(!pre.ends_with('\n'));
}

#[test]
fn empty_topics_give_a_bare_label() {
    let ids = vec!["ln:a".to_string()];
    let pre = fields(&ids, &[]).preimage().unwrap();
    assert!(pre.contains("\ntopics:\nts:"));
}

#[test]
fn ambiguous_preimage_fields_are_rejected() {
    let ids = vec!["ln:a".to_string()];
    let comma = vec!["a.b,c.d".to_string()];
    let list_error = Err(invalid_input(
        "a list entry is empty or contains a comma or newline",
    ));
    assert_eq!(fields(&ids, &comma).preimage(), list_error);
    let newline_topic = vec!["a.b\nc.d".to_string()];
    assert_eq!(fields(&ids, &newline_topic).preimage(), list_error);
    for field in 0..6 {
        let mut f = fields(&ids, &[]);
        let slot = match field {
            0 => &mut f.audience,
            1 => &mut f.app,
            2 => &mut f.install_id,
            3 => &mut f.push_token,
            4 => &mut f.encryption_key,
            _ => &mut f.secret_sha256,
        };
        *slot = "ab\ncd";
        assert_eq!(
            f.preimage(),
            Err(invalid_input("a registration field contains a newline")),
            "{field}"
        );
    }
}

#[test]
fn empty_list_entries_are_rejected() {
    let list_error = Err(invalid_input(
        "a list entry is empty or contains a comma or newline",
    ));
    let ids = vec!["ln:a".to_string()];
    assert_eq!(fields(&ids, &[String::new()]).preimage(), list_error);
    let with_empty = vec!["a.b".to_string(), String::new()];
    assert_eq!(fields(&ids, &with_empty).preimage(), list_error);
    assert_eq!(fields(&[String::new()], &[]).preimage(), list_error);
    assert!(fields(&[], &[]).preimage().is_ok());
}

/// Changes one argument of a registration.
type Tweak = Box<dyn Fn(&mut Args)>;

/// The vector's registration arguments, for tweaking one at a time.
struct Args {
    audience: String,
    app: String,
    install_id: String,
    platform: WakePlatform,
    environment: WakeEnvironment,
    push_token: String,
    encryption_key: String,
    secret_sha256: String,
    identities: Vec<String>,
    topics: Vec<String>,
}

impl Args {
    fn from_vector() -> Self {
        let r = vector_registration(&load(REGISTRATION_V1));
        Args {
            audience: r.audience,
            app: r.app,
            install_id: r.install_id,
            platform: r.platform,
            environment: r.environment,
            push_token: r.push_token,
            encryption_key: r.encryption_public_key,
            secret_sha256: r.secret_sha256,
            identities: r.identities,
            topics: r.topics,
        }
    }

    fn prepare(self) -> Result<WakeRegistrationRequest, WakeError> {
        wake_prepare_registration(
            self.audience,
            self.app,
            self.install_id,
            self.platform,
            self.environment,
            self.push_token,
            self.encryption_key,
            self.secret_sha256,
            self.identities,
            self.topics,
            Some(1790000000),
        )
    }
}

#[test]
fn prepare_rejects_what_the_gateway_would() {
    let ln = load(REGISTRATION_V1)["ln_identity"]
        .as_str()
        .unwrap()
        .to_string();
    let pk = load(REGISTRATION_V1)["pk_identity"]
        .as_str()
        .unwrap()
        .to_string();
    let other_ln = format!(
        "ln:{}",
        load(V0_SERVER)["device_public_key"].as_str().unwrap()
    );
    let mut non_canonical_pk = pk.clone();
    // The last character carries 1 data bit and 4 padding bits.
    assert!(non_canonical_pk.ends_with('y'));
    non_canonical_pk.pop();
    non_canonical_pk.push('b');
    let cases: Vec<(&str, Tweak)> = vec![
        (
            "empty audience",
            Box::new(|a: &mut Args| a.audience.clear()),
        ),
        (
            "short install id",
            Box::new(|a: &mut Args| a.install_id = "A".repeat(15)),
        ),
        (
            "padded install id",
            Box::new(|a: &mut Args| a.install_id.push('=')),
        ),
        (
            "fcm sandbox",
            Box::new(|a: &mut Args| {
                a.platform = WakePlatform::Fcm;
                a.push_token = "a:b_c-d".repeat(15);
            }),
        ),
        (
            "uppercase apns token",
            Box::new(|a: &mut Args| a.push_token = a.push_token.to_uppercase()),
        ),
        (
            "odd apns token",
            Box::new(|a: &mut Args| a.push_token.push('0')),
        ),
        (
            "short fcm token",
            Box::new(|a: &mut Args| {
                a.platform = WakePlatform::Fcm;
                a.environment = WakeEnvironment::Production;
                a.push_token = "a".repeat(99);
            }),
        ),
        (
            "uppercase key",
            Box::new(|a: &mut Args| a.encryption_key = a.encryption_key.to_uppercase()),
        ),
        (
            "off-curve key",
            Box::new(|a: &mut Args| a.encryption_key = format!("02{}", "ff".repeat(32))),
        ),
        (
            "uppercase secret hash",
            Box::new(|a: &mut Args| a.secret_sha256 = a.secret_sha256.to_uppercase()),
        ),
        ("no identity", Box::new(|a: &mut Args| a.identities.clear())),
        ("two ln", {
            let (ln, other_ln) = (ln.clone(), other_ln.clone());
            Box::new(move |a: &mut Args| a.identities = vec![ln.clone(), other_ln.clone()])
        }),
        ("duplicate", {
            let pk = pk.clone();
            Box::new(move |a: &mut Args| a.identities = vec![pk.clone(), pk.clone()])
        }),
        ("three", {
            let (ln, pk, other_ln) = (ln.clone(), pk.clone(), other_ln.clone());
            Box::new(move |a: &mut Args| {
                a.identities = vec![ln.clone(), pk.clone(), other_ln.clone()]
            })
        }),
        ("pubky prefix", {
            let pk = pk.clone();
            Box::new(move |a: &mut Args| a.identities = vec![pk.replacen("pk:", "pk:pubky", 1)])
        }),
        ("uppercase pk", {
            let pk = pk.clone();
            Box::new(move |a: &mut Args| {
                a.identities = vec![format!("pk:{}", pk[3..].to_uppercase())]
            })
        }),
        ("non-canonical pk", {
            let pk = non_canonical_pk.clone();
            Box::new(move |a: &mut Args| a.identities = vec![pk.clone()])
        }),
        (
            "weak pk",
            Box::new(|a: &mut Args| a.identities = vec![format!("pk:{}", "y".repeat(52))]),
        ),
        ("uppercase ln", {
            let ln = ln.clone();
            Box::new(move |a: &mut Args| {
                a.identities = vec![format!("ln:{}", ln[3..].to_uppercase())]
            })
        }),
        (
            "off-curve ln",
            Box::new(|a: &mut Args| a.identities = vec![format!("ln:02{}", "ff".repeat(32))]),
        ),
        ("bare key", {
            let ln = ln.clone();
            Box::new(move |a: &mut Args| a.identities = vec![ln[3..].to_string()])
        }),
        (
            "33 topics",
            Box::new(|a: &mut Args| a.topics = (0..33).map(|i| format!("t.t{i}")).collect()),
        ),
        (
            "no dot",
            Box::new(|a: &mut Args| a.topics = vec!["blocktank".to_string()]),
        ),
        (
            "uppercase namespace",
            Box::new(|a: &mut Args| a.topics = vec!["Blocktank.x".to_string()]),
        ),
        (
            "bare star",
            Box::new(|a: &mut Args| a.topics = vec!["*".to_string()]),
        ),
        (
            "long",
            Box::new(|a: &mut Args| a.topics = vec![format!("a.{}", "b".repeat(63))]),
        ),
    ];
    for (name, tweak) in cases {
        let mut args = Args::from_vector();
        tweak(&mut args);
        let result = args.prepare();
        assert!(
            matches!(
                result,
                Err(WakeError::InvalidInput { .. } | WakeError::InvalidKey { .. })
            ),
            "{name}: {result:?}"
        );
    }

    let mut fcm = Args::from_vector();
    fcm.platform = WakePlatform::Fcm;
    fcm.environment = WakeEnvironment::Production;
    fcm.push_token = "a:b_c-d".repeat(15);
    fcm.identities = vec![pk.clone()];
    fcm.topics = vec!["blocktank.incomingHtlc".to_string(), "paykit.*".to_string()];
    let prepared = fcm.prepare().unwrap();
    assert!(prepared
        .message
        .contains("\nplatform:fcm\nenvironment:production\n"));
    assert!(prepared.message.contains(&format!("\nidentities:{pk}\n")));
    let mut no_topics = Args::from_vector();
    no_topics.topics.clear();
    assert!(no_topics.prepare().is_ok());
}

#[test]
fn prepare_defaults_the_timestamp_to_now() {
    let v = load(REGISTRATION_V1);
    let mut request = v["request"].clone();
    request["ts"] = Value::Null;
    let prepared = vector_registration(&json!({"audience": v["audience"], "request": request}));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(prepared.timestamp.abs_diff(now) <= 5);
    assert!(prepared
        .message
        .ends_with(&format!("\nts:{}", prepared.timestamp)));
}

#[test]
fn pubky_proofs_only_sign_registration_messages() {
    let v = load(REGISTRATION_V1);
    for message in ["", "wake-register-v1", "wake-register-v1x\n", "hello"] {
        assert_eq!(
            wake_sign_pubky_proof(s(&v, "pk_seed").to_string(), message.to_string()),
            Err(invalid_input(
                "only wake registration messages can be signed"
            )),
            "{message:?}"
        );
    }
    for key in ["", "44", &"zz".repeat(32)] {
        assert!(matches!(
            wake_sign_pubky_proof(key.to_string(), s(&v, "preimage").to_string()),
            Err(WakeError::InvalidKey { .. })
        ));
    }
    let signature = wake_sign_pubky_proof(
        s(&v, "pk_seed").to_string(),
        "wake-register-v1\n".to_string(),
    )
    .unwrap();
    assert_eq!(signature.len(), 128);
}

// ----- Credentials and keys -----

#[test]
fn device_secret_format() {
    let secret = device_secret_from_bytes(&[0x77; 32]);
    assert_eq!(secret.token, DEVICE_SECRET);
    assert_eq!(
        secret.sha256_hex,
        "a4908532a2f65068ce0b3498af2b68e43949be33aa85840288e653295bf52080"
    );
    assert!(is_valid_device_secret(&secret.token));
    assert!(!format!("{secret:?}").contains("d3d3"));

    let generated = wake_generate_device_secret().unwrap();
    assert!(is_valid_device_secret(&generated.token));
    assert_ne!(generated.token, secret.token);
    assert_eq!(
        generated.sha256_hex,
        hex::encode(sha256::Hash::hash(generated.token.as_bytes()).to_byte_array())
    );
}

#[test]
fn device_secret_rejects_malformed() {
    let body = &DEVICE_SECRET[4..];
    for bad in [
        format!("wkp_{body}"),
        body.to_string(),
        format!("wkd_{}", &body[..42]),
        format!("wkd_{body}A"),
        format!("wkd_{}d", &body[..42]),
        format!("wkd_{}=", &body[..42]),
        format!("wkd_{}+", &body[..42]),
    ] {
        assert!(!is_valid_device_secret(&bad), "{bad}");
    }
}

#[test]
fn install_ids_are_22_base64url_characters() {
    assert_eq!(install_id_from_bytes(&[0xfb; 16]), "-_v7-_v7-_v7-_v7-_v7-w");
    let generated = wake_generate_install_id();
    assert_eq!(generated.len(), 22);
    assert!(generated
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    assert_ne!(generated, wake_generate_install_id());
}

#[test]
fn keypair_generation_retries_out_of_range_draws() {
    // Zero and n..2^256 are not scalars; the third draw is.
    let mut draws = 0;
    let keypair = generate_keypair_with(|buf: &mut [u8]| {
        draws += 1;
        buf.fill(match draws {
            1 => 0,
            2 => 0xff,
            _ => 0x11,
        });
        Ok(())
    })
    .unwrap();
    assert_eq!(keypair.secret_key_hex, "11".repeat(32));
    assert_eq!(
        keypair.public_key_hex,
        s(&load(V0_SERVER), "ephemeral_public_key")
    );
    assert!(!format!("{keypair:?}").contains(&"11".repeat(32)));
}

#[test]
fn constant_invalid_entropy_fails_instead_of_hanging() {
    for byte in [0x00, 0xff] {
        let mut draws = 0;
        let result = generate_keypair_with(|buf: &mut [u8]| {
            draws += 1;
            buf.fill(byte);
            Ok(())
        });
        assert_eq!(
            result,
            Err(WakeError::InvalidKey {
                reason: "entropy source is degenerate".to_string()
            })
        );
        assert_eq!(draws, 64);
    }
}

#[test]
fn generated_keypairs_decrypt_what_is_sealed_to_them() {
    let keypair = wake_generate_keypair().unwrap();
    assert_eq!(keypair.secret_key_hex.len(), 64);
    assert_eq!(keypair.public_key_hex.len(), 66);
    assert_ne!(keypair, wake_generate_keypair().unwrap());
    let plaintext = format!(
        r#"{{"v":1,"id":"{WAKE_ID}","topic":"a.b","urgency":"alert","deadline":9,"payload":[1]}}"#
    );
    let container = seal_deterministic(
        Scheme::V1,
        &bytes33(&keypair.public_key_hex),
        plaintext.as_bytes(),
        &parse_secret_key(&"66".repeat(32)).unwrap(),
        &[1; 12],
    );
    let envelope = wake_decrypt_v1(keypair.secret_key_hex, container.to_json()).unwrap();
    assert_eq!(envelope.payload_json.as_deref(), Some("[1]"));
}

// ----- Client: builders -----

fn body_json(request: &HttpRequest) -> Value {
    serde_json::from_str(request.body.as_deref().unwrap()).unwrap()
}

#[test]
fn builders_for_every_route() {
    let topics = vec!["blocktank.*".to_string()];
    let routes = [
        (
            build_server_info(GATEWAY).unwrap(),
            Method::GET,
            "v1/info",
            false,
            None,
        ),
        (
            build_list_topics(GATEWAY).unwrap(),
            Method::GET,
            "v1/topics",
            false,
            None,
        ),
        (
            build_ack(GATEWAY, DEVICE_SECRET, WAKE_ID, WakeAckOutcome::NoHandler).unwrap(),
            Method::POST,
            "v1/acks",
            true,
            Some(json!({"id": WAKE_ID, "outcome": "no_handler"})),
        ),
        (
            build_set_presence(GATEWAY, DEVICE_SECRET, 60).unwrap(),
            Method::PUT,
            "v1/presence",
            true,
            Some(json!({"ttl_secs": 60})),
        ),
        (
            build_clear_presence(GATEWAY, DEVICE_SECRET).unwrap(),
            Method::DELETE,
            "v1/presence",
            true,
            None,
        ),
        (
            build_set_topics(GATEWAY, DEVICE_SECRET, &topics).unwrap(),
            Method::PUT,
            "v1/devices/self/topics",
            true,
            Some(json!({"topics": ["blocktank.*"]})),
        ),
        (
            build_unregister(GATEWAY, DEVICE_SECRET).unwrap(),
            Method::DELETE,
            "v1/devices/self",
            true,
            None,
        ),
    ];
    for (request, method, route, authenticated, body) in routes {
        assert_eq!(request.method, method, "{route}");
        assert_eq!(request.url.as_str(), format!("{GATEWAY}/{route}"));
        assert_eq!(
            request.bearer.as_deref(),
            authenticated.then_some(DEVICE_SECRET),
            "{route}"
        );
        match body {
            Some(expected) => assert_eq!(body_json(&request), expected, "{route}"),
            None => assert!(request.body.is_none(), "{route}"),
        }
    }
    for (outcome, wire) in [
        (WakeAckOutcome::Handled, "handled"),
        (WakeAckOutcome::NoHandler, "no_handler"),
        (WakeAckOutcome::Failed, "failed"),
    ] {
        let request = build_ack(GATEWAY, DEVICE_SECRET, WAKE_ID, outcome).unwrap();
        assert_eq!(body_json(&request)["outcome"], wire);
    }
}

#[test]
fn gateway_urls_keep_their_path_prefix() {
    for (base, expected) in [
        ("https://wake.example", "https://wake.example/v1/info"),
        ("https://wake.example/", "https://wake.example/v1/info"),
        (
            "https://api.example/wake",
            "https://api.example/wake/v1/info",
        ),
        (
            "https://api.example/wake/",
            "https://api.example/wake/v1/info",
        ),
        (
            "http://127.0.0.1:9010?x=1#y",
            "http://127.0.0.1:9010/v1/info",
        ),
    ] {
        assert_eq!(build_server_info(base).unwrap().url.as_str(), expected);
    }
    for bad in ["", "wake.example", "ftp://wake.example", "file:///tmp/x"] {
        assert!(matches!(
            build_server_info(bad),
            Err(WakeError::InvalidInput { .. })
        ));
    }
}

#[test]
fn builders_validate_secrets_and_wake_ids() {
    let malformed = "malformed device secret";
    for bad in ["", "wkd_", "Bearer x", &format!("{DEVICE_SECRET}\r\nX: y")] {
        assert_eq!(
            build_clear_presence(GATEWAY, bad).err(),
            Some(invalid_input(malformed))
        );
    }
    assert_eq!(
        build_ack(GATEWAY, DEVICE_SECRET, "nope", WakeAckOutcome::Handled).err(),
        Some(invalid_input("wake id is not a uuid"))
    );
    let upper = build_ack(
        GATEWAY,
        DEVICE_SECRET,
        &WAKE_ID.to_uppercase(),
        WakeAckOutcome::Handled,
    )
    .unwrap();
    assert_eq!(body_json(&upper)["id"], WAKE_ID);
}

// ----- Client: parsers -----

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.to_string(),
    }
}

fn rejected(status: u16, code: &str, detail: &str) -> WakeError {
    WakeError::GatewayRejected {
        status,
        code: code.to_string(),
        detail: detail.to_string(),
    }
}

#[test]
fn parsers_for_every_route() {
    assert_eq!(
        parse_server_info(&response(
            200,
            r#"{"audience":"wake.localhost","server_time":1790000000,"version":"0.1.0"}"#
        ))
        .unwrap(),
        WakeServerInfo {
            audience: "wake.localhost".to_string(),
            server_time: 1790000000,
            version: "0.1.0".to_string(),
        }
    );
    let registered = r#"{"device_id":"01a0c450-6c00-7000-8000-0000000000aa","identities":["ln:02"],"topics":["blocktank.*"],"unknown_topics":["x.y"],"server_time":5}"#;
    for status in [200, 201] {
        assert_eq!(
            parse_register(&response(status, registered)).unwrap(),
            WakeRegistration {
                device_id: "01a0c450-6c00-7000-8000-0000000000aa".to_string(),
                identities: vec!["ln:02".to_string()],
                topics: vec!["blocktank.*".to_string()],
                unknown_topics: vec!["x.y".to_string()],
                server_time: 5,
            }
        );
    }
    assert_eq!(
        parse_set_presence(&response(200, r#"{"expires_at":1790000060}"#)).unwrap(),
        1790000060
    );
    assert_eq!(
        parse_set_topics(&response(
            200,
            r#"{"topics":["blocktank.*"],"unknown_topics":["x.y"]}"#
        ))
        .unwrap(),
        vec!["blocktank.*".to_string()]
    );
    assert_eq!(parse_no_content(&response(204, "")), Ok(()));

    let topics = r#"{"topics":[
        {"name":"blocktank.incomingHtlc","description":"An LSP holds an HTLC","owner":"blocktank",
         "apps":["bitkit"],"urgency":"background","deadline_secs":60,"presence":"ignore","peer":false,
         "alertable":true,"display":{"title":"Incoming Payment","body":"Open Bitkit"},
         "fallback":{"window_secs":45},"max_payload_bytes":512},
        {"name":"paykit.sync","description":"d","owner":"peer","apps":["bitkit"],"urgency":"background",
         "deadline_secs":600,"presence":"only_if_offline","peer":true,"alertable":false,"display":null,
         "fallback":null,"max_payload_bytes":0}]}"#;
    assert_eq!(
        parse_list_topics(&response(200, topics)).unwrap(),
        vec![
            WakeTopic {
                name: "blocktank.incomingHtlc".to_string(),
                description: "An LSP holds an HTLC".to_string(),
                urgency: "background".to_string(),
                deadline_secs: 60,
                alertable: true,
                title: Some("Incoming Payment".to_string()),
                body: Some("Open Bitkit".to_string()),
                peer: false,
            },
            WakeTopic {
                name: "paykit.sync".to_string(),
                description: "d".to_string(),
                urgency: "background".to_string(),
                deadline_secs: 600,
                alertable: false,
                title: None,
                body: None,
                peer: true,
            },
        ]
    );
}

#[test]
fn error_statuses_map_to_gateway_rejected() {
    let unauthorized = response(401, r#"{"error":"unauthorized","message":"bad secret"}"#);
    let unknown_wake = response(404, r#"{"error":"unknown_wake","message":"no delivery"}"#);
    let stale = response(
        409,
        r#"{"error":"stale_registration","message":"older than the stored one"}"#,
    );
    let stale_ts = response(
        401,
        r#"{"error":"stale_timestamp","message":"skew","server_time":1790000000}"#,
    );
    assert_eq!(
        parse_no_content(&unauthorized),
        Err(rejected(401, "unauthorized", "bad secret"))
    );
    assert_eq!(
        parse_no_content(&unknown_wake),
        Err(rejected(404, "unknown_wake", "no delivery"))
    );
    assert_eq!(
        parse_register(&stale),
        Err(rejected(
            409,
            "stale_registration",
            "older than the stored one"
        ))
    );
    assert_eq!(
        parse_register(&stale_ts),
        Err(rejected(401, "stale_timestamp", "skew"))
    );
    assert_eq!(
        parse_set_presence(&unauthorized),
        Err(rejected(401, "unauthorized", "bad secret"))
    );
    assert_eq!(
        parse_set_topics(&unauthorized),
        Err(rejected(401, "unauthorized", "bad secret"))
    );
    assert_eq!(
        parse_server_info(&response(503, r#"{"error":"unavailable","message":"db"}"#)),
        Err(rejected(503, "unavailable", "db"))
    );
    let no_body = rejected(502, "unknown", "the response carries no wake error body");
    for body in ["<html>Bad Gateway</html>", "", r#"{"error":7}"#] {
        assert_eq!(
            parse_list_topics(&response(502, body)),
            Err(no_body.clone())
        );
    }
    assert_eq!(
        parse_no_content(&response(302, "")),
        Err(rejected(
            302,
            "unknown",
            "the response carries no wake error body"
        ))
    );
}

#[test]
fn malformed_success_bodies_are_invalid_responses() {
    for body in [
        "",
        "<html>",
        r#"{"audience":"a"}"#,
        r#"{"expires_at":"soon"}"#,
    ] {
        assert!(matches!(
            parse_server_info(&response(200, body)),
            Err(WakeError::InvalidResponse { .. })
        ));
        let err = parse_set_presence(&response(200, body)).unwrap_err();
        assert!(matches!(err, WakeError::InvalidResponse { .. }));
        assert!(!err.to_string().contains("soon"), "{err}");
    }
}

// ----- Client: executor, against a local server -----

/// Serves one canned response on 127.0.0.1 and returns what it received.
async fn serve_once(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/prefix", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut received = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            received.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&received).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if received.len() >= end + 4 + length || n == 0 {
                    break;
                }
            }
        }
        let location = if (300..400).contains(&status) {
            "Location: /moved\r\n"
        } else {
            ""
        };
        let reply = format!(
            "HTTP/1.1 {status} X\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
        String::from_utf8(received).unwrap()
    });
    (url, handle)
}

#[tokio::test]
async fn executor_sends_the_built_request() {
    let (url, server) = serve_once(200, r#"{"expires_at":1790000060}"#).await;
    let expires_at = wake_set_presence(url, DEVICE_SECRET.to_string(), 90)
        .await
        .unwrap();
    assert_eq!(expires_at, 1790000060);
    let received = server.await.unwrap();
    let lower = received.to_ascii_lowercase();
    assert!(
        received.starts_with("PUT /prefix/v1/presence HTTP/1.1\r\n"),
        "{received}"
    );
    assert!(lower.contains(&format!(
        "authorization: bearer {}",
        DEVICE_SECRET.to_ascii_lowercase()
    )));
    assert!(lower.contains("content-type: application/json"));
    assert!(lower.contains("accept: application/json"));
    assert!(
        received.ends_with("\r\n\r\n{\"ttl_secs\":90}"),
        "{received}"
    );
}

#[tokio::test]
async fn executor_maps_statuses_and_network_failures() {
    let (url, server) = serve_once(404, r#"{"error":"unknown_wake","message":"m"}"#).await;
    let result = wake_ack(
        url,
        DEVICE_SECRET.to_string(),
        WAKE_ID.to_string(),
        WakeAckOutcome::Handled,
    )
    .await;
    assert_eq!(result, Err(rejected(404, "unknown_wake", "m")));
    let received = server.await.unwrap();
    assert!(received.starts_with("POST /prefix/v1/acks HTTP/1.1\r\n"));
    assert!(received.ends_with(&format!("{{\"id\":\"{WAKE_ID}\",\"outcome\":\"handled\"}}")));

    let (url, server) = serve_once(204, "").await;
    assert_eq!(
        wake_unregister(url, DEVICE_SECRET.to_string()).await,
        Ok(())
    );
    assert!(server
        .await
        .unwrap()
        .starts_with("DELETE /prefix/v1/devices/self HTTP/1.1\r\n"));

    // Redirects are not followed.
    let (url, server) = serve_once(307, "").await;
    assert_eq!(
        wake_clear_presence(url, DEVICE_SECRET.to_string()).await,
        Err(rejected(
            307,
            "unknown",
            "the response carries no wake error body"
        ))
    );
    server.await.unwrap();

    // Nothing listens on a port that was just released.
    let port = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let result = wake_server_info(format!("http://127.0.0.1:{port}")).await;
    assert!(
        matches!(result, Err(WakeError::RequestFailed { .. })),
        "{result:?}"
    );
}

// ----- Live gateway -----

/// Registers against a running gateway with both proofs, then exercises
/// presence and topics. Run with
/// `WAKE_GATEWAY_URL=http://127.0.0.1:9010 cargo test wake -- --ignored`.
#[tokio::test]
#[ignore]
async fn live_gateway_round_trip() {
    let url = std::env::var("WAKE_GATEWAY_URL").expect("WAKE_GATEWAY_URL is not set");
    let info = wake_server_info(url.clone()).await.unwrap();

    let encryption = wake_generate_keypair().unwrap();
    let device_secret = wake_generate_device_secret().unwrap();
    let ln_secret = parse_secret_key(&wake_generate_keypair().unwrap().secret_key_hex).unwrap();
    let ln_identity = format!("ln:{}", public_hex(&ln_secret));
    let pk_secret_hex = wake_generate_keypair().unwrap().secret_key_hex;
    let pk_seed: [u8; 32] = hex::decode(&pk_secret_hex).unwrap().try_into().unwrap();
    let pk_identity = format!(
        "pk:{}",
        pubky::Keypair::from_secret(&pk_seed).public_key().z32()
    );
    let mut push_token = [0u8; 32];
    os_random(&mut push_token).unwrap();

    let request = wake_prepare_registration(
        info.audience.clone(),
        "bitkit".to_string(),
        wake_generate_install_id(),
        WakePlatform::Apns,
        WakeEnvironment::Sandbox,
        hex::encode(push_token),
        encryption.public_key_hex.clone(),
        device_secret.sha256_hex.clone(),
        vec![ln_identity.clone(), pk_identity.clone()],
        vec!["blocktank.*".to_string()],
        None,
    )
    .unwrap();
    let proofs = vec![
        WakeIdentityProof {
            identity: ln_identity.clone(),
            signature: lightning_sign(&ln_secret, request.message.as_bytes()),
        },
        WakeIdentityProof {
            identity: pk_identity.clone(),
            signature: wake_sign_pubky_proof(pk_secret_hex, request.message.clone()).unwrap(),
        },
    ];
    let registration = wake_register(url.clone(), request, proofs).await.unwrap();
    let mut identities = registration.identities.clone();
    identities.sort();
    let mut expected = vec![ln_identity, pk_identity];
    expected.sort();
    assert_eq!(identities, expected);
    assert_eq!(registration.topics, vec!["blocktank.*".to_string()]);

    let token = device_secret.token.clone();
    let expires_at = wake_set_presence(url.clone(), token.clone(), 60)
        .await
        .unwrap();
    assert!(expires_at > info.server_time);
    wake_clear_presence(url.clone(), token.clone())
        .await
        .unwrap();

    let topics = wake_list_topics(url.clone()).await.unwrap();
    assert!(!topics.is_empty());
    let stored = wake_set_topics(
        url.clone(),
        token.clone(),
        vec!["blocktank.*".to_string(), "unknown.topicName".to_string()],
    )
    .await
    .unwrap();
    assert_eq!(stored, vec!["blocktank.*".to_string()]);

    wake_unregister(url.clone(), token.clone()).await.unwrap();
    assert!(matches!(
        wake_set_presence(url, token, 60).await,
        Err(WakeError::GatewayRejected { status: 401, .. })
    ));
}
