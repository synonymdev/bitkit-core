//! The wallet's Lightning address on Ark Labs' hosted lnurl-server, ported from `@arkade-os/lnurl-client`
//! (`arkadeLnurl(...).owned` and `.claim`, `deriveSessionTokenForIdentity`, `claimPublicKeyOf`, `encodeLnurl`).
//!
//! The server knows a wallet only by its session token, an ECDSA signature by the identity key, so a wallet
//! restored from the same seed derives the same token and finds its address again.

use super::errors::ArkadeError;
use bech32::{Bech32, Hrp};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::key::{Keypair, Secp256k1};
use bitcoin::secp256k1::Message;
use reqwest::{Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MESSAGE_PREFIX: &str = "lnurl-session:";

/// `deriveSessionTokenForIdentity(identity, domain)`: hex SHA-256 of the compact RFC 6979 ECDSA signature over
/// SHA-256(`lnurl-session:` + domain). The TypeScript client signs twice to prove the signer deterministic;
/// RFC 6979 is, so one signature here is the same bytes.
pub fn session_token(identity: &Keypair, domain: &str) -> Result<String, ArkadeError> {
    let domain = domain.trim().to_lowercase();
    if domain.is_empty() {
        return Err(ArkadeError::invalid(
            "a session token must be bound to a domain",
        ));
    }
    let digest = sha256::Hash::hash(format!("{MESSAGE_PREFIX}{domain}").as_bytes());
    let signature = Secp256k1::new().sign_ecdsa(
        &Message::from_digest(digest.to_byte_array()),
        &identity.secret_key(),
    );
    Ok(hex::encode(
        sha256::Hash::hash(&signature.serialize_compact()).to_byte_array(),
    ))
}

/// `claimPublicKeyOf(identity)`: the identity key, compressed, as hex.
pub fn claim_public_key(identity: &Keypair) -> String {
    hex::encode(identity.public_key().serialize())
}

/// `deriveSessionId(token)`: the first 32 hex chars of SHA-256 of the token bytes.
pub fn session_id(token: &str) -> Result<String, ArkadeError> {
    let bytes = hex::decode(token).map_err(ArkadeError::internal)?;
    Ok(hex::encode(sha256::Hash::hash(&bytes).to_byte_array())[..32].to_string())
}

/// `encodeLnurl(url)`: bech32 with the `lnurl` prefix, uppercased.
pub fn encode_lnurl(url: &str) -> Result<String, ArkadeError> {
    let hrp = Hrp::parse("lnurl").map_err(ArkadeError::internal)?;
    let encoded = bech32::encode::<Bech32>(hrp, url.as_bytes()).map_err(ArkadeError::internal)?;
    Ok(encoded.to_uppercase())
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeLightningAddress {
    pub handle: String,
    pub lightning_address: Option<String>,
    pub lnurl: String,
}

/// Where the server pays this wallet: what `arkadeLnurl` reads from the wallet as its destinations.
pub struct Destinations {
    pub arkade_address: String,
    pub boarding_address: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedAddress {
    username: Option<String>,
    handle: Option<String>,
    domain: String,
    status: String,
    lightning_address: Option<String>,
    session_lnurl: Option<String>,
    arkade_address: Option<String>,
    claim_public_key: Option<String>,
    boarding_address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisteredAddress {
    username: Option<String>,
    handle: Option<String>,
    lightning_address: Option<String>,
    session_lnurl: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityBinding<'a> {
    arkade_address: &'a str,
    claim_public_key: &'a str,
    boarding_address: &'a str,
    domain: &'a str,
}

pub struct LnurlService {
    base_url: String,
    domain: String,
    token: String,
    claim_public_key: String,
    http: reqwest::Client,
    /// The receiver once found or claimed; also the lock that serialises claims, since the server allows one
    /// address per wallet.
    receiver: tokio::sync::Mutex<Option<ArkadeLightningAddress>>,
}

impl LnurlService {
    pub fn new(base_url: &str, identity: &Keypair) -> Result<Self, ArkadeError> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let domain = Url::parse(&base_url)
            .map_err(|e| ArkadeError::invalid(format!("invalid LNURL server URL: {e}")))?
            .host_str()
            .ok_or_else(|| ArkadeError::invalid("LNURL server URL has no host"))?
            .to_lowercase();
        Ok(Self {
            token: session_token(identity, &domain)?,
            claim_public_key: claim_public_key(identity),
            base_url,
            domain,
            http: reqwest::Client::new(),
            receiver: tokio::sync::Mutex::new(None),
        })
    }

    /// The bridge's `lightningAddress()`: the address this identity owns, bound again to the current
    /// destinations, or else a newly claimed random one. A claim that loses a race falls back to the address the
    /// other claim registered.
    pub async fn lightning_address(
        &self,
        to: &Destinations,
    ) -> Result<ArkadeLightningAddress, ArkadeError> {
        let mut receiver = self.receiver.lock().await;
        if let Some(r) = receiver.as_ref() {
            return Ok(r.clone());
        }
        let found = match self.owned(to).await? {
            Some(r) => r,
            None => match self.claim(to).await {
                Ok(r) => r,
                Err(e) => self.owned(to).await?.ok_or(e)?,
            },
        };
        *receiver = Some(found.clone());
        Ok(found)
    }

    /// `owned({ refresh: true })`.
    async fn owned(
        &self,
        to: &Destinations,
    ) -> Result<Option<ArkadeLightningAddress>, ArkadeError> {
        let listed: Vec<ListedAddress> = read(
            self.http
                .get(format!("{}/lnurl/address", self.base_url))
                .bearer_auth(&self.token)
                .send()
                .await,
        )
        .await?;
        let active: Vec<ListedAddress> = listed
            .into_iter()
            .filter(|a| a.domain.eq_ignore_ascii_case(&self.domain) && a.status == "active")
            .collect();
        let mut refused = None;
        for a in &active {
            let bound = a.arkade_address.as_deref() == Some(to.arkade_address.as_str())
                && a.claim_public_key.as_deref() == Some(self.claim_public_key.as_str())
                && a.boarding_address.as_deref() == Some(to.boarding_address.as_str());
            if bound {
                continue;
            }
            if let Err(e) = self.bind(handle_of(&a.handle, &a.username)?, to).await {
                refused.get_or_insert(e);
            }
        }
        if let Some(e) = refused {
            return Err(e);
        }
        let Some(chosen) = active
            .iter()
            .find(|a| a.session_lnurl.is_some())
            .or(active.first())
        else {
            return Ok(None);
        };
        let handle = handle_of(&chosen.handle, &chosen.username)?;
        Ok(Some(self.receiver_from(
            handle,
            chosen.lightning_address.clone(),
            chosen.session_lnurl.is_some(),
        )?))
    }

    /// `claim({})`: register a random address, then bind this identity to it.
    async fn claim(&self, to: &Destinations) -> Result<ArkadeLightningAddress, ArkadeError> {
        let body = serde_json::json!({ "token": self.token, "domain": self.domain });
        let registered: RegisteredAddress = read(
            self.http
                .post(format!("{}/lnurl/address", self.base_url))
                .json(&body)
                .send()
                .await,
        )
        .await?;
        let handle = handle_of(&registered.handle, &registered.username)?;
        self.bind(handle, to).await?;
        self.receiver_from(
            handle,
            registered.lightning_address,
            registered.session_lnurl.is_some(),
        )
    }

    /// `registerArkadeIdentity`: where the server pays offline receives, and the key that may claim them.
    async fn bind(&self, handle: &str, to: &Destinations) -> Result<(), ArkadeError> {
        let mut url = Url::parse(&self.base_url).map_err(ArkadeError::internal)?;
        url.path_segments_mut()
            .map_err(|_| ArkadeError::internal("LNURL server URL cannot take a path"))?
            .extend(["lnurl", "address", handle, "arkade"]);
        let body = IdentityBinding {
            arkade_address: &to.arkade_address,
            claim_public_key: &self.claim_public_key,
            boarding_address: &to.boarding_address,
            domain: &self.domain,
        };
        let _: Value = read(
            self.http
                .post(url)
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await,
        )
        .await?;
        Ok(())
    }

    fn receiver_from(
        &self,
        handle: &str,
        lightning_address: Option<String>,
        nameless: bool,
    ) -> Result<ArkadeLightningAddress, ArkadeError> {
        let url = if nameless {
            format!("{}/lnurl/{}", self.base_url, session_id(&self.token)?)
        } else {
            format!("{}/.well-known/lnurlp/{handle}", self.base_url)
        };
        Ok(ArkadeLightningAddress {
            handle: handle.to_string(),
            lightning_address,
            lnurl: encode_lnurl(&url)?,
        })
    }
}

fn handle_of<'a>(
    handle: &'a Option<String>,
    username: &'a Option<String>,
) -> Result<&'a str, ArkadeError> {
    handle
        .as_deref()
        .or(username.as_deref())
        .ok_or_else(|| ArkadeError::lnurl("address has no handle"))
}

/// `apiFetch`: the JSON body, or the server's `error` (or LUD `reason`) for a refused request.
async fn read<T: serde::de::DeserializeOwned>(
    response: reqwest::Result<Response>,
) -> Result<T, ArkadeError> {
    let response = response.map_err(ArkadeError::network)?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let reason = match (
        body.get("status").and_then(Value::as_str),
        body.get("reason").and_then(Value::as_str),
    ) {
        (Some("ERROR"), Some(reason)) => Some(reason.to_string()),
        _ => body
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    if let Some(reason) = reason {
        return Err(ArkadeError::lnurl(reason));
    }
    if !status.is_success() {
        return Err(ArkadeError::lnurl(status));
    }
    serde_json::from_value(body).map_err(ArkadeError::lnurl)
}
