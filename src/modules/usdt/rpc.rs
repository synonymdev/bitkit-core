use super::{UsdtError, UsdtTransfer, UsdtTransferStatus};
use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const REQUEST_INTERVAL: Duration = Duration::from_millis(750);
const BURST_WINDOW: Duration = Duration::from_millis(750 * 19);

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
        let parsed = url::Url::parse(&url).map_err(|_| UsdtError::NetworkUnavailable)?;
        if parsed.scheme() != "https"
            && !(parsed.scheme() == "http"
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")))
        {
            return Err(UsdtError::NetworkUnavailable);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| UsdtError::NetworkUnavailable)?;
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
        let response = response
            .error_for_status()
            .map_err(|_| UsdtError::NetworkUnavailable)?;
        let overflow = if method == "eth_getLogs" {
            UsdtError::LogRangeTooLarge
        } else {
            UsdtError::InvalidResponse
        };
        let response: Response =
            serde_json::from_value(Self::bounded_json(response, overflow).await?)?;
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
            if message.contains("insufficient funds") || message.contains("insufficient balance") {
                return Err(UsdtError::InsufficientBalance);
            }
            return Err(UsdtError::TransactionRejected {
                reason: error.message.chars().take(200).collect(),
            });
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
        let url = format!(
            "https://scan.layerzero-api.com/v1/messages/tx/{}",
            transfer.tx_hash
        );
        #[cfg(test)]
        let url = self
            .bridge_status_url
            .as_ref()
            .map(|base| format!("{base}/{}", transfer.tx_hash))
            .unwrap_or(url);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| UsdtError::NetworkUnavailable)?
            .error_for_status()
            .map_err(|_| UsdtError::NetworkUnavailable)?;
        let response = Self::bounded_json(response, UsdtError::InvalidResponse).await?;
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

    async fn bounded_json(
        mut response: reqwest::Response,
        overflow: UsdtError,
    ) -> Result<Value, UsdtError> {
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| UsdtError::NetworkUnavailable)?
        {
            if body.len() + chunk.len() > 2_097_152 {
                return Err(overflow);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(Into::into)
    }

    pub async fn balance(&self, address: Address) -> Result<U256, UsdtError> {
        self.call("eth_getBalance", json!([address, "pending"]))
            .await
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

#[cfg(test)]
mod tests {
    use super::*;

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
