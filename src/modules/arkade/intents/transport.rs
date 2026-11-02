//! The Nostr RFQ transport (`nostr.ts`): kind-24859 ephemeral events, NIP-44 v2 encrypted to the
//! solver card's `discovery_pubkey`, from a fresh transport key per negotiation so it is not
//! linkable to the wallet. The reply subscription opens before the request is published.

use super::error::Error;
use super::error::Result;
use super::wire::expect_quote;
use super::wire::RfqQuote;
use nostr_sdk::nips::nip44;
use nostr_sdk::prelude::*;
use serde_json::Value;
use std::time::Duration;

/// Directed RFQ traffic; must match the solver's `NOSTR_KIND_DIRECTED`.
pub const RFQ_DIRECTED_KIND: u16 = 24859;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The solver card's transport facts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SolverCard {
    pub name: String,
    /// x-only hex: who requests are addressed and encrypted to, and who must sign replies.
    pub discovery_pubkey: String,
    pub relays: Vec<String>,
}

impl SolverCard {
    /// `beta-solver`, the mainnet solver Ark Labs pinned (the registry lists none).
    pub fn beta_solver() -> Self {
        Self {
            name: "beta-solver".into(),
            discovery_pubkey: "66422c952f8dcb96e4d0c3f049cd1e265b8461b916d9913c65c2494b64b4e3ce"
                .into(),
            relays: vec!["wss://nostr.arkade.sh".into()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct NostrTransport {
    pub card: SolverCard,
    pub timeout: Duration,
}

impl NostrTransport {
    pub fn new(card: SolverCard) -> Self {
        Self {
            card,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Send one `rfq_request` and wait for its `rfq_quote` (or `rfq_refusal`).
    pub async fn request_quote(&self, request: &Value) -> Result<RfqQuote> {
        let rfq_id = request["rfq_id"].as_str().unwrap_or_default().to_string();
        let pair = request["pair"].as_str().unwrap_or_default().to_string();
        let solver = PublicKey::from_hex(&self.card.discovery_pubkey)
            .map_err(|e| Error::Transport(format!("bad solver key: {e}")))?;
        let keys = Keys::generate();
        let client = Client::new(keys.clone());
        let result = self
            .negotiate(&client, &keys, solver, request, &rfq_id)
            .await;
        client.shutdown().await;
        expect_quote(result?, &rfq_id, &pair)
    }

    async fn negotiate(
        &self,
        client: &Client,
        keys: &Keys,
        solver: PublicKey,
        request: &Value,
        rfq_id: &str,
    ) -> Result<Value> {
        for relay in &self.card.relays {
            client
                .add_relay(relay.as_str())
                .await
                .map_err(|e| Error::Transport(format!("relay {relay}: {e}")))?;
        }
        client.connect().await;
        client.wait_for_connection(Duration::from_secs(10)).await;

        let mut notifications = client.notifications();
        let filter = Filter::new()
            .kind(Kind::Custom(RFQ_DIRECTED_KIND))
            .pubkey(keys.public_key())
            .author(solver);
        client
            .subscribe(filter, None)
            .await
            .map_err(|e| Error::Transport(format!("subscribe: {e}")))?;

        let content = nip44::encrypt(
            keys.secret_key(),
            &solver,
            request.to_string(),
            nip44::Version::V2,
        )
        .map_err(|e| Error::Transport(format!("encrypt: {e}")))?;
        let event = EventBuilder::new(Kind::Custom(RFQ_DIRECTED_KIND), content)
            .tag(Tag::public_key(solver))
            .sign_with_keys(keys)
            .map_err(|e| Error::Transport(format!("sign: {e}")))?;
        let sent = client
            .send_event(&event)
            .await
            .map_err(|e| Error::Transport(format!("publish: {e}")))?;
        if sent.success.is_empty() {
            return Err(Error::Transport(format!(
                "no relay accepted the RFQ message: {:?}",
                sent.failed
            )));
        }

        let wait = async {
            loop {
                match notifications.recv().await {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        if event.pubkey != solver
                            || event.kind != Kind::Custom(RFQ_DIRECTED_KIND)
                            || event.verify().is_err()
                        {
                            continue;
                        }
                        let Ok(plain) = nip44::decrypt(keys.secret_key(), &solver, &event.content)
                        else {
                            continue;
                        };
                        let Ok(payload) = serde_json::from_str::<Value>(&plain) else {
                            continue;
                        };
                        if payload.get("rfq_id").and_then(Value::as_str) == Some(rfq_id) {
                            return Ok(payload);
                        }
                    }
                    Ok(RelayPoolNotification::Shutdown) => {
                        return Err(Error::Transport("relay pool shut down".into()))
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(e) => return Err(Error::Transport(e.to_string())),
                }
            }
        };
        tokio::time::timeout(self.timeout, wait)
            .await
            .map_err(|_| Error::Timeout(self.timeout.as_secs()))?
    }
}
