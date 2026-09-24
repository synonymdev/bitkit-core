use super::{UsdtError, UsdtTransfer, UsdtTransferStatus};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_sol_types::SolCall;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const REQUEST_INTERVAL: Duration = Duration::from_millis(750);
const BURST_WINDOW: Duration = Duration::from_millis(750 * 19);

#[derive(Deserialize)]
pub(super) struct Block {
    pub hash: B256,
    pub timestamp: U256,
    pub transactions: Vec<B256>,
}

pub(super) struct Rpc {
    client: reqwest::Client,
    #[cfg(test)]
    pub(super) bridge_status_url: Option<String>,
    url: String,
    chain_id: u64,
    next_request: Arc<Mutex<Instant>>,
}

#[derive(Deserialize)]
struct Response {
    result: Option<Value>,
    error: Option<RpcError>,
}
#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl Rpc {
    pub fn new(url: String, chain_id: u64) -> Result<Self, UsdtError> {
        let client = endpoint_client(&url, Duration::from_secs(25))?;
        Ok(Self {
            client,
            #[cfg(test)]
            bridge_status_url: None,
            url,
            chain_id,
            next_request: Arc::new(Mutex::new(Instant::now())),
        })
    }

    pub fn with_url(&self, url: String) -> Result<Self, UsdtError> {
        let mut rpc = Self::new(url, self.chain_id)?;
        rpc.next_request = self.next_request.clone();
        Ok(rpc)
    }

    pub async fn verify_chain(&self) -> Result<(), UsdtError> {
        let chain: U256 = self.call("eth_chainId", json!([])).await?;
        if chain != U256::from(self.chain_id) {
            return Err(UsdtError::WrongNetwork);
        }
        Ok(())
    }

    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, UsdtError> {
        self.reserve_request().await;
        let response = self
            .client
            .post(&self.url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| UsdtError::NetworkUnavailable)?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(UsdtError::RateLimited);
        }
        let status = response.status();
        let overflow = if method == "eth_getLogs" {
            UsdtError::LogRangeTooLarge
        } else {
            UsdtError::InvalidResponse
        };
        // The proxy projects receipts to fixed-size USDT protocol events.
        let limit = if method == "eth_getTransactionReceipt" {
            16 * 1024 * 1024
        } else {
            2_097_152
        };
        if status.is_server_error() {
            return Err(UsdtError::NetworkUnavailable);
        }
        let body = bounded_json(response, limit, overflow).await;
        let response: Response =
            match body.and_then(|value| serde_json::from_value(value).map_err(Into::into)) {
                Ok(response) => response,
                Err(_) if !status.is_success() => return Err(UsdtError::NetworkUnavailable),
                Err(error) => return Err(error),
            };
        if let Some(error) = response.error {
            let message = error.message.to_ascii_lowercase();
            if error.code == -32016
                || [
                    "rate limit",
                    "quota",
                    "credit",
                    "too many requests",
                    "requests per",
                    "compute units",
                ]
                .iter()
                .any(|term| message.contains(term))
            {
                return Err(UsdtError::RateLimited);
            }
            if method == "eth_getLogs" && error.code == -32005 {
                return Err(UsdtError::LogRangeTooLarge);
            }
            if error.code == -32002 {
                return Err(UsdtError::NetworkUnavailable);
            }
            if matches!(
                method,
                "eth_chainId"
                    | "eth_blockNumber"
                    | "eth_getCode"
                    | "eth_getBalance"
                    | "eth_getTransactionCount"
                    | "eth_call"
                    | "eth_getLogs"
                    | "eth_getBlockByNumber"
                    | "eth_getTransactionReceipt"
                    | "eth_getTransactionByHash"
            ) {
                return Err(UsdtError::NetworkUnavailable);
            }
            return Err(UsdtError::TransactionRejected {
                reason: error.message.chars().take(200).collect(),
            });
        }
        if !status.is_success() {
            return Err(UsdtError::NetworkUnavailable);
        }
        serde_json::from_value(response.result.unwrap_or(Value::Null)).map_err(Into::into)
    }

    async fn reserve_request(&self) {
        let mut next = self.next_request.lock().await;
        let scheduled = (*next).max(Instant::now());
        // Allow 20 requests in a burst, then 80/minute across chain and bundler calls.
        tokio::time::sleep_until(scheduled - BURST_WINDOW).await;
        *next = scheduled + REQUEST_INTERVAL;
    }

    pub async fn bridge_status(
        &self,
        transfer: &UsdtTransfer,
    ) -> Result<UsdtTransferStatus, UsdtError> {
        if transfer.bridge_guid.is_none() {
            return Ok(transfer.status);
        }
        let base = "https://scan.layerzero-api.com";
        #[cfg(test)]
        let base = self.bridge_status_url.as_deref().unwrap_or(base);
        let url = format!("{base}/v1/messages/tx/{}", transfer.tx_hash);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| UsdtError::NetworkUnavailable)?
            .error_for_status()
            .map_err(|_| UsdtError::NetworkUnavailable)?;
        let response = bounded_json(response, 2_097_152, UsdtError::InvalidResponse).await?;
        let messages = response["data"]
            .as_array()
            .ok_or(UsdtError::InvalidResponse)?;
        let message = messages.iter().find(|message| {
            message["guid"].as_str().is_some_and(|guid| {
                transfer
                    .bridge_guid
                    .as_ref()
                    .is_some_and(|expected| guid.eq_ignore_ascii_case(expected))
            }) && message["pathway"]["srcEid"].as_u64() == Some(30110)
                && message["pathway"]["dstEid"].as_u64()
                    == transfer.destination.endpoint().map(u64::from)
                && message["pathway"]["sender"]["address"]
                    .as_str()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&super::types::OFT.to_string()))
                && message["source"]["tx"]["txHash"]
                    .as_str()
                    .is_some_and(|h| h.eq_ignore_ascii_case(&transfer.tx_hash))
        });
        Ok(match message.and_then(|m| m["status"]["name"].as_str()) {
            Some("DELIVERED") => UsdtTransferStatus::Confirmed,
            Some(
                "FAILED"
                | "BLOCKED"
                | "PAYLOAD_STORED"
                | "APPLICATION_BURNED"
                | "APPLICATION_SKIPPED",
            ) => UsdtTransferStatus::BridgeNeedsAttention,
            _ => transfer.status,
        })
    }

    pub async fn balance(&self, address: Address) -> Result<U256, UsdtError> {
        self.call("eth_getBalance", json!([address, "pending"]))
            .await
    }

    pub async fn block(&self, number: u64) -> Result<Block, UsdtError> {
        self.call("eth_getBlockByNumber", json!([U256::from(number), false]))
            .await
    }

    pub async fn block_receipt(
        &self,
        hash: B256,
        block: &Block,
        number: u64,
    ) -> Result<Value, UsdtError> {
        let receipt: Value = self
            .call("eth_getTransactionReceipt", json!([hash]))
            .await?;
        if receipt.is_null() {
            return Err(UsdtError::NetworkUnavailable);
        }
        if serde_json::from_value::<B256>(receipt["transactionHash"].clone())? != hash
            || serde_json::from_value::<B256>(receipt["blockHash"].clone())? != block.hash
            || serde_json::from_value::<U256>(receipt["blockNumber"].clone())? != U256::from(number)
        {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(receipt)
    }

    pub async fn contract<C: SolCall>(&self, to: Address, call: C) -> Result<C::Return, UsdtError> {
        let bytes: Bytes = self
            .call(
                "eth_call",
                json!([{"to":to,"data":Bytes::from(call.abi_encode())},"latest"]),
            )
            .await?;
        C::abi_decode_returns(&bytes).map_err(|_| UsdtError::InvalidResponse)
    }
}

pub(super) fn endpoint_client(url: &str, timeout: Duration) -> Result<reqwest::Client, UsdtError> {
    let parsed = url::Url::parse(url).map_err(|_| UsdtError::NotConfigured)?;
    if (parsed.scheme() != "https"
        && !(parsed.scheme() == "http"
            && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"))))
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(UsdtError::NotConfigured);
    }
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| UsdtError::NetworkUnavailable)
}

pub(super) async fn bounded_json(
    mut response: reqwest::Response,
    limit: usize,
    overflow: UsdtError,
) -> Result<Value, UsdtError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| UsdtError::NetworkUnavailable)?
    {
        if body.len() + chunk.len() > limit {
            return Err(overflow);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_reject_embedded_credentials_and_insecure_remote_hosts() {
        for url in [
            "http://example.com",
            "https://user:key@example.com",
            "https://example.com?apikey=secret",
            "https://example.com#secret",
            "not a url",
        ] {
            assert!(matches!(
                Rpc::new(url.into(), 42161),
                Err(UsdtError::NotConfigured)
            ));
        }
        assert!(Rpc::new("https://example.com/v1/usdt/chain-rpc".into(), 42161).is_ok());
        assert!(Rpc::new("http://127.0.0.1:3100/v1/usdt/chain-rpc".into(), 42161).is_ok());
    }

    #[tokio::test]
    async fn http_errors_preserve_structured_rejections_and_network_failures() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body) in [
            (
                400,
                r#"{"error":{"code":-32602,"message":"Unsupported USDT request"}}"#,
            ),
            (
                502,
                r#"{"error":{"code":-32002,"message":"Provider unavailable"}}"#,
            ),
            (503, "Service unavailable"),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 4096];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                let response = format!("HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let error = Rpc::new(url, 42161)
                .unwrap()
                .call::<Value>("eth_estimateUserOperationGas", json!([]))
                .await
                .unwrap_err();
            if status == 400 {
                assert!(matches!(error, UsdtError::TransactionRejected { .. }));
            } else {
                assert!(matches!(error, UsdtError::NetworkUnavailable));
            }
            server.await.unwrap();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn chain_and_bundler_share_bursts_and_sustained_budget() {
        let chain = Rpc::new("https://chain.example".into(), 42161).unwrap();
        let bundler = chain.with_url("https://bundler.example".into()).unwrap();
        let start = Instant::now();
        for _ in 0..10 {
            chain.reserve_request().await;
            bundler.reserve_request().await;
        }
        assert_eq!(Instant::now(), start);
        for _ in 0..80 {
            chain.reserve_request().await;
        }
        assert_eq!(Instant::now() - start, Duration::from_secs(60));
        tokio::time::sleep(Duration::from_secs(30)).await;
        let idle = Instant::now();
        for _ in 0..20 {
            bundler.reserve_request().await;
        }
        assert_eq!(Instant::now(), idle);
        chain.reserve_request().await;
        assert_eq!(Instant::now() - idle, REQUEST_INTERVAL);
    }
}
