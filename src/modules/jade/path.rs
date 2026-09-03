//! BIP32 path validation and lowering to Jade's wire representation.
//!
//! Paths cross the FFI as strings such as `m/84'/0'/0'/0/0`, matching every
//! other signer in this repo. Jade wants an array of `u32` with the hardened
//! bit already set.

use std::str::FromStr;

use bitcoin::bip32::DerivationPath;

use super::errors::JadeError;

/// Jade rejects paths deeper than this.
const MAX_DEPTH: usize = 8;

/// Validate a derivation path string.
///
/// `DerivationPath::from_str` alone is not enough. It accepts `""`, `"m"` and
/// `"m/"` as the master path, and it strips an optional `m/` prefix rather than
/// requiring one, so `"84'/0'/0'"` also parses. Either would be a silent
/// footgun here: an empty path handed to `sign_message` would sign with the
/// master key, and a prefix-less path means the app and this crate disagree
/// about what a path string is.
///
/// `allow_master` opts in to the empty path, which is only wanted by the
/// deliberate master-fingerprint lookup.
pub(crate) fn validate(path: &str, allow_master: bool) -> Result<(), JadeError> {
    let invalid = |reason: String| JadeError::InvalidPath {
        error_details: reason,
    };

    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(invalid("path is empty".to_string()));
    }

    let remainder = if trimmed == "m" {
        ""
    } else if let Some(rest) = trimmed.strip_prefix("m/") {
        rest
    } else {
        return Err(invalid(format!("path must start with 'm/': {path}")));
    };

    if remainder.is_empty() {
        if allow_master {
            return Ok(());
        }
        return Err(invalid(
            "the master path is not valid for this operation".to_string(),
        ));
    }

    let components: Vec<&str> = remainder.split('/').collect();
    if components.len() > MAX_DEPTH {
        return Err(invalid(format!(
            "path depth {} exceeds the maximum of {MAX_DEPTH}",
            components.len()
        )));
    }

    for (index, component) in components.iter().enumerate() {
        if component.is_empty() {
            return Err(invalid(format!("empty path component at index {index}")));
        }
        let digits = component
            .strip_suffix('\'')
            .or_else(|| component.strip_suffix('h'))
            .unwrap_or(component);
        if digits.is_empty() || digits.parse::<u32>().is_err() {
            return Err(invalid(format!(
                "invalid path component '{component}' at index {index}"
            )));
        }
    }

    // Delegate the range check (index below 2^31) to the typed parser.
    DerivationPath::from_str(trimmed)
        .map(|_| ())
        .map_err(|error| invalid(format!("{path}: {error}")))
}

/// Validate and lower a path to the `u32` array Jade expects.
pub(crate) fn to_wire(path: &str, allow_master: bool) -> Result<Vec<u32>, JadeError> {
    validate(path, allow_master)?;
    let parsed = DerivationPath::from_str(path.trim()).map_err(|error| JadeError::InvalidPath {
        error_details: format!("{path}: {error}"),
    })?;
    Ok(parsed.into_iter().map(|child| u32::from(*child)).collect())
}

/// The BIP44 purpose element of a path, if it has one.
///
/// Used to check that the requested address variant agrees with the path, so a
/// caller cannot ask for a legacy address under an `m/84'` path.
pub(crate) fn purpose(path: &str) -> Option<u32> {
    let wire = to_wire(path, true).ok()?;
    // Strip the hardened bit before comparing against 44 / 49 / 84 / 86.
    wire.first().map(|element| element & 0x7fff_ffff)
}
