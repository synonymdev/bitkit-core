use super::{
    keys::{derive_owner_key, parse_address},
    rpc::{bounded_json, endpoint_client},
    user_operation::sign_hash,
    UsdtDestination, UsdtError,
};
use alloy_primitives::{eip191_hash_message, Address};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

const TRON_USDT: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";

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
            || result.slippage_bps != 50
        {
            return Err(UsdtError::InvalidResponse);
        }
        if validate_source_address(&result.address, network)
            .map_err(|_| UsdtError::InvalidResponse)?
            == self.address
        {
            return Err(UsdtError::InvalidResponse);
        }
        result.uri = match network {
            UsdtDepositNetwork::Ethereum => format!(
                "ethereum:{}@1/transfer?address={}",
                UsdtDestination::Ethereum.token().to_checksum(None),
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
        let page: UsdtDepositPage = self
            .call(
                json!({"action":"history","offset":offset}),
                mnemonic.into(),
                passphrase.map(Into::into),
            )
            .await?;
        if page.next_offset.is_some_and(|next| next <= offset) {
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

fn validate_source_address(value: &str, network: UsdtDepositNetwork) -> Result<Address, UsdtError> {
    match network {
        UsdtDepositNetwork::Ethereum => {
            let address = parse_address(value)?;
            if address == UsdtDestination::Ethereum.token() {
                return Err(UsdtError::InvalidAddress);
            }
            Ok(address)
        }
        UsdtDepositNetwork::Tron => {
            if value.len() != 34 || !value.starts_with('T') || !value.is_ascii() {
                return Err(UsdtError::InvalidAddress);
            }
            let payload =
                bitcoin::base58::decode_check(value).map_err(|_| UsdtError::InvalidAddress)?;
            if payload.len() != 21
                || payload[0] != 0x41
                || payload[1..].iter().all(|byte| *byte == 0)
                || value == TRON_USDT
            {
                return Err(UsdtError::InvalidAddress);
            }
            Ok(Address::from_slice(&payload[1..]))
        }
    }
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
        let tron = "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8";
        for bad in [
            TRON_USDT,
            "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb",
            &"T".repeat(10000),
        ] {
            assert!(validate_source_address(bad, UsdtDepositNetwork::Tron).is_err());
        }
        assert!(validate_source_address(
            &UsdtDestination::Ethereum.token().to_checksum(None),
            UsdtDepositNetwork::Ethereum
        )
        .is_err());
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
    async fn service_responses_cover_receive_history_detail_and_refund() {
        let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
        let address = "0x1111111111111111111111111111111111111111";
        let deposit = json!({"id":"dep_one","network":"ethereum","asset":"USDT","source_tx":"0xsource","status":"held","code":null,"refund_tx":null});
        let (url, server) = service(vec![
            (200, json!({"networks":["ethereum","tron","bitcoin"]})),
            (200, json!({"network":"ethereum","address":address,"recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":50,"min_usd_cents":"200","max_usd_cents":"10000","uri":123})),
            (200, json!({"deposits":[deposit],"next_offset":50})),
            (200, json!({"deposit":deposit,"order":{"status":"held"}})),
            (202, json!({"status":"refund_requested"})),
        ]).await;
        let client = UsdtDepositClient::new(owner, url).unwrap();
        assert_eq!(
            client.networks().await.unwrap(),
            vec![UsdtDepositNetwork::Ethereum, UsdtDepositNetwork::Tron]
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
        let page = client.history(0, PHRASE.into(), None).await.unwrap();
        assert_eq!(page.deposits[0].amount, None);
        assert_eq!(page.next_offset, Some(50));
        let detail = client
            .detail("dep_one".into(), 0, PHRASE.into(), None)
            .await
            .unwrap();
        assert_eq!(detail.order.unwrap().amount_out, None);
        client
            .request_refund(
                "dep_one".into(),
                0,
                "0x2222222222222222222222222222222222222222".into(),
                UsdtDepositNetwork::Ethereum,
                PHRASE.into(),
                None,
            )
            .await
            .unwrap();
        let requests = server.await.unwrap();
        let expected = [
            json!({"action":"receive","network":"ethereum","amount":"100000000"}),
            json!({"action":"history","offset":0}),
            json!({"action":"detail","depositId":"dep_one","offset":0}),
            json!({"action":"refund","depositId":"dep_one","offset":0,"refundAddress":"0x2222222222222222222222222222222222222222"}),
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
            (
                "standing_tron_refund_requires_operator",
                UsdtError::DepositNeedsAttention,
            ),
            ("provider_unavailable", UsdtError::NetworkUnavailable),
        ] {
            let (url, server) = service(vec![(
                400,
                json!({"error":code,"min_usd_cents":"200","max_usd_cents":"10000"}),
            )])
            .await;
            let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
            let client = UsdtDepositClient::new(owner, url).unwrap();
            let error = client.history(0, PHRASE.into(), None).await.unwrap_err();
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
                (200, json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":50,"min_usd_cents":limit,"max_usd_cents":limit})),
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
                json!({"network":"ethereum","address":UsdtDestination::Ethereum.token(),"recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":50}),
            ),
            (
                200,
                json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":"invalid","amount":"100000000","estimated_received":"98500000","slippage_bps":50}),
            ),
        ];
        for slippage_bps in [0, 49, 51] {
            responses.push((200, json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":slippage_bps})));
        }
        let invalid_receives = responses.len();
        responses.push((200, json!({"deposits":[],"next_offset":0})));
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
            client.history(0, PHRASE.into(), None).await,
            Err(UsdtError::InvalidResponse)
        ));
        server.await.unwrap();
    }
}
