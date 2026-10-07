use super::{
    keys::{derive_owner_key, parse_address},
    rpc::{bounded_json, endpoint_client},
    user_operation::sign_hash,
    UsdtError,
};
use alloy_primitives::{eip191_hash_message, Address};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

const SOLANA_USDT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
const TRON_USDT: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum UsdtDepositNetwork {
    Ethereum,
    Tron,
    Solana,
    Polygon,
    Bsc,
    Base,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositAddress {
    pub network: UsdtDepositNetwork,
    pub address: String,
    pub recipient: String,
    #[serde(deserialize_with = "number")]
    /// Expected input in millionths of USDT, including for 18-decimal BSC USDT.
    pub amount: u64,
    #[serde(deserialize_with = "number")]
    pub estimated_received: u64,
    #[serde(default, deserialize_with = "deposit_limit")]
    pub min_usd_cents: Option<String>,
    #[serde(default, deserialize_with = "deposit_limit")]
    pub max_usd_cents: Option<String>,
    pub slippage_bps: u32,
    #[serde(skip)]
    pub uri: String,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDeposit {
    pub id: String,
    pub network: String,
    pub asset: String,
    #[serde(default, deserialize_with = "optional_number")]
    /// Observed source amount in millionths of USDT; finer BSC dust is rounded down.
    pub amount: Option<u64>,
    pub source_tx: String,
    pub status: String,
    pub code: Option<String>,
    pub refund_tx: Option<String>,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositPage {
    pub deposits: Vec<UsdtDeposit>,
    pub next_offset: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositOrder {
    pub status: String,
    #[serde(default, deserialize_with = "optional_number")]
    /// Batch input in millionths of USDT, not necessarily this deposit alone.
    pub amount_in: Option<u64>,
    #[serde(default, deserialize_with = "optional_number")]
    pub amount_out: Option<u64>,
    pub destination_tx: Option<String>,
    pub refund_tx: Option<String>,
    pub code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositDetail {
    pub deposit: UsdtDeposit,
    pub order: Option<UsdtDepositOrder>,
}

#[derive(uniffi::Object)]
pub struct UsdtDepositClient {
    address: Address,
    client: reqwest::Client,
    url: String,
}

#[uniffi::export(async_runtime = "tokio")]
impl UsdtDepositClient {
    #[uniffi::constructor]
    pub fn new(address: String, service_url: String) -> Result<Arc<Self>, UsdtError> {
        let client = endpoint_client(&service_url, Duration::from_secs(22))?;
        Ok(Arc::new(Self {
            address: parse_address(&address)?,
            client,
            url: service_url,
        }))
    }

    pub async fn networks(&self) -> Result<Vec<UsdtDepositNetwork>, UsdtError> {
        #[derive(Deserialize)]
        struct Networks {
            networks: Vec<String>,
        }
        Ok(self
            .response::<Networks>(self.client.get(&self.url))
            .await?
            .networks
            .into_iter()
            .filter_map(|network| match network.as_str() {
                "ethereum" => Some(UsdtDepositNetwork::Ethereum),
                "tron" => Some(UsdtDepositNetwork::Tron),
                "solana" => Some(UsdtDepositNetwork::Solana),
                "polygon" => Some(UsdtDepositNetwork::Polygon),
                "bsc" => Some(UsdtDepositNetwork::Bsc),
                "base" => Some(UsdtDepositNetwork::Base),
                _ => None,
            })
            .collect())
    }

    pub async fn receive(
        &self,
        network: UsdtDepositNetwork,
        amount: u64,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtDepositAddress, UsdtError> {
        let mnemonic = Zeroizing::new(mnemonic);
        let passphrase = passphrase.map(Zeroizing::new);
        if amount == 0 {
            return Err(UsdtError::InvalidAmount);
        }
        let mut result: UsdtDepositAddress = self
            .call(
                json!({"action":"receive", "network":network,
            "amount":amount.to_string()}),
                mnemonic,
                passphrase,
            )
            .await?;
        if result.network != network
            || parse_address(&result.recipient).map_err(|_| UsdtError::InvalidResponse)?
                != self.address
            || result.amount != amount
            || result.estimated_received == 0
            || result.estimated_received > amount
            || result.slippage_bps != 50
        {
            return Err(UsdtError::InvalidResponse);
        }
        validate_source_address(&result.address, network)
            .map_err(|_| UsdtError::InvalidResponse)?;
        if let Some((chain, token)) = network.evm_token() {
            if parse_address(&result.address).map_err(|_| UsdtError::InvalidResponse)?
                == self.address
            {
                return Err(UsdtError::InvalidResponse);
            }
            result.uri = format!(
                "ethereum:{token}@{chain}/transfer?address={}",
                result.address
            );
        } else {
            if network == UsdtDepositNetwork::Tron {
                let decoded = bitcoin::base58::decode_check(&result.address)
                    .map_err(|_| UsdtError::InvalidResponse)?;
                if Address::from_slice(&decoded[1..]) == self.address {
                    return Err(UsdtError::InvalidResponse);
                }
            }
            result.uri = result.address.clone();
        }
        Ok(result)
    }

    pub async fn history(
        &self,
        offset: u32,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtDepositPage, UsdtError> {
        let page: UsdtDepositPage = self
            .call(
                json!({"action":"history","offset":offset}),
                mnemonic.into(),
                passphrase.map(Into::into),
            )
            .await?;
        if page.deposits.len() > 50
            || page
                .next_offset
                .is_some_and(|next| next <= offset || next > 100_000)
        {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(page)
    }

    pub async fn detail(
        &self,
        deposit_id: String,
        offset: u32,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtDepositDetail, UsdtError> {
        let result: UsdtDepositDetail = self
            .call(
                json!({"action":"detail","depositId":deposit_id,"offset":offset}),
                mnemonic.into(),
                passphrase.map(Into::into),
            )
            .await?;
        if result.deposit.id != deposit_id {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(result)
    }

    pub async fn request_refund(
        &self,
        deposit_id: String,
        offset: u32,
        refund_address: String,
        network: UsdtDepositNetwork,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<(), UsdtError> {
        let mnemonic = Zeroizing::new(mnemonic);
        let passphrase = passphrase.map(Zeroizing::new);
        let refund_address = refund_address.trim();
        validate_source_address(refund_address, network)?;
        let result: Value = self
            .call(
                json!({"action":"refund","depositId":deposit_id,"offset":offset,
            "refundAddress":refund_address}),
                mnemonic,
                passphrase,
            )
            .await?;
        if result["status"] != "refund_requested" {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(())
    }
}

impl UsdtDepositClient {
    async fn call<T: DeserializeOwned>(
        &self,
        payload: Value,
        mnemonic: Zeroizing<String>,
        passphrase: Option<Zeroizing<String>>,
    ) -> Result<T, UsdtError> {
        let signed = self.authorize(payload, mnemonic, passphrase, super::wallet::now())?;
        self.response(self.client.post(&self.url).json(&signed))
            .await
    }

    fn authorize(
        &self,
        payload: Value,
        mnemonic: Zeroizing<String>,
        passphrase: Option<Zeroizing<String>>,
        timestamp: u64,
    ) -> Result<Value, UsdtError> {
        let key = derive_owner_key(mnemonic, passphrase, self.address)?;
        let request =
            json!({"owner":self.address.to_checksum(None),"timestamp":timestamp,"payload":payload})
                .to_string();
        let message = format!("Bitkit USDT deposits v1\n{request}");
        let signature = sign_hash(eip191_hash_message(message), &key)?;
        drop(key);
        Ok(json!({"request":request,"signature":signature}))
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
        let value = bounded_json(response, 262144, UsdtError::InvalidResponse).await;
        if !status.is_success() {
            let error = value.unwrap_or_default();
            return Err(match error["error"].as_str() {
                Some("not_configured") => UsdtError::NotConfigured,
                Some("provider_unavailable") => UsdtError::NetworkUnavailable,
                Some("invalid_authorization") => UsdtError::DepositAuthorizationRejected,
                Some("not_found") => UsdtError::DepositNotFound,
                Some("clock_skew") => UsdtError::ClockSkew,
                Some("amount_too_small" | "amount_too_large") => {
                    UsdtError::DepositAmountOutOfRange {
                        min_usd_cents: deposit_limit(&error["min_usd_cents"])?,
                        max_usd_cents: deposit_limit(&error["max_usd_cents"])?,
                    }
                }
                Some("amount_exceeds_liquidity") => UsdtError::UnsupportedRoute,
                Some("invalid_refund_address") => UsdtError::InvalidAddress,
                Some("route_unavailable") => UsdtError::UnsupportedRoute,
                Some(
                    "refund_not_available"
                    | "instruction_conflict"
                    | "operator_required"
                    | "standing_tron_refund_requires_operator",
                ) => UsdtError::DepositNeedsAttention,
                _ if status == reqwest::StatusCode::UNAUTHORIZED
                    || status == reqwest::StatusCode::FORBIDDEN =>
                {
                    UsdtError::DepositAuthorizationRejected
                }
                _ if status.is_client_error() || status.is_redirection() => {
                    UsdtError::InvalidResponse
                }
                _ => UsdtError::NetworkUnavailable,
            });
        }
        serde_json::from_value(value?).map_err(Into::into)
    }
}

impl UsdtDepositNetwork {
    fn evm_token(self) -> Option<(u64, &'static str)> {
        Some(match self {
            Self::Ethereum => (1, "0xdac17f958d2ee523a2206206994597c13d831ec7"),
            Self::Polygon => (137, "0xc2132d05d31c914a87c6611c10748aeb04b58e8f"),
            Self::Bsc => (56, "0x55d398326f99059ff775485246999027b3197955"),
            Self::Base => (8453, "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2"),
            Self::Solana | Self::Tron => return None,
        })
    }
}

pub(super) fn validate_source_address(
    value: &str,
    network: UsdtDepositNetwork,
) -> Result<(), UsdtError> {
    if network == UsdtDepositNetwork::Tron {
        if value.len() != 34 || !value.starts_with('T') || !value.is_ascii() || value == TRON_USDT {
            return Err(UsdtError::InvalidAddress);
        }
        let payload =
            bitcoin::base58::decode_check(value).map_err(|_| UsdtError::InvalidAddress)?;
        if payload.len() != 21 || payload[0] != 0x41 || payload[1..].iter().all(|b| *b == 0) {
            return Err(UsdtError::InvalidAddress);
        }
        return Ok(());
    }
    if let Some((_, token)) = network.evm_token() {
        let address = parse_address(value)?;
        if address == parse_address(token)? {
            return Err(UsdtError::InvalidAddress);
        }
    } else {
        if !(32..=44).contains(&value.len()) || value == SOLANA_USDT {
            return Err(UsdtError::InvalidAddress);
        }
        let bytes = bitcoin::base58::decode(value).map_err(|_| UsdtError::InvalidAddress)?;
        if bytes.len() != 32 || bytes.iter().all(|b| *b == 0) {
            return Err(UsdtError::InvalidAddress);
        }
    }
    Ok(())
}

fn deposit_limit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(value
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.len() <= 40 && value.bytes().all(|c| c.is_ascii_digit())
        })
        .map(str::to_owned))
}

pub(super) fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}
pub(super) fn optional_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|v| v.parse().map_err(serde::de::Error::custom))
        .transpose()
}

#[cfg(test)]
mod tests;
