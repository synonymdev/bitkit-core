//! Redaction of Trezor transport debug output.
//!
//! `TransportCallback::log_debug` forwards diagnostics produced by
//! trezor-connect-rs to the native apps. That stream is owned by the
//! dependency and its exact contents change on every bump, so bitkit-core
//! cannot assume it is free of key material — and consumer-side regex
//! scrubbing is not a security boundary (see synonymdev/bitkit-android#1067).
//!
//! Everything crossing the FFI boundary therefore goes through
//! [`sanitize_debug_log`] first. The policy is deliberately conservative:
//! anything that looks like a secret is replaced with a stable `<redacted>`
//! placeholder, and only values that are provably harmless (booleans, counts,
//! byte lengths, `None`) survive under a sensitive label. Diagnostics that
//! carry no value at all — subsystem tags, state names, error strings — pass
//! through untouched.

use lazy_regex::{lazy_regex, Lazy};
use regex::{Captures, Regex};

/// Placeholder substituted for any redacted value.
const REDACTED: &str = "<redacted>";

/// Maximum length of a forwarded tag, in characters.
const MAX_TAG_CHARS: usize = 32;

/// Maximum length of a forwarded message, in characters.
///
/// Caps the per-chunk BLE spam that otherwise floods the native debug buffer,
/// and bounds anything the passes below failed to recognise as a secret.
const MAX_MESSAGE_CHARS: usize = 512;

/// Key fragments whose value is never forwarded, whatever it looks like.
///
/// A PIN and a pairing code are short integers, so the "counts are harmless"
/// exemption below must not apply to them.
const ALWAYS_SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "entropy",
    "mnemonic",
    "pairingcode",
    "passphrase",
    "password",
    "pin",
    "privkey",
    "secret",
    "seed",
];

/// Key fragments whose value is forwarded only when it is provably harmless
/// (see [`is_harmless_value`]) — these labels are usually attached to sizes
/// and flags worth keeping, such as `has_credentials=true`.
const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "ciphertext",
    "credential",
    "key",
    "nonce",
    "payload",
    "plaintext",
    "psbt",
    "salt",
    "session",
    "signature",
    "token",
    "transaction",
    "xprv",
    "xpub",
];

/// Sanitize a `(tag, message)` pair before it is handed to the native
/// `log_debug` callback.
///
/// Returns owned strings ready for the FFI call.
pub fn sanitize_debug_log(tag: &str, message: &str) -> (String, String) {
    (
        truncate(&sanitize(tag), MAX_TAG_CHARS),
        truncate(&sanitize(message), MAX_MESSAGE_CHARS),
    )
}

/// Apply every redaction pass to a single line of debug output.
fn sanitize(text: &str) -> String {
    let text = redact_labeled_values(text);
    redact_bare_secrets(&text)
}

/// Redact `key=value`, `key: value` and `"key": value` pairs whose key names a
/// secret, unless the value is self-evidently harmless.
fn redact_labeled_values(text: &str) -> String {
    static LABELED_VALUE: Lazy<Regex> = lazy_regex!(
        r#"(?x)
        (?P<key>[A-Za-z_][A-Za-z0-9_.\-]*)  # label, optionally JSON-quoted
        "?
        \s*[:=]\s*
        (?P<value>
              "[^"]*"                       # quoted string
            | \[[^\]]*\]                    # array
            | \{[^}]*\}                     # object
            | [^\s,;)\]}"]+                 # bare token
        )"#
    );

    LABELED_VALUE
        .replace_all(text, |caps: &Captures| {
            let whole = &caps[0];
            let key = &caps["key"];
            let value = &caps["value"];
            let separator = &whole[key.len()..whole.len() - value.len()];

            if is_always_sensitive_key(key) {
                return format!("{}{}{}", key, separator, placeholder_for(value));
            }
            if !is_sensitive_key(key) || is_harmless_value(value) {
                return whole.to_string();
            }
            // `credential: host_key=32bytes` — the captured "value" is itself a
            // labeled pair, so descend into it instead of blanking the lot.
            if is_labeled_pair(value) {
                return format!("{}{}{}", key, separator, redact_labeled_values(value));
            }
            format!("{}{}{}", key, separator, placeholder_for(value))
        })
        .into_owned()
}

/// Redact secrets that carry no label at all: extended keys, base64 PSBTs,
/// long base64 blobs and long hex runs (serialized transactions, raw frames).
fn redact_bare_secrets(text: &str) -> String {
    // A base64-encoded PSBT always starts with the `psbt\xff` magic.
    static PSBT: Lazy<Regex> = lazy_regex!(r"\bcHNidP[A-Za-z0-9+/]+=*");
    // xpub/xprv and the ypub/zpub/tpub/upub/vpub variants, mainnet or testnet.
    static EXTENDED_KEY: Lazy<Regex> =
        lazy_regex!(r"\b[xyztuvXYZTUV](?:pub|prv)[1-9A-HJ-NP-Za-km-z]{40,}");
    // 16 bytes or more of contiguous hex: frame payloads, txids, serialized txs.
    static LONG_HEX: Lazy<Regex> = lazy_regex!(r"\b[0-9a-fA-F]{32,}\b");
    // Any other long unbroken base64 run — serialized credentials and the like.
    static LONG_BASE64: Lazy<Regex> = lazy_regex!(r"[A-Za-z0-9+/]{64,}=*");

    let text = PSBT.replace_all(text, REDACTED);
    let text = EXTENDED_KEY.replace_all(&text, REDACTED);
    let text = LONG_HEX.replace_all(&text, REDACTED);
    LONG_BASE64.replace_all(&text, REDACTED).into_owned()
}

/// Whether a label suggests its value is key material.
fn is_sensitive_key(key: &str) -> bool {
    matches_fragment(key, SENSITIVE_KEY_FRAGMENTS) || is_always_sensitive_key(key)
}

/// Whether a label is one whose value is redacted unconditionally.
fn is_always_sensitive_key(key: &str) -> bool {
    matches_fragment(key, ALWAYS_SENSITIVE_KEY_FRAGMENTS)
}

/// Fold a label down to its letters and digits, then look for any fragment in
/// it — so `host_static_key`, `hostStaticKey` and `"host-static-key"` all hit
/// `key`. Over-matching here only costs diagnostic detail; under-matching leaks.
fn matches_fragment(key: &str, fragments: &[&str]) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    fragments
        .iter()
        .any(|fragment| normalized.contains(fragment))
}

/// Whether a captured value is itself a `key=value` pair.
fn is_labeled_pair(value: &str) -> bool {
    static LABELED_PAIR: Lazy<Regex> = lazy_regex!(r#"^[A-Za-z_][A-Za-z0-9_.\-]*"?\s*[:=]"#);
    LABELED_PAIR.is_match(value)
}

/// Whether a value is safe to forward even under a sensitive label.
///
/// Only counts, sizes, booleans and absence qualify — these are the
/// diagnostics worth keeping (`has_credentials=true`, `payload: 48 bytes`).
fn is_harmless_value(value: &str) -> bool {
    static COUNT_OR_SIZE: Lazy<Regex> =
        lazy_regex!(r"(?i)^-?[0-9]+\s*(b|kb|mb|bit|bits|byte|bytes|char|chars|ms|s)?$");

    matches!(
        value.to_ascii_lowercase().as_str(),
        "true" | "false" | "none" | "null" | "nil" | "n/a" | "unknown" | r#""""# | "[]" | "{}"
    ) || COUNT_OR_SIZE.is_match(value)
}

/// Build a placeholder that preserves the shape of the value it replaces, so
/// quoted fields stay quoted and arrays stay arrays.
fn placeholder_for(value: &str) -> String {
    match value.as_bytes().first() {
        Some(b'"') => format!("\"{}\"", REDACTED),
        Some(b'[') => format!("[{}]", REDACTED),
        Some(b'{') => format!("{{{}}}", REDACTED),
        _ => REDACTED.to_string(),
    }
}

/// Truncate to `max_chars` on a character boundary, marking the cut.
fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => format!("{}…<truncated>", &text[..byte_index]),
        None => text.to_string(),
    }
}
