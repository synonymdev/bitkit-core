use rust_blocktank_client::{
    CreateCjitOptions,
    CreateOrderOptions,
    IBt0ConfMinTxFeeWindow,
    IBtBolt11Invoice,
    IBtEstimateFeeResponse,
    IBtEstimateFeeResponse2,
    IBtInfo,
    IBtOrder,
    ICJitEntry
};
use crate::modules::blocktank::{BlocktankDB, BlocktankError};

impl BlocktankDB {
    /// Fetches service information from Blocktank and stores it in the database.
    /// Returns the fetched information if successful.
    pub async fn fetch_and_store_info(&self) -> Result<IBtInfo, BlocktankError> {
        let info = self.client.get_info().await.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to fetch info from Blocktank: {}", e)
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
        let response = self.client.create_order(
            lsp_balance_sat,
            channel_expiry_weeks,
            options
        ).await;

        println!("Raw API response: {:#?}", response);

        let order = response.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to create order with Blocktank client: {}", e)
        })?;

        self.upsert_order(&order).await?;
        Ok(order)
    }

    pub async fn open_channel(
        &self,
        order_id: String,
        connection_string: String
    ) -> Result<IBtOrder, BlocktankError> {
        let response = self.client.open_channel(
            &order_id,
            &connection_string,
        ).await.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to open channel with Blocktank client: {}", e)
        })?;

        self.upsert_order(&response).await?;
        Ok(response)
    }

    /// Fetches and updates multiple orders in the database
    pub async fn refresh_orders(&self, order_ids: &[String]) -> Result<Vec<IBtOrder>, BlocktankError> {
        let orders = self.client.get_orders(order_ids)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to fetch orders: {}", e)
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

        let order_ids: Vec<String> = active_orders
            .iter()
            .map(|order| order.id.clone())
            .collect();

        self.refresh_orders(&order_ids).await
    }

    pub async fn get_min_zero_conf_tx_fee(
        &self,
        order_id: String,
    ) -> Result<IBt0ConfMinTxFeeWindow, BlocktankError> {
        let response = self.client.get_min_zero_conf_tx_fee(&order_id)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to get minimum zero-conf transaction fee: {}", e)
            })?;

        Ok(response)
    }

    pub async fn estimate_order_fee(
        &self,
        lsp_balance_sat: u64,
        channel_expiry_weeks: u32,
        options: Option<CreateOrderOptions>,
    ) -> Result<IBtEstimateFeeResponse, BlocktankError> {
        let response = self.client.estimate_order_fee(
            lsp_balance_sat,
            channel_expiry_weeks,
            options
        ).await.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to estimate order fee: {}", e)
        })?;

        Ok(response)
    }

    pub async fn estimate_order_fee_full(
        &self,
        lsp_balance_sat: u64,
        channel_expiry_weeks: u32,
        options: Option<CreateOrderOptions>,
    ) -> Result<IBtEstimateFeeResponse2, BlocktankError> {
        let response = self.client.estimate_order_fee_full(
            lsp_balance_sat,
            channel_expiry_weeks,
            options
        ).await.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to estimate full order fee: {}", e)
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
        let response = self.client.create_cjit_entry(
            channel_size_sat,
            invoice_sat,
            invoice_description,
            node_id,
            channel_expiry_weeks,
            options
        ).await.map_err(|e| BlocktankError::DataError {
            error_details: format!("Failed to create CJIT entry: {}", e)
        })?;

        self.upsert_cjit_entry(&response).await?;
        Ok(response)
    }

    /// Fetches a CJIT entry by ID from Blocktank and stores it in the database.
    /// Returns the fetched CJIT entry if successful.
    pub async fn refresh_cjit_entry(&self, entry_id: &str) -> Result<ICJitEntry, BlocktankError> {
        let response = self.client.get_cjit_entry(entry_id)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to fetch CJIT entry from Blocktank: {}", e)
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
                    println!("Warning: Failed to refresh CJIT entry {}: {}", entry_id, e);
                    continue;
                }
            }
        }

        Ok(refreshed_entries)
    }

    /// Mines blocks on the regtest network
    pub async fn regtest_mine(&self, count: Option<u32>) -> Result<(), BlocktankError> {
        self.client.regtest_mine(count)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to mine blocks: {}", e)
            })
    }

    /// Deposits satoshis to an address on the regtest network
    pub async fn regtest_deposit(
        &self,
        address: &str,
        amount_sat: Option<u64>,
    ) -> Result<String, BlocktankError> {
        self.client.regtest_deposit(address, amount_sat)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to deposit to address: {}", e)
            })
    }

    /// Pays an invoice on the regtest network
    pub async fn regtest_pay(
        &self,
        invoice: &str,
        amount_sat: Option<u64>,
    ) -> Result<String, BlocktankError> {
        self.client.regtest_pay(invoice, amount_sat)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to pay invoice: {}", e)
            })
    }

    /// Gets paid invoice on the regtest network
    pub async fn regtest_get_payment(&self, payment_id: &str) -> Result<IBtBolt11Invoice, BlocktankError> {
        self.client.regtest_get_payment(payment_id)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to get payment: {}", e)
            })
    }

    /// Closes a channel on the regtest network
    pub async fn regtest_close_channel(
        &self,
        funding_tx_id: &str,
        vout: u32,
        force_close_after_s: Option<u64>,
    ) -> Result<String, BlocktankError> {
        self.client.regtest_close_channel(funding_tx_id, vout, force_close_after_s)
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to close channel: {}", e)
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
        custom_url: Option<&str>,
    ) -> Result<String, BlocktankError> {
        self.client.register_device(
            device_token,
            public_key,
            features,
            node_id,
            iso_timestamp,
            signature,
            custom_url
        )
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to register device: {}", e)
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
        self.client.test_notification(
            device_token,
            secret_message,
            notification_type,
            custom_url
        )
            .await
            .map_err(|e| BlocktankError::DataError {
                error_details: format!("Failed to send test notification: {}", e)
            })
    }
}