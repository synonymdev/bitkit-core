//! Requests to a Pubky Swaps provider over iroh `pubky-swap/direct/1`.
//!
//! The provider authenticates the request by the connection's key, so this needs no
//! homeserver on either side. Each request opens a stream, writes `{"message": …}`, and reads
//! the provider's reply, which is one `SwapMessage`.

use std::time::Duration;

use iroh::endpoint::presets;
use iroh::{Endpoint, PublicKey as EndpointId, SecretKey};
use serde_json::json;
use swap_common::messages::SwapMessage;

use super::errors::PubkySwapError;

const DIRECT_ALPN: &[u8] = b"pubky-swap/direct/1";
const MAX_REPLY_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct DirectClient {
    endpoint: Endpoint,
}

impl DirectClient {
    /// Bind an endpoint whose id is the wallet's swap identity.
    pub(crate) async fn bind(identity_secret: [u8; 32]) -> Result<Self, PubkySwapError> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&identity_secret))
            .bind()
            .await
            .map_err(PubkySwapError::transport)?;
        // Reaching a provider needs a home relay; without one the first connects all time out.
        if tokio::time::timeout(ONLINE_TIMEOUT, endpoint.online())
            .await
            .is_err()
        {
            log::warn!("pubky swaps: no home relay after {ONLINE_TIMEOUT:?}");
        }
        Ok(Self { endpoint })
    }

    /// Send one request to `provider_pubky` and return its reply.
    pub(crate) async fn request(
        &self,
        provider_pubky: &str,
        message: &SwapMessage,
    ) -> Result<SwapMessage, PubkySwapError> {
        let provider = provider_endpoint_id(provider_pubky)?;
        let body = serde_json::to_vec(&json!({ "message": message }))?;
        let conn = tokio::time::timeout(
            CONNECT_TIMEOUT,
            self.endpoint.connect(provider, DIRECT_ALPN),
        )
        .await
        .map_err(|_| PubkySwapError::transport("timed out connecting to the provider"))?
        .map_err(PubkySwapError::transport)?;
        let reply = tokio::time::timeout(REQUEST_TIMEOUT, async {
            let (mut send, mut recv) = conn.open_bi().await.map_err(PubkySwapError::transport)?;
            send.write_all(&body)
                .await
                .map_err(PubkySwapError::transport)?;
            send.finish().map_err(PubkySwapError::transport)?;
            recv.read_to_end(MAX_REPLY_BYTES)
                .await
                .map_err(PubkySwapError::transport)
        })
        .await
        .map_err(|_| PubkySwapError::transport("timed out waiting for the provider"))??;
        conn.close(0u32.into(), b"done");
        serde_json::from_slice(&reply)
            .map_err(|e| PubkySwapError::transport(format!("unreadable reply: {e}")))
    }

    pub(crate) async fn close(self) {
        self.endpoint.close().await;
    }
}

/// A pubky and an iroh endpoint id are the same Ed25519 key.
fn provider_endpoint_id(provider_pubky: &str) -> Result<EndpointId, PubkySwapError> {
    let bare = bare_pubky(provider_pubky);
    let key = pubky::PublicKey::try_from_z32(bare)
        .map_err(|e| PubkySwapError::invalid(format!("provider pubky: {e}")))?;
    EndpointId::from_bytes(&key.to_bytes())
        .map_err(|e| PubkySwapError::invalid(format!("provider pubky: {e}")))
}

/// The 52-character z-base-32 key, with any `pubky://` or `pubky` display prefix removed.
pub(crate) fn bare_pubky(pubky: &str) -> &str {
    let pubky = pubky.trim();
    let pubky = pubky.strip_prefix("pubky://").unwrap_or(pubky);
    match pubky.strip_prefix("pubky") {
        Some(rest) if rest.len() == 52 => rest,
        _ => pubky,
    }
}
