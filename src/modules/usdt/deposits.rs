use super::{
    keys::{derive_key, key_address, parse_address},
    rpc::{bounded_json, endpoint_client},
    user_operation::sign_hash,
    UsdtError,
};
use alloy_primitives::{eip191_hash_message, Address};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum UsdtDepositNetwork {
    Ethereum,
    Tron,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositAddress {
    pub network: UsdtDepositNetwork,
    pub address: String,
    pub recipient: String,
    #[serde(deserialize_with = "number")]
    pub amount: u64,
    #[serde(deserialize_with = "number")]
    pub estimated_received: u64,
    pub min_usd_cents: Option<String>,
    pub max_usd_cents: Option<String>,
    pub slippage_bps: u32,
    #[serde(default)]
    pub uri: String,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDeposit {
    pub id: String,
    pub network: String,
    pub asset: String,
    #[serde(deserialize_with = "optional_number")]
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
    #[serde(deserialize_with = "optional_number")]
    pub amount_in: Option<u64>,
    #[serde(deserialize_with = "optional_number")]
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
            networks: Vec<UsdtDepositNetwork>,
        }
        Ok(self
            .response::<Networks>(self.client.get(&self.url))
            .await?
            .networks)
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
            || parse_address(&result.recipient)? != self.address
            || result.amount != amount
            || result.estimated_received == 0
            || result.slippage_bps != 50
        {
            return Err(UsdtError::InvalidResponse);
        }
        validate_source_address(&result.address, network)?;
        result.uri = match network {
            UsdtDepositNetwork::Ethereum => format!(
                "ethereum:0xdAC17F958D2ee523a2206206994597C13D831ec7@1/transfer?address={}",
                result.address
            ),
            UsdtDepositNetwork::Tron => result.address.clone(),
        };
        Ok(result)
    }

    pub async fn history(
        &self,
        offset: u32,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtDepositPage, UsdtError> {
        self.call(
            json!({"action":"history","offset":offset}),
            mnemonic.into(),
            passphrase.map(Into::into),
        )
        .await
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
        validate_source_address(refund_address.trim(), network)?;
        let result: Value = self
            .call(
                json!({"action":"refund","depositId":deposit_id,"offset":offset,
            "refundAddress":refund_address.trim()}),
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
        let key = derive_key(mnemonic, passphrase)?;
        if key_address(&key) != self.address {
            return Err(UsdtError::InvalidCredentials);
        }
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
                Some("invalid_authorization") => UsdtError::InvalidCredentials,
                Some("clock_skew") => UsdtError::ClockSkew,
                Some("amount_too_small" | "amount_too_large" | "amount_exceeds_liquidity") => {
                    UsdtError::InvalidAmount
                }
                Some("invalid_refund_address") => UsdtError::InvalidAddress,
                Some("route_unavailable") => UsdtError::UnsupportedRoute,
                Some(
                    "refund_not_available"
                    | "instruction_conflict"
                    | "not_found"
                    | "operator_required"
                    | "standing_tron_refund_requires_operator",
                ) => UsdtError::DepositNeedsAttention,
                _ => UsdtError::NetworkUnavailable,
            });
        }
        serde_json::from_value(value?).map_err(Into::into)
    }
}

fn validate_source_address(value: &str, network: UsdtDepositNetwork) -> Result<(), UsdtError> {
    match network {
        UsdtDepositNetwork::Ethereum => {
            parse_address(value)?;
        }
        UsdtDepositNetwork::Tron => {
            let payload =
                bitcoin::base58::decode_check(value).map_err(|_| UsdtError::InvalidAddress)?;
            if payload.len() != 21 || payload[0] != 0x41 {
                return Err(UsdtError::InvalidAddress);
            }
        }
    }
    Ok(())
}

fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}
fn optional_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|v| v.parse().map_err(serde::de::Error::custom))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn deposit_authorization_matches_ethers_and_rejects_another_wallet() {
        let vector: Value =
            serde_json::from_str(include_str!("fixtures/deposit-signature.json")).unwrap();
        let request: Value = serde_json::from_str(vector["request"].as_str().unwrap()).unwrap();
        let client = UsdtDepositClient::new(
            request["owner"].as_str().unwrap().into(),
            "https://example.com/v1/usdt/deposits".into(),
        )
        .unwrap();
        assert_eq!(
            client
                .authorize(
                    request["payload"].clone(),
                    PHRASE.to_owned().into(),
                    None,
                    1_800_000_000
                )
                .unwrap(),
            vector
        );
        assert!(matches!(
            client.authorize(
                request["payload"].clone(),
                PHRASE.to_owned().into(),
                Some("other".to_owned().into()),
                1_800_000_000
            ),
            Err(UsdtError::InvalidCredentials)
        ));
    }

    #[test]
    fn deposit_addresses_and_transport_reject_wrong_networks() {
        let tron = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
        assert!(validate_source_address(tron, UsdtDepositNetwork::Tron).is_ok());
        assert!(validate_source_address(tron, UsdtDepositNetwork::Ethereum).is_err());
        assert!(validate_source_address(
            "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6s",
            UsdtDepositNetwork::Tron
        )
        .is_err());
        for url in [
            "http://example.com",
            "https://user:password@example.com",
            "https://example.com?key=secret",
        ] {
            assert!(UsdtDepositClient::new(
                "0x1111111111111111111111111111111111111111".into(),
                url.into()
            )
            .is_err());
        }
    }
}
