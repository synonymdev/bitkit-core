//! A provider's capability signing key, followed from its published records (Paykit
//! Capabilities §5): its identity key authorizes a routing key per key generation (paykit-rs's
//! Noise Key Authorization), and the routing key delegates to the capability signing key that
//! signs its `paykit-cap.*` quotes and contracts.
//!
//! This reads the same records and checks the same bytes as pubky-swap's
//! `paykit_identity::resolve_signing_key`, on the pubky client bitkit-core already has. The tests
//! hold records upstream's code wrote.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use super::errors::PubkySwapError;
use super::homeserver::fetch_public_file_if_any;

const AUTHORIZATION_PATH: &str = "/pub/paykit-authority/v0/current-key.json";
const AUTHORIZATION_KIND: &str = "paykit.noise_key_authorization";
const AUTHORIZATION_DOMAIN: &[u8] = b"paykit.noise_key_authorization/v1\0";
const DELEGATION_PATH: &str = "/pub/paykit/ext/capabilities/v1/signing-key.json";
const DELEGATION_KIND: &str = "paykit-cap.signing_key";
const DELEGATION_DOMAIN: &[u8] = b"paykit-cap/subkey/v1";
const MAX_BYTES: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    version: u8,
    kind: String,
    owner: String,
    noise_public_key: String,
    noise_static_public_key: String,
    key_generation: u64,
    signature: String,
}

/// The delegation as written. Fields are in JCS order and hold only ASCII strings and safe
/// integers, so `serde_json` writes the bytes the routing key signed.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Delegation {
    key_generation: u64,
    kind: String,
    owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sig: Option<String>,
    signing_key: String,
    version: u64,
}

fn invalid(what: &str) -> PubkySwapError {
    PubkySwapError::validation(format!("provider signing key: {what}"))
}

fn z32_key(z32: &str) -> Result<VerifyingKey, PubkySwapError> {
    let key = pubky::PublicKey::try_from_z32(z32).map_err(|_| invalid("a key is not a pubky"))?;
    VerifyingKey::from_bytes(&key.to_bytes()).map_err(|_| invalid("a key is not Ed25519"))
}

/// The capability signing key, as hex, that `owner`'s records name: the authorization signed by
/// `owner`, then the delegation for the same owner and generation signed by the routing key it
/// authorizes. A delegation for another generation is refused.
pub(crate) fn verify_chain(
    owner: &str,
    authorization: &[u8],
    delegation: &[u8],
) -> Result<String, PubkySwapError> {
    if authorization.len() > MAX_BYTES || delegation.len() > MAX_BYTES {
        return Err(invalid("a record is too large"));
    }
    let auth: Authorization =
        serde_json::from_slice(authorization).map_err(|_| invalid("unreadable authorization"))?;
    if auth.version != 1 || auth.kind != AUTHORIZATION_KIND || auth.key_generation == 0 {
        return Err(invalid("not a version 1 Noise Key Authorization"));
    }
    if auth.owner != owner {
        return Err(invalid("the authorization is for another identity"));
    }
    let owner_key = z32_key(&auth.owner)?;
    let routing_key = z32_key(&auth.noise_public_key)?;
    let noise_static: [u8; 32] = hex::decode(&auth.noise_static_public_key)
        .ok()
        .and_then(|key| key.try_into().ok())
        .ok_or_else(|| invalid("the authorization's static key is malformed"))?;
    let signature: [u8; 64] = STANDARD
        .decode(&auth.signature)
        .ok()
        .and_then(|sig| sig.try_into().ok())
        .ok_or_else(|| invalid("the authorization's signature is malformed"))?;
    let signed = [
        AUTHORIZATION_DOMAIN,
        owner_key.as_bytes(),
        routing_key.as_bytes(),
        &noise_static,
        &auth.key_generation.to_be_bytes(),
    ]
    .concat();
    owner_key
        .verify_strict(&signed, &Signature::from_bytes(&signature))
        .map_err(|_| invalid("the authorization is not signed by its owner"))?;

    let wire: Delegation =
        serde_json::from_slice(delegation).map_err(|_| invalid("unreadable delegation"))?;
    if wire.version != 1 || wire.kind != DELEGATION_KIND {
        return Err(invalid("not a version 1 delegation"));
    }
    if wire.owner != auth.owner {
        return Err(invalid("the delegation is for another identity"));
    }
    if wire.key_generation != auth.key_generation {
        return Err(invalid("the delegation is for another key generation"));
    }
    let signing_key: [u8; 32] = URL_SAFE_NO_PAD
        .decode(&wire.signing_key)
        .ok()
        .and_then(|key| key.try_into().ok())
        .ok_or_else(|| invalid("the delegated key is malformed"))?;
    VerifyingKey::from_bytes(&signing_key)
        .map_err(|_| invalid("the delegated key is not Ed25519"))?;
    let signature: [u8; 64] = wire
        .sig
        .as_deref()
        .and_then(|sig| URL_SAFE_NO_PAD.decode(sig).ok())
        .and_then(|sig| sig.try_into().ok())
        .ok_or_else(|| invalid("the delegation's signature is malformed"))?;
    let mut signed = DELEGATION_DOMAIN.to_vec();
    signed.push(0);
    signed.extend(serde_json::to_vec(&Delegation {
        sig: None,
        ..wire.clone()
    })?);
    routing_key
        .verify_strict(&signed, &Signature::from_bytes(&signature))
        .map_err(|_| invalid("the delegation is not signed by the authorized routing key"))?;
    Ok(hex::encode(signing_key))
}

/// The capability signing key `provider` (bare z32) publishes, or `None` when it publishes no
/// authorization or delegation. An error when a published record does not verify.
pub(crate) async fn resolve(provider: &str) -> Result<Option<String>, PubkySwapError> {
    let fetch = |path: &str| {
        let addr = format!("{provider}{path}");
        async move { fetch_public_file_if_any(&addr).await }
    };
    let Some(authorization) = fetch(AUTHORIZATION_PATH).await? else {
        return Ok(None);
    };
    let Some(delegation) = fetch(DELEGATION_PATH).await? else {
        return Ok(None);
    };
    verify_chain(provider, &authorization, &delegation).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written by pubky-swap's `paykit_identity` at f45e204 and paykit-rs at 01de5dd, for Pubky
    // secret `[7; 32]`. `DELEGATION_1` is upstream's golden vector.
    const OWNER: &str = "7jfgaa9nutjyixzikb7tgmsf9gkwq7iqz498zr1nd5ig1fng4esy";
    const AUTHORIZATION_1: &str = r#"{"version":1,"kind":"paykit.noise_key_authorization","owner":"7jfgaa9nutjyixzikb7tgmsf9gkwq7iqz498zr1nd5ig1fng4esy","noise_public_key":"yr51em3ri9u7yeos61bu3fa1smuepmigxigjceixykbm76j93wjo","noise_static_public_key":"22441e724551e9f183ce9d7dc0cc3879351dff59f04ad989ca8529c540aeca1b","key_generation":1,"signature":"FjpeQWA/dA7C4+IoiNzX3WU/p1dXEKxVBS1M8XRRs0bBZ7if3nvxD/keZ+pOge1TDdzNH6RH/kSGxg3O4j/YCw=="}"#;
    const DELEGATION_1: &str = r#"{"key_generation":1,"kind":"paykit-cap.signing_key","owner":"7jfgaa9nutjyixzikb7tgmsf9gkwq7iqz498zr1nd5ig1fng4esy","sig":"Sj9Xps4rjkmNrqYv4EU8OuD7gyN4LGNb1SyAbjv0p9PEEg7HekF6unysSnkPRXDn_k9XOBHmll3-Xdo2xkrtAQ","signing_key":"7TrkHFnQPW67doNM4TipjT9CmZygnHHbriRjgBb4Ry8","version":1}"#;
    const SIGNER_1: &str = "ed3ae41c59d03d6ebb76834ce138a98d3f42999ca09c71dbae24638016f8472f";
    const AUTHORIZATION_2: &str = r#"{"version":1,"kind":"paykit.noise_key_authorization","owner":"7jfgaa9nutjyixzikb7tgmsf9gkwq7iqz498zr1nd5ig1fng4esy","noise_public_key":"h9968eqdmsz5fcrfjrfdssmrpyc35mdazxjd1wcxf7dfspaz3kmo","noise_static_public_key":"b41fd37e6772a4efc6475d7c57ce8197264aff7209785c25ca71c1051d167220","key_generation":2,"signature":"PpXMWkTx9Y0+0Yt99dqdKAL99GEzMVPQnYCYVlxTIqkKt6wnVCTUczHZFuJMSgym5FS/8Lb0M6jd6+l/UXInDA=="}"#;
    const DELEGATION_2: &str = r#"{"key_generation":2,"kind":"paykit-cap.signing_key","owner":"7jfgaa9nutjyixzikb7tgmsf9gkwq7iqz498zr1nd5ig1fng4esy","sig":"6M_pzVhuKJiBpPwTTd15LQu89Asth_k1iLgsDn4XvgBd-MtTPtsmTCSMiNL4LAOEALtCrjEphJ0VffYE16A1Dw","signing_key":"oaDkbPRgoLq6pWzuAwpr8xCrflZjn47MRHG7aJxwLMY","version":1}"#;
    const SIGNER_2: &str = "a1a0e46cf460a0babaa56cee030a6bf310ab7e56639f8ecc4471bb689c702cc6";

    fn chain(authorization: &str, delegation: &str) -> Result<String, PubkySwapError> {
        verify_chain(OWNER, authorization.as_bytes(), delegation.as_bytes())
    }

    fn edit(json: &str, field: &str, value: serde_json::Value) -> String {
        let mut record: serde_json::Value = serde_json::from_str(json).unwrap();
        record[field] = value;
        serde_json::to_string(&record).unwrap()
    }

    #[test]
    fn follows_upstream_records_to_the_delegated_key() {
        assert_eq!(chain(AUTHORIZATION_1, DELEGATION_1).unwrap(), SIGNER_1);
        assert_eq!(chain(AUTHORIZATION_2, DELEGATION_2).unwrap(), SIGNER_2);
    }

    #[test]
    fn refuses_a_delegation_for_another_generation() {
        assert!(chain(AUTHORIZATION_2, DELEGATION_1).is_err());
        assert!(chain(AUTHORIZATION_1, DELEGATION_2).is_err());
    }

    #[test]
    fn refuses_records_for_another_identity() {
        let other = "yr51em3ri9u7yeos61bu3fa1smuepmigxigjceixykbm76j93wjo";
        assert!(verify_chain(other, AUTHORIZATION_1.as_bytes(), DELEGATION_1.as_bytes()).is_err());
    }

    #[test]
    fn refuses_tampered_records() {
        let signer_2_b64 = URL_SAFE_NO_PAD.encode(hex::decode(SIGNER_2).unwrap());
        // Another delegated key under the same signature, or another routing key.
        assert!(chain(
            AUTHORIZATION_1,
            &edit(DELEGATION_1, "signing_key", signer_2_b64.into())
        )
        .is_err());
        let routing_2 = "h9968eqdmsz5fcrfjrfdssmrpyc35mdazxjd1wcxf7dfspaz3kmo";
        assert!(chain(
            &edit(AUTHORIZATION_1, "noise_public_key", routing_2.into()),
            DELEGATION_1
        )
        .is_err());
        assert!(chain(
            AUTHORIZATION_1,
            &edit(DELEGATION_1, "sig", serde_json::Value::Null)
        )
        .is_err());
        assert!(chain(AUTHORIZATION_1, &edit(DELEGATION_1, "extra", 1.into())).is_err());
    }
}
