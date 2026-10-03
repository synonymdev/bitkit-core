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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum UsdtDepositNetwork {
    Ethereum,
    Solana,
    Polygon,
    Optimism,
    Base,
    Avalanche,
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
    #[serde(default, deserialize_with = "deposit_limit")]
    pub min_usd_cents: Option<String>,
    #[serde(default, deserialize_with = "deposit_limit")]
    pub max_usd_cents: Option<String>,
    #[serde(skip)]
    pub uri: String,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDeposit {
    pub id: String,
    pub network: String,
    pub asset: String,
    #[serde(default, deserialize_with = "optional_number")]
    pub amount: Option<u64>,
    pub source_tx: String,
    pub status: String,
    pub code: Option<String>,
    pub refund_tx: Option<String>,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositPage {
    pub deposits: Vec<UsdtDeposit>,
    pub next_before: Option<u64>,
    pub next_offset: u32,
}

#[derive(Clone, Debug, Deserialize, uniffi::Record)]
pub struct UsdtDepositOrder {
    pub status: String,
    #[serde(default, deserialize_with = "optional_number")]
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
                "solana" => Some(UsdtDepositNetwork::Solana),
                "polygon" => Some(UsdtDepositNetwork::Polygon),
                "optimism" => Some(UsdtDepositNetwork::Optimism),
                "base" => Some(UsdtDepositNetwork::Base),
                "avalanche" => Some(UsdtDepositNetwork::Avalanche),
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
        {
            return Err(UsdtError::InvalidResponse);
        }
        validate_source_address(&result.address, network)
            .map_err(|_| UsdtError::InvalidResponse)?;
        if network != UsdtDepositNetwork::Solana
            && parse_address(&result.address).map_err(|_| UsdtError::InvalidResponse)?
                == self.address
        {
            return Err(UsdtError::InvalidResponse);
        }
        result.uri = match network.evm_token() {
            Some((chain, token)) => format!(
                "ethereum:{token}@{chain}/transfer?address={}",
                result.address
            ),
            None => result.address.clone(),
        };
        Ok(result)
    }

    pub async fn history(
        &self,
        before: u64,
        offset: u32,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtDepositPage, UsdtError> {
        let mnemonic = Zeroizing::new(mnemonic);
        let passphrase = passphrase.map(Zeroizing::new);
        if (before == 0 && offset != 0) || offset > 50_000 {
            return Err(UsdtError::InvalidResponse);
        }
        let page: UsdtDepositPage = self
            .call(
                json!({"action":"history","before":before,"offset":offset}),
                mnemonic,
                passphrase,
            )
            .await?;
        let valid_cursor = match page.next_before {
            None => page.next_offset == 0,
            Some(next) if page.next_offset == 0 => next > 0 && (before == 0 || next < before),
            Some(next) => {
                next > 0
                    && (before == 0 || next == before)
                    && !page.deposits.is_empty()
                    && page.next_offset as usize == offset as usize + page.deposits.len()
                    && page.next_offset <= 50_000
            }
        };
        if page.deposits.len() > 100 || !valid_cursor {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(page)
    }

    pub async fn detail(
        &self,
        deposit_id: String,
        before: u64,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtDepositDetail, UsdtError> {
        let result: UsdtDepositDetail = self
            .call(
                json!({"action":"detail","depositId":deposit_id,"before":before}),
                mnemonic.into(),
                passphrase.map(Into::into),
            )
            .await?;
        if result.deposit.id != deposit_id {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(result)
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
                Some("route_unavailable") => UsdtError::UnsupportedRoute,
                Some("operator_required") => UsdtError::DepositNeedsAttention,
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
            Self::Optimism => (10, "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58"),
            Self::Base => (8453, "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2"),
            Self::Avalanche => (43114, "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7"),
            Self::Solana => return None,
        })
    }
}

fn validate_source_address(value: &str, network: UsdtDepositNetwork) -> Result<(), UsdtError> {
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
    fn deposit_authorization_matches_vector_and_rejects_another_wallet() {
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
        let solana = "6G41T4zUUYm47xgYBFoUioUGhigxS98Cj79y7C5nAf1L";
        for network in [
            UsdtDepositNetwork::Ethereum,
            UsdtDepositNetwork::Polygon,
            UsdtDepositNetwork::Optimism,
            UsdtDepositNetwork::Base,
            UsdtDepositNetwork::Avalanche,
        ] {
            assert!(validate_source_address(network.evm_token().unwrap().1, network).is_err());
            assert!(
                validate_source_address("0x1111111111111111111111111111111111111111", network)
                    .is_ok()
            );
            assert!(validate_source_address(solana, network).is_err());
        }
        assert!(validate_source_address(solana, UsdtDepositNetwork::Solana).is_ok());
        for bad in [
            SOLANA_USDT,
            "11111111111111111111111111111111",
            "0x1111111111111111111111111111111111111111",
        ] {
            assert!(validate_source_address(bad, UsdtDepositNetwork::Solana).is_err());
        }
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

    async fn service(
        responses: Vec<(u16, Value)>,
    ) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/usdt/deposits", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(10), async move {
            let mut requests = Vec::new();
            for (status, value) in responses {
                let (socket, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(socket);
                let mut first_line = String::new();
                reader.read_line(&mut first_line).await.unwrap();
                assert!(first_line == "GET /v1/usdt/deposits HTTP/1.1\r\n" || first_line == "POST /v1/usdt/deposits HTTP/1.1\r\n");
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert_ne!(reader.read_line(&mut line).await.unwrap(), 0, "Request ended before its headers");
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).await.unwrap();
                if first_line.starts_with("POST") {
                    requests.push(serde_json::from_slice::<Value>(&bytes).unwrap());
                }
                let body = value.to_string();
                reader.get_mut().write_all(format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            requests
            }).await.expect("Deposit fixture requests must complete")
        });
        (url, task)
    }

    #[tokio::test]
    async fn service_responses_cover_receive_history_and_detail() {
        let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
        let address = "0x1111111111111111111111111111111111111111";
        let deposit = json!({"id":"dep_one","network":"ethereum","asset":"USDT","source_tx":"0xsource","status":"held","code":null,"refund_tx":null});
        let (url, server) = service(vec![
            (200, json!({"networks":["ethereum","solana","polygon","optimism","base","avalanche","bitcoin"]})),
            (200, json!({"network":"ethereum","address":address,"recipient":owner,"amount":"100000000","estimated_received":"98500000","min_usd_cents":"200","max_usd_cents":"10000","uri":123})),
            (200, json!({"deposits":[deposit],"next_before":50,"next_offset":0})),
            (200, json!({"deposit":deposit,"order":{"status":"held"}})),
        ]).await;
        let client = UsdtDepositClient::new(owner, url).unwrap();
        assert_eq!(
            client.networks().await.unwrap(),
            vec![
                UsdtDepositNetwork::Ethereum,
                UsdtDepositNetwork::Solana,
                UsdtDepositNetwork::Polygon,
                UsdtDepositNetwork::Optimism,
                UsdtDepositNetwork::Base,
                UsdtDepositNetwork::Avalanche
            ]
        );
        let received = client
            .receive(
                UsdtDepositNetwork::Ethereum,
                100_000_000,
                PHRASE.into(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(received.estimated_received, 98_500_000);
        assert_eq!(received.min_usd_cents.as_deref(), Some("200"));
        assert_eq!(received.max_usd_cents.as_deref(), Some("10000"));
        assert!(received.uri.contains(address));
        let page = client.history(0, 0, PHRASE.into(), None).await.unwrap();
        assert_eq!(page.deposits[0].amount, None);
        assert_eq!(page.next_before, Some(50));
        let detail = client
            .detail("dep_one".into(), 0, PHRASE.into(), None)
            .await
            .unwrap();
        assert_eq!(detail.order.unwrap().amount_out, None);
        let requests = server.await.unwrap();
        let expected = [
            json!({"action":"receive","network":"ethereum","amount":"100000000"}),
            json!({"action":"history","before":0,"offset":0}),
            json!({"action":"detail","depositId":"dep_one","before":0}),
        ];
        assert_eq!(requests.len(), expected.len());
        for (signed, payload) in requests.into_iter().zip(expected) {
            let request: Value = serde_json::from_str(signed["request"].as_str().unwrap()).unwrap();
            assert_eq!(request["payload"], payload);
            assert_eq!(
                signed,
                client
                    .authorize(
                        payload,
                        PHRASE.to_string().into(),
                        None,
                        request["timestamp"].as_u64().unwrap()
                    )
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn history_pages_require_an_advancing_window_or_offset() {
        let deposit = json!({"id":"deposit","network":"ethereum","asset":"USDT","source_tx":"0xsource","status":"pending"});
        for (next, offset, valid) in [
            (Some(1000), 101, true),
            (Some(900), 0, true),
            (None, 0, true),
            (Some(1000), 100, false),
            (Some(1000), 102, false),
            (Some(1001), 101, false),
            (Some(900), 101, false),
            (None, 101, false),
        ] {
            let (url, server) = service(vec![(
                200,
                json!({
                    "deposits":[deposit],"next_before":next,"next_offset":offset
                }),
            )])
            .await;
            let client = UsdtDepositClient::new(
                super::super::usdt_address(PHRASE.into(), None).unwrap(),
                url,
            )
            .unwrap();
            assert_eq!(
                client.history(1000, 100, PHRASE.into(), None).await.is_ok(),
                valid
            );
            let requests = server.await.unwrap();
            let signed: Value =
                serde_json::from_str(requests[0]["request"].as_str().unwrap()).unwrap();
            assert_eq!(
                signed["payload"],
                json!({"action":"history","before":1000,"offset":100})
            );
        }
    }

    #[tokio::test]
    async fn service_errors_preserve_recovery_actions() {
        for (code, expected) in [
            ("not_found", UsdtError::DepositNotFound),
            (
                "invalid_authorization",
                UsdtError::DepositAuthorizationRejected,
            ),
            ("clock_skew", UsdtError::ClockSkew),
            (
                "amount_too_small",
                UsdtError::DepositAmountOutOfRange {
                    min_usd_cents: None,
                    max_usd_cents: None,
                },
            ),
            (
                "amount_too_large",
                UsdtError::DepositAmountOutOfRange {
                    min_usd_cents: None,
                    max_usd_cents: None,
                },
            ),
            ("amount_exceeds_liquidity", UsdtError::UnsupportedRoute),
            ("operator_required", UsdtError::DepositNeedsAttention),
            ("provider_unavailable", UsdtError::NetworkUnavailable),
        ] {
            let (url, server) = service(vec![(
                400,
                json!({"error":code,"min_usd_cents":"200","max_usd_cents":"10000"}),
            )])
            .await;
            let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
            let client = UsdtDepositClient::new(owner, url).unwrap();
            let error = client.history(0, 0, PHRASE.into(), None).await.unwrap_err();
            assert_eq!(
                std::mem::discriminant(&error),
                std::mem::discriminant(&expected)
            );
            if let UsdtError::DepositAmountOutOfRange {
                min_usd_cents,
                max_usd_cents,
            } = error
            {
                assert_eq!(min_usd_cents.as_deref(), Some("200"));
                assert_eq!(max_usd_cents.as_deref(), Some("10000"));
            }
            server.await.unwrap();
        }
        for (status, expected) in [
            (429, UsdtError::RateLimited),
            (401, UsdtError::DepositAuthorizationRejected),
            (403, UsdtError::DepositAuthorizationRejected),
            (404, UsdtError::InvalidResponse),
            (502, UsdtError::NetworkUnavailable),
        ] {
            let (url, server) = service(vec![(status, json!({}))]).await;
            let client = UsdtDepositClient::new(
                super::super::usdt_address(PHRASE.into(), None).unwrap(),
                url,
            )
            .unwrap();
            let error = client.networks().await.unwrap_err();
            assert_eq!(
                std::mem::discriminant(&error),
                std::mem::discriminant(&expected)
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn optional_limits_preserve_receive_and_amount_errors() {
        for (limit, expected) in [
            (json!("200"), Some("200")),
            (Value::Null, None),
            (json!("unknown"), None),
            (json!(200), None),
            (json!(""), None),
            (json!("1".repeat(41)), None),
        ] {
            let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
            let (url, server) = service(vec![
                (200, json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":owner,"amount":"100000000","estimated_received":"98500000","min_usd_cents":limit,"max_usd_cents":limit})),
                (400, json!({"error":"amount_too_small","min_usd_cents":limit,"max_usd_cents":limit})),
            ]).await;
            let client = UsdtDepositClient::new(owner, url).unwrap();
            let received = client
                .receive(
                    UsdtDepositNetwork::Ethereum,
                    100_000_000,
                    PHRASE.into(),
                    None,
                )
                .await
                .unwrap();
            assert_eq!(received.min_usd_cents.as_deref(), expected);
            assert_eq!(received.max_usd_cents.as_deref(), expected);
            let UsdtError::DepositAmountOutOfRange {
                min_usd_cents,
                max_usd_cents,
            } = client
                .receive(
                    UsdtDepositNetwork::Ethereum,
                    100_000_000,
                    PHRASE.into(),
                    None,
                )
                .await
                .unwrap_err()
            else {
                panic!("Optional limits must not hide the amount error");
            };
            assert_eq!(min_usd_cents.as_deref(), expected);
            assert_eq!(max_usd_cents.as_deref(), expected);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn invalid_receive_terms_and_nonadvancing_pages_are_rejected() {
        let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
        let mut responses = vec![
            (
                200,
                json!({"network":"ethereum","address":UsdtDepositNetwork::Ethereum.evm_token().unwrap().1,"recipient":owner,"amount":"100000000","estimated_received":"98500000"}),
            ),
            (
                200,
                json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":"invalid","amount":"100000000","estimated_received":"98500000"}),
            ),
        ];
        for received in ["0", "100000001"] {
            responses.push((200, json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":owner,"amount":"100000000","estimated_received":received})));
        }
        let invalid_receives = responses.len();
        responses.push((200, json!({"deposits":[],"next_before":0,"next_offset":0})));
        let (url, server) = service(responses).await;
        let client = UsdtDepositClient::new(owner, url).unwrap();
        for _ in 0..invalid_receives {
            assert!(matches!(
                client
                    .receive(
                        UsdtDepositNetwork::Ethereum,
                        100_000_000,
                        PHRASE.into(),
                        None
                    )
                    .await,
                Err(UsdtError::InvalidResponse)
            ));
        }
        assert!(matches!(
            client.history(0, 0, PHRASE.into(), None).await,
            Err(UsdtError::InvalidResponse)
        ));
        server.await.unwrap();
    }
}
