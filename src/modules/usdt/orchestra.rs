use super::{
    deposits::{number, optional_number},
    keys::parse_address,
    rpc::{bounded_json, endpoint_client},
    UsdtDestination, UsdtError, UsdtOrchestraTransfer,
};
use alloy_primitives::Address;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

pub(super) struct Orchestra {
    client: reqwest::Client,
    url: String,
}

// Funding and destination instructions travel together through signing, recovery and backup.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct OrchestraPlan {
    pub quote_id: String,
    pub ticket: String,
    pub funding_address: String,
    pub destination: UsdtDestination,
    pub recipient: String,
    pub amount: u64,
    pub received_amount: u64,
    pub expires_at: u64,
}

impl std::fmt::Debug for OrchestraPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrchestraPlan")
            .field("quote_id", &self.quote_id)
            .field("destination", &self.destination)
            .finish_non_exhaustive()
    }
}

impl OrchestraPlan {
    pub fn transfer(&self) -> UsdtOrchestraTransfer {
        UsdtOrchestraTransfer {
            quote_id: self.quote_id.clone(),
            funding_address: self.funding_address.clone(),
            destination_tx: None,
            refund_tx: None,
            refund_amount: None,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Delivery {
    pub status: String,
    #[serde(deserialize_with = "optional_number")]
    pub received_amount: Option<u64>,
    pub destination_tx: Option<String>,
    pub refund_tx: Option<String>,
    #[serde(deserialize_with = "optional_number")]
    pub refund_amount: Option<u64>,
}

impl Orchestra {
    pub fn new(url: String) -> Result<Self, UsdtError> {
        Ok(Self {
            client: endpoint_client(&url, Duration::from_secs(22))?,
            url,
        })
    }

    pub async fn networks(&self) -> Result<Vec<UsdtDestination>, UsdtError> {
        #[derive(Deserialize)]
        struct Networks {
            networks: Vec<String>,
        }
        let networks: Networks = self.response(self.client.get(&self.url)).await?;
        Ok(networks
            .networks
            .into_iter()
            .filter_map(|network| UsdtDestination::from_network(&network))
            .collect())
    }

    pub async fn quote(
        &self,
        owner: Address,
        recipient: String,
        amount: u64,
        destination: UsdtDestination,
    ) -> Result<OrchestraPlan, UsdtError> {
        #[derive(Deserialize)]
        struct Quote {
            quote_id: String,
            funding_address: String,
            #[serde(deserialize_with = "number")]
            received_amount: u64,
            expires_at: u64,
            ticket: String,
        }
        let quote: Quote = self
            .response(self.client.post(&self.url).json(&json!({
                "action":"quote", "request_id":uuid::Uuid::new_v4().to_string(),
                "owner":owner.to_checksum(None), "recipient":recipient, "amount":amount.to_string(),
                "network":destination.network(),
            })))
            .await?;
        let funding =
            parse_address(&quote.funding_address).map_err(|_| UsdtError::InvalidResponse)?;
        if quote.quote_id.is_empty()
            || quote.quote_id.len() > 128
            || quote.ticket.is_empty()
            || quote.ticket.len() > 4096
            || quote.received_amount == 0
            || quote.received_amount > amount
            || quote.expires_at <= super::wallet::now() + 10
            || [
                owner,
                super::types::TOKEN,
                super::types::OFT,
                super::types::BRIDGE_HELPER,
                super::account::ENTRY_POINT,
                super::account::DELEGATE,
                super::paymaster::PAYMASTER,
            ]
            .contains(&funding)
        {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(OrchestraPlan {
            quote_id: quote.quote_id,
            ticket: quote.ticket,
            funding_address: funding.to_checksum(None),
            destination,
            recipient,
            amount,
            received_amount: quote.received_amount,
            expires_at: quote.expires_at,
        })
    }

    pub async fn status(
        &self,
        plan: &OrchestraPlan,
        source_tx: &str,
    ) -> Result<Delivery, UsdtError> {
        self.response(
            self.client
                .post(&self.url)
                .json(&json!({"action":"status", "ticket":plan.ticket, "source_tx":source_tx})),
        )
        .await
    }

    async fn response<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, UsdtError> {
        let response = request
            .send()
            .await
            .map_err(|_| UsdtError::NetworkUnavailable)?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(UsdtError::RateLimited);
        }
        let value = bounded_json(response, 16384, UsdtError::InvalidResponse).await?;
        if !status.is_success() {
            return Err(match value["error"].as_str() {
                Some("amount_too_small" | "amount_too_large" | "amount_exceeds_liquidity") => {
                    UsdtError::InvalidAmount
                }
                Some("invalid_address") => UsdtError::InvalidAddress,
                Some("route_unavailable" | "not_configured") => UsdtError::UnsupportedRoute,
                _ => UsdtError::NetworkUnavailable,
            });
        }
        serde_json::from_value(value).map_err(Into::into)
    }
}

pub(super) fn validate_recipient(
    value: &str,
    destination: UsdtDestination,
) -> Result<String, UsdtError> {
    match destination {
        UsdtDestination::Tron | UsdtDestination::Solana => {
            let network = if destination == UsdtDestination::Tron {
                super::UsdtDepositNetwork::Tron
            } else {
                super::UsdtDepositNetwork::Solana
            };
            super::deposits::validate_source_address(value, network)?;
            Ok(value.to_owned())
        }
        _ => {
            let address = parse_address(value)?;
            if destination.token() == Some(address)
                || (destination == UsdtDestination::Arbitrum
                    && [super::types::OFT, super::types::BRIDGE_HELPER].contains(&address))
                || [
                    super::account::ENTRY_POINT,
                    super::account::DELEGATE,
                    super::paymaster::PAYMASTER,
                ]
                .contains(&address)
            {
                return Err(UsdtError::InvalidAddress);
            }
            Ok(address.to_checksum(None))
        }
    }
}

/// Validates a recipient for the chosen network; payment URIs remain Arbitrum-only.
#[uniffi::export]
pub fn usdt_validate_recipient(
    value: String,
    destination: UsdtDestination,
) -> Result<String, UsdtError> {
    validate_recipient(value.trim(), destination)
}
