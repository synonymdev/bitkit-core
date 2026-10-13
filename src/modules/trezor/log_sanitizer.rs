//! Redaction of Trezor transport debug output.
//!
//! `TransportCallback::log_debug` forwards diagnostics produced by
//! trezor-connect-rs. That stream belongs to the dependency and its contents
//! change on every bump, so it cannot be assumed free of key material, and
//! consumer-side scrubbing is not a security boundary (see
//! synonymdev/bitkit-android#1067).
//!
//! [`sanitize_debug_log`] therefore runs over everything before it crosses the
//! FFI boundary. Anything resembling a secret becomes `<redacted>`; only
//! provably harmless values (booleans, counts, byte lengths, `None`) survive
//! under a sensitive label. Value-free diagnostics such as tags, state names
//! and error strings pass through untouched.

use lazy_regex::{lazy_regex, Lazy};
use regex::Regex;

const REDACTED: &str = "<redacted>";

const MAX_TAG_CHARS: usize = 32;

/// Caps the per-chunk BLE spam that floods the native debug buffer, and bounds
/// anything the passes below fail to recognise as a secret.
const MAX_MESSAGE_CHARS: usize = 512;

/// How far redaction descends into nested values before it stops looking and
/// blanks the rest. Real diagnostics nest a level or two; a long chain of them
/// is only ever a way to drive this module off the stack.
const MAX_NESTING_DEPTH: usize = 8;

/// Key fragments whose value is never forwarded, whatever it looks like: a PIN
/// or pairing code is a short integer, so the count exemption must not reach it.
const ALWAYS_SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "entropy",
    "mnemonic",
    "pairingcode",
    "passphrase",
    "password",
    "pin",
    "privkey",
    "recovery",
    "secret",
    "seed",
];

/// Key fragments that make a bare integer provably a count or a size rather
/// than a numeric token.
const COUNT_KEY_FRAGMENTS: &[&str] = &["count", "index", "len", "num", "offset", "size"];

/// Key fragments whose value is forwarded only when [`is_harmless_value`]
/// clears it, keeping diagnostics like `has_credentials=true`.
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
/// Truncation runs first so the redaction passes only ever see a bounded input.
pub fn sanitize_debug_log(tag: &str, message: &str) -> (String, String) {
    (
        sanitize(&truncate(tag, MAX_TAG_CHARS)),
        sanitize(&truncate(message, MAX_MESSAGE_CHARS)),
    )
}

fn sanitize(text: &str) -> String {
    redact_bare_secrets(&redact_labeled_values(text, 0))
}

/// Redact `key=value`, `key: value` and `"key": value` pairs whose key names a
/// secret, unless the value is self-evidently harmless.
fn redact_labeled_values(text: &str, depth: usize) -> String {
    static LABELED_VALUE: Lazy<Regex> = lazy_regex!(
        r#"(?x)
        (?P<key>[A-Za-z_][A-Za-z0-9_.\-]*)  # label, optionally JSON-quoted
        "?
        \s*[:=]\s*
        (?P<value>
              "(?:[^"\\]|\\.)*"             # quoted string, escapes included
            | "(?:[^"\\]|\\.)*              # ... or one upstream left unclosed
            | \[[^\]]*\]                    # array
            | \{[^}]*\}                     # object
                                            # count with a spaced-out unit
            | -?[0-9]+\s*(?i:bytes|byte|bits|bit|chars|char|kb|mb|ms|b|s)\b
            | [^\s,;)\]}"]+                 # bare token
        )"#
    );

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;

    for caps in LABELED_VALUE.captures_iter(text) {
        let whole = caps.get(0).expect("group 0 always matches");
        if whole.start() < cursor {
            continue; // swallowed by the multi-word redaction of an earlier pair
        }
        let key = &caps["key"];
        let value = &caps["value"];
        let separator = &whole.as_str()[key.len()..whole.len() - value.len()];
        // `seed phrase: ...` is one label written as two words, and the pattern
        // above can only capture the last of them.
        let label = format!("{}{}", preceding_words(text, whole.start()), key);

        out.push_str(&text[cursor..whole.start()]);
        cursor = whole.end();

        if !is_always_sensitive_key(&label) {
            if !is_sensitive_key(&label) {
                // Innocuous label, but the value can still nest a pair that is
                // not: `context={"token": "hunter2"}`.
                out.push_str(key);
                out.push_str(separator);
                out.push_str(&redact_nested_values(value, depth));
                continue;
            }
            if is_harmless_value(&label, value) {
                out.push_str(whole.as_str());
                continue;
            }
            if depth < MAX_NESTING_DEPTH && is_sensitive_labeled_pair(value) {
                // `credential: host_key=32bytes`, so descend into the inner
                // pair instead of blanking the lot.
                out.push_str(key);
                out.push_str(separator);
                out.push_str(&redact_labeled_values(value, depth + 1));
                continue;
            }
        }

        out.push_str(key);
        out.push_str(separator);
        out.push_str(&placeholder_for(value));
        cursor = end_of_multiword_value(text, value, cursor);
    }

    out.push_str(&text[cursor..]);
    out
}

/// The one or two plain words written immediately before a label. Anything but
/// a bare word ends the run, so only words reading as part of the label count.
fn preceding_words(text: &str, start: usize) -> &str {
    static PRECEDING_WORDS: Lazy<Regex> = lazy_regex!(r"(?:[A-Za-z_][A-Za-z0-9_.\-]*[\t ]+){1,2}$");
    PRECEDING_WORDS
        .find(&text[..start])
        .map_or("", |words| words.as_str())
}

/// Redact inside the value of a label that is not itself sensitive: the parser
/// above sees flat text and would otherwise forward a nested `{"token": ...}`
/// whole. Past [`MAX_NESTING_DEPTH`] the value is blanked instead of descended
/// into, so an adversarial chain of pairs cannot exhaust the stack.
fn redact_nested_values(value: &str, depth: usize) -> String {
    static LABELED_PAIR: Lazy<Regex> = lazy_regex!(r#"^[A-Za-z_][A-Za-z0-9_.\-]*"?\s*[:=]\s*\S"#);

    if depth >= MAX_NESTING_DEPTH {
        return placeholder_for(value);
    }
    match delimiters(value) {
        Some((open, close)) => format!(
            "{}{}{}",
            open,
            redact_labeled_values(&value[1..value.len() - 1], depth + 1),
            close
        ),
        None if LABELED_PAIR.is_match(value) => redact_labeled_values(value, depth + 1),
        None => value.to_string(),
    }
}

/// The delimiter pair enclosing a value, if it is enclosed at all. An unclosed
/// quote is not, and must not have its last character mistaken for one.
fn delimiters(value: &str) -> Option<(char, char)> {
    let bytes = value.as_bytes();
    let (open, close) = match bytes.first()? {
        b'"' => ('"', '"'),
        b'[' => ('[', ']'),
        b'{' => ('{', '}'),
        _ => return None,
    };
    (value.len() >= 2 && bytes[value.len() - 1] == close as u8).then_some((open, close))
}

/// End of a redacted value, extended past the words the capture left behind.
///
/// An unquoted secret can be several words long (`mnemonic=abandon ability
/// ...`) and the value pattern stops at the first space. Trailing words are
/// absorbed up to the next delimiter or `key=value` pair, so unrelated
/// diagnostics on the same line stay intact.
fn end_of_multiword_value(text: &str, value: &str, end: usize) -> usize {
    static TRAILING_WORD: Lazy<Regex> = lazy_regex!(r#"^[\t ]+[^\s,;:=(){}\[\]"]+"#);

    if delimiters(value).is_some() {
        return end;
    }

    let mut end = end;
    while let Some(word) = TRAILING_WORD.find(&text[end..]) {
        let next = end + word.end();
        // `passphrase=hunter2 pin = 1234`: the separator may be spaced out, and
        // absorbing `pin` would leave its own value behind unredacted.
        if text[next..]
            .trim_start_matches([' ', '\t'])
            .starts_with([':', '='])
        {
            break;
        }
        end = next;
    }
    end
}

/// Redact secrets carrying no label at all.
fn redact_bare_secrets(text: &str) -> String {
    // A base64-encoded PSBT always starts with the `psbt\xff` magic.
    static PSBT: Lazy<Regex> = lazy_regex!(r"\bcHNidP[A-Za-z0-9+/]+=*");
    // xpub/xprv and the ypub/zpub/tpub/upub/vpub variants, mainnet or testnet.
    static EXTENDED_KEY: Lazy<Regex> =
        lazy_regex!(r"\b[xyztuvXYZTUV](?:pub|prv)[1-9A-HJ-NP-Za-km-z]{40,}");
    // 16 bytes or more of contiguous hex: frame payloads, txids, serialized txs.
    static LONG_HEX: Lazy<Regex> = lazy_regex!(r"\b[0-9a-fA-F]{32,}\b");
    // The same payloads once a debug formatter has split them into groups:
    // `04, 20, 00, ff, ...`, `04 20 00 ff ...`, `04:20:00:ff:...`.
    static GROUPED_HEX: Lazy<Regex> =
        lazy_regex!(r"(?i)\b[0-9a-f]{2}(?:[\s,:_-]+[0-9a-f]{2}){7,}\b");
    // `[4, 32, 0, 255, ...]`, Rust's `Debug` for a slice of bytes.
    static BYTE_ARRAY: Lazy<Regex> = lazy_regex!(r"\[\s*[0-9]{1,3}(?:\s*,\s*[0-9]{1,3}){7,}\s*\]");
    // Any other long unbroken base64 run: serialized credentials and the like.
    static LONG_BASE64: Lazy<Regex> = lazy_regex!(r"[A-Za-z0-9+/]{64,}=*");

    let text = PSBT.replace_all(text, REDACTED);
    let text = EXTENDED_KEY.replace_all(&text, REDACTED);
    let text = LONG_HEX.replace_all(&text, REDACTED);
    let text = GROUPED_HEX.replace_all(&text, REDACTED);
    let text = BYTE_ARRAY.replace_all(&text, format!("[{}]", REDACTED));
    LONG_BASE64.replace_all(&text, REDACTED).into_owned()
}

fn is_sensitive_key(key: &str) -> bool {
    matches_fragment(key, SENSITIVE_KEY_FRAGMENTS) || is_always_sensitive_key(key)
}

fn is_always_sensitive_key(key: &str) -> bool {
    matches_fragment(key, ALWAYS_SENSITIVE_KEY_FRAGMENTS)
}

/// Fold a label down to its letters and digits, then look for any fragment in
/// it, so `host_static_key`, `hostStaticKey` and `"host-static-key"` all hit
/// `key`. Over-matching only costs diagnostic detail; under-matching leaks.
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

fn is_sensitive_labeled_pair(value: &str) -> bool {
    static LABELED_PAIR: Lazy<Regex> =
        lazy_regex!(r#"^(?P<key>[A-Za-z_][A-Za-z0-9_.\-]*)"?\s*[:=]\s*\S"#);
    LABELED_PAIR
        .captures(value)
        .is_some_and(|captures| is_sensitive_key(&captures["key"]))
}

/// Whether a value is safe to forward even under a sensitive label.
///
/// A number only counts when it says so, by carrying a unit or by its label
/// naming a length. A bare integer under any other sensitive label is as
/// likely to be a numeric token or a code, so it is redacted.
fn is_harmless_value(key: &str, value: &str) -> bool {
    static SIZE: Lazy<Regex> =
        lazy_regex!(r"(?i)^-?[0-9]+\s*(b|kb|mb|bit|bits|byte|bytes|char|chars|ms|s)$");
    static BARE_NUMBER: Lazy<Regex> = lazy_regex!(r"^-?[0-9]+$");

    if matches!(
        value.to_ascii_lowercase().as_str(),
        "true" | "false" | "none" | "null" | "nil" | "n/a" | "unknown" | r#""""# | "[]" | "{}"
    ) {
        return true;
    }
    SIZE.is_match(value)
        || (BARE_NUMBER.is_match(value) && matches_fragment(key, COUNT_KEY_FRAGMENTS))
}

/// A placeholder preserving the shape of the value it replaces, so quoted
/// fields stay quoted and arrays stay arrays.
fn placeholder_for(value: &str) -> String {
    match delimiters(value) {
        Some((open, close)) => format!("{}{}{}", open, REDACTED, close),
        None => REDACTED.to_string(),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => format!("{}…<truncated>", &text[..byte_index]),
        None => text.to_string(),
    }
}
