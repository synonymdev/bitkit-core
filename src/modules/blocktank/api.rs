use crate::modules::blocktank::{BlocktankDB, BlocktankError, IGift};
use serde_json::json;
use std::time::Duration;

const REGTEST_TIMEOUT_SECS: u64 = 30;
const ERR_REGTEST_UNAUTHORIZED: &str =
    "Blocktank regtest API returned 401 Unauthorized: regtest auth is missing or was rejected";
use rust_blocktank_client::{
    CreateCjitOptions, CreateOrderOptions, IBt0ConfMinTxFeeWindow, IBtBolt11Invoice,
    IBtEstimateFeeResponse, IBtEstimateFeeResponse2, IBtInfo, IBtOrder, ICJitEntry,
};

impl BlocktankDB {
    /// Fetches service information from Blocktank and stores it in the database.
    /// Returns the fetched information if successful.
    pub async fn fetch_and_store_info(&self) -> Result<IBtInfo, BlocktankError> {
        let info = self
            .client
            .get_info()
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to fetch info from Blocktank: {}", e),
            })?;

        self.upsert_info(&info).await?;
        Ok(info)
    }

    /// Creates a new order and stores it in the database
    pub async fn create_and_store_order(
        &self,
        lsp_balance_sat: u64,
        channel_expiry_weeks: u32,
        options: Option<CreateOrderOptions>,
    ) -> Result<IBtOrder, BlocktankError> {
        let response = self
            .client
            .create_order(lsp_balance_sat, channel_expiry_weeks, options)
            .await;

        let order = response.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to create order with Blocktank client: {}", e),
        })?;

        self.upsert_order(&order).await?;
        Ok(order)
    }

    pub async fn open_channel(
        &self,
        order_id: String,
        connection_string: String,
    ) -> Result<IBtOrder, BlocktankError> {
        let response = self
            .client
            .open_channel(&order_id, &connection_string)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to open channel with Blocktank client: {}", e),
            })?;

        self.upsert_order(&response).await?;
        Ok(response)
    }

    /// Fetches and updates multiple orders in the database
    pub async fn refresh_orders(
        &self,
        order_ids: &[String],
    ) -> Result<Vec<IBtOrder>, BlocktankError> {
        let orders =
            self.client
                .get_orders(order_ids)
                .await
                .map_err(|e| BlocktankError::DataError {
                    error_details: format!("Failed to fetch orders: {}", e),
                })?;

        for order in &orders {
            self.upsert_order(order).await?;
        }

        Ok(orders)
    }

    /// Fetches and stores the active orders in the database
    pub async fn refresh_active_orders(&self) -> Result<Vec<IBtOrder>, BlocktankError> {
        let active_orders = self.get_active_orders().await?;

        if active_orders.is_empty() {
            return Ok(Vec::new());
        }

        let order_ids: Vec<String> = active_orders.iter().map(|order| order.id.clone()).collect();

        self.refresh_orders(&order_ids).await
    }

    pub async fn get_min_zero_conf_tx_fee(
        &self,
        order_id: String,
    ) -> Result<IBt0ConfMinTxFeeWindow, BlocktankError> {
        let response = self
            .client
            .get_min_zero_conf_tx_fee(&order_id)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to get minimum zero-conf transaction fee: {}", e),
            })?;

        Ok(response)
    }

    pub async fn estimate_order_fee(
        &self,
        lsp_balance_sat: u64,
        channel_expiry_weeks: u32,
        options: Option<CreateOrderOptions>,
    ) -> Result<IBtEstimateFeeResponse, BlocktankError> {
        let response = self
            .client
            .estimate_order_fee(lsp_balance_sat, channel_expiry_weeks, options)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to estimate order fee: {}", e),
            })?;

        Ok(response)
    }

    pub async fn estimate_order_fee_full(
        &self,
        lsp_balance_sat: u64,
        channel_expiry_weeks: u32,
        options: Option<CreateOrderOptions>,
    ) -> Result<IBtEstimateFeeResponse2, BlocktankError> {
        let response = self
            .client
            .estimate_order_fee_full(lsp_balance_sat, channel_expiry_weeks, options)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to estimate full order fee: {}", e),
            })?;

        Ok(response)
    }

    pub async fn create_cjit_entry(
        &self,
        channel_size_sat: u64,
        invoice_sat: u64,
        invoice_description: &str,
        node_id: &str,
        channel_expiry_weeks: u32,
        options: Option<CreateCjitOptions>,
    ) -> Result<ICJitEntry, BlocktankError> {
        let response = self
            .client
            .create_cjit_entry(
                channel_size_sat,
                invoice_sat,
                invoice_description,
                node_id,
                channel_expiry_weeks,
                options,
            )
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to create CJIT entry: {}", e),
            })?;

        self.upsert_cjit_entry(&response).await?;
        Ok(response)
    }

    /// Fetches a CJIT entry by ID from Blocktank and stores it in the database.
    /// Returns the fetched CJIT entry if successful.
    pub async fn refresh_cjit_entry(&self, entry_id: &str) -> Result<ICJitEntry, BlocktankError> {
        let response =
            self.client
                .get_cjit_entry(entry_id)
                .await
                .map_err(|e| BlocktankError::DataError {
                    error_details: format!("Failed to fetch CJIT entry from Blocktank: {}", e),
                })?;

        self.upsert_cjit_entry(&response).await?;
        Ok(response)
    }

    /// Fetches and stores the active CJIT entries in the database
    pub async fn refresh_active_cjit_entries(&self) -> Result<Vec<ICJitEntry>, BlocktankError> {
        let active_entries = self.get_active_cjit_entries().await?;

        if active_entries.is_empty() {
            return Ok(Vec::new());
        }

        let entry_ids: Vec<String> = active_entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect();

        // Since we don't have a bulk refresh method for CJIT entries,
        // we'll refresh them one by one
        let mut refreshed_entries = Vec::new();
        for entry_id in entry_ids {
            match self.refresh_cjit_entry(&entry_id).await {
                Ok(entry) => refreshed_entries.push(entry),
                Err(e) => {
                    log::warn!("Failed to refresh CJIT entry {}: {}", entry_id, e);
                    continue;
                }
            }
        }

        Ok(refreshed_entries)
    }

    /// Sends a request to Blocktank's `regtest/*` routes, with basic auth from a `user:password`
    /// string when given, and returns the response body.
    ///
    /// Temporary: `rust-blocktank-client` 0.0.16 can't send auth on these routes. Move back to it
    /// once a release that can is out (synonymdev/rust-blocktank-client#14).
    async fn regtest_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
        auth: Option<&str>,
    ) -> Result<String, String> {
        let url = regtest_url(&self.blocktank_url, path)?;
        let shown_url = redacted(&url);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(REGTEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
        let mut request = client.request(method, url);
        if let Some(auth) = auth.filter(|a| !a.is_empty()) {
            let (username, password) = auth.split_once(':').unwrap_or((auth, ""));
            request = request.basic_auth(username, Some(password));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("Request to {} failed: {}", shown_url, e.without_url()))?;
        let status = response.status();
        let text = response.text().await.map_err(|e| {
            format!(
                "Failed to read response from {}: {}",
                shown_url,
                e.without_url()
            )
        })?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ERR_REGTEST_UNAUTHORIZED.to_string());
        }
        if !status.is_success() {
            return Err(format!(
                "HTTP status {} from {}: {}",
                status, shown_url, text
            ));
        }
        Ok(text)
    }

    /// Mines blocks on the regtest network
    pub async fn regtest_mine(
        &self,
        count: Option<u32>,
        auth: Option<&str>,
    ) -> Result<(), BlocktankError> {
        let body = json!({ "count": count.unwrap_or(1) });
        self.regtest_request(reqwest::Method::POST, "chain/mine", Some(body), auth)
            .await
            .map(|_| ())
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to mine blocks: {}", e),
            })
    }

    /// Deposits satoshis to an address on the regtest network
    pub async fn regtest_deposit(
        &self,
        address: &str,
        amount_sat: Option<u64>,
        auth: Option<&str>,
    ) -> Result<String, BlocktankError> {
        let mut body = json!({ "address": address });
        if let Some(amount_sat) = amount_sat {
            body["amountSat"] = json!(amount_sat);
        }
        self.regtest_request(reqwest::Method::POST, "chain/deposit", Some(body), auth)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to deposit to address: {}", e),
            })
    }

    /// Pays an invoice on the regtest network
    pub async fn regtest_pay(
        &self,
        invoice: &str,
        amount_sat: Option<u64>,
        auth: Option<&str>,
    ) -> Result<String, BlocktankError> {
        let body = json!({ "invoice": invoice, "amountSat": amount_sat });
        self.regtest_request(reqwest::Method::POST, "channel/pay", Some(body), auth)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to pay invoice: {}", e),
            })
    }

    /// Gets paid invoice on the regtest network
    pub async fn regtest_get_payment(
        &self,
        payment_id: &str,
        auth: Option<&str>,
    ) -> Result<IBtBolt11Invoice, BlocktankError> {
        let path = format!("channel/pay/{}", payment_id);
        self.regtest_request(reqwest::Method::GET, &path, None, auth)
            .await
            .and_then(|text| {
                serde_json::from_str(&text)
                    .map_err(|e| format!("Failed to parse payment: {}. Response: {}", e, text))
            })
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to get payment: {}", e),
            })
    }

    /// Closes a channel on the regtest network
    pub async fn regtest_close_channel(
        &self,
        funding_tx_id: &str,
        vout: u32,
        force_close_after_s: Option<u64>,
        auth: Option<&str>,
    ) -> Result<String, BlocktankError> {
        let mut body = json!({ "fundingTxId": funding_tx_id, "vout": vout });
        if let Some(force_close_after_s) = force_close_after_s {
            body["forceCloseAfterSec"] = json!(force_close_after_s);
        }
        self.regtest_request(reqwest::Method::POST, "channel/close", Some(body), auth)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to close channel: {}", e),
            })
    }

    /// Registers a device with Blocktank
    pub async fn register_device(
        &self,
        device_token: &str,
        public_key: &str,
        features: &[String],
        node_id: &str,
        iso_timestamp: &str,
        signature: &str,
        is_production: Option<bool>,
        custom_url: Option<&str>,
    ) -> Result<String, BlocktankError> {
        self.client
            .register_device(
                device_token,
                public_key,
                features,
                node_id,
                iso_timestamp,
                signature,
                is_production,
                custom_url,
            )
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to register device: {}", e),
            })
    }

    /// Sends a test notification to a registered device
    pub async fn test_notification(
        &self,
        device_token: &str,
        secret_message: &str,
        notification_type: Option<&str>,
        custom_url: Option<&str>,
    ) -> Result<String, BlocktankError> {
        self.client
            .test_notification(device_token, secret_message, notification_type, custom_url)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to send test notification: {}", e),
            })
    }

    /// Makes a payment for a gift invoice
    pub async fn gift_pay(&self, invoice: &str) -> Result<IGift, BlocktankError> {
        self.client
            .gift_pay(invoice)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to pay gift invoice: {}", e),
            })
            .map(|gift| gift.into())
    }

    /// Creates a gift order
    pub async fn gift_order(
        &self,
        client_node_id: &str,
        code: &str,
    ) -> Result<IGift, BlocktankError> {
        self.client
            .gift_order(client_node_id, code)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to create gift order: {}", e),
            })
            .map(|gift| gift.into())
    }

    /// Gets a paid gift payment
    pub async fn get_gift(&self, gift_id: &str) -> Result<IGift, BlocktankError> {
        self.client
            .get_gift(gift_id)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to get gift: {}", e),
            })
            .map(|gift| gift.into())
    }

    /// Gets a paid payment
    pub async fn get_payment(&self, payment_id: &str) -> Result<IBtBolt11Invoice, BlocktankError> {
        self.client
            .get_payment(payment_id)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to get payment: {}", e),
            })
    }
}

/// Joins `regtest/<path>` onto the Blocktank base URL the way the Blocktank client does, so a
/// query string on the base URL is dropped instead of swallowing the path.
fn regtest_url(base_url: &str, path: &str) -> Result<url::Url, String> {
    let base = if base_url.ends_with('/') {
        base_url.to_string()
    } else {
        format!("{}/", base_url)
    };
    url::Url::parse(&base)
        .and_then(|base| base.join(&format!("regtest/{}", path)))
        .map_err(|e| format!("Invalid Blocktank URL: {}", e))
}

/// The URL without any credentials, for error messages.
fn redacted(url: &url::Url) -> String {
    let mut url = url.clone();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.to_string()
}

#[cfg(test)]
mod regtest_url_tests {
    use super::*;

    #[test]
    fn joins_regtest_path_and_drops_base_query() {
        let url = regtest_url("https://host/api/?x=1", "chain/mine").unwrap();
        assert_eq!(url.as_str(), "https://host/api/regtest/chain/mine");
        let url = regtest_url("https://host/blocktank/api/v2", "chain/deposit").unwrap();
        assert_eq!(
            url.as_str(),
            "https://host/blocktank/api/v2/regtest/chain/deposit"
        );
    }

    #[test]
    fn redacts_credentials_in_base_url() {
        let url = regtest_url("http://user:secret@127.0.0.1:1/api", "chain/mine").unwrap();
        let shown = redacted(&url);
        assert!(
            !shown.contains("secret") && !shown.contains("user"),
            "{}",
            shown
        );
    }

    #[tokio::test]
    async fn regtest_error_hides_base_url_password() {
        let db = BlocktankDB::new(":memory:", Some("http://user:secret@127.0.0.1:1/api"))
            .await
            .unwrap();
        let err = db.regtest_mine(None, None).await.unwrap_err().to_string();
        assert!(!err.contains("secret"), "{}", err);
    }
}
