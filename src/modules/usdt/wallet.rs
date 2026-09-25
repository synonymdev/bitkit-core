use super::{
    account::{validate_delegation, ENTRY_POINT},
    amount::token_amount,
    keys::{derive_key, parse_address},
    paymaster::{Pimlico, PAYMASTER},
    rpc::Rpc,
    store::{QuoteData, Store},
    transaction::{event_data, EntryPoint, Erc20, Paymaster, Plan},
    types::{CHAIN_ID, EXPLORER, TOKEN},
    user_operation::Authorization,
    UsdtError, UsdtQuote, UsdtTransfer, UsdtTransferStatus,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_sol_types::{SolCall, SolEvent};
use serde_json::{json, Value};
use std::sync::{atomic::AtomicU64, Arc};
use tokio::sync::Mutex;

#[derive(uniffi::Object)]
pub struct UsdtWallet {
    pub(super) address: Address,
    pub(super) rpc: Rpc,
    pub(super) paymaster: Pimlico,
    pub(super) store: Store,
    operation: Mutex<()>,
    pub(super) history_range_limit: AtomicU64,
}

#[uniffi::export(async_runtime = "tokio")]
impl UsdtWallet {
    #[uniffi::constructor]
    pub fn new(
        address: String,
        storage_path: String,
        rpc_url: String,
        bundler_url: String,
    ) -> Result<Arc<Self>, UsdtError> {
        if rpc_url.is_empty() || bundler_url.is_empty() {
            return Err(UsdtError::NotConfigured);
        }
        let address = parse_address(&address)?;
        let rpc = Rpc::new(rpc_url, CHAIN_ID)?;
        let paymaster = Pimlico {
            rpc: rpc.with_url(bundler_url)?,
        };
        let store = Store::open(&storage_path, &format!("{CHAIN_ID}:{}", address))?;
        Ok(Arc::new(Self {
            address,
            rpc,
            paymaster,
            store,
            operation: Mutex::new(()),
            history_range_limit: AtomicU64::new(super::history::MAX_LOG_RANGE),
        }))
    }

    pub fn receive_address(&self) -> String {
        self.address.to_checksum(None)
    }

    pub fn receive_uri(&self) -> String {
        format!(
            "ethereum:{}@{CHAIN_ID}/transfer?address={}",
            TOKEN.to_checksum(None),
            self.receive_address()
        )
    }

    pub async fn balance(&self) -> Result<u64, UsdtError> {
        self.rpc.verify_chain().await?;
        token_amount(self.token_balance().await?)
    }

    pub async fn quote_transfer(
        &self,
        recipient: String,
        amount: u64,
    ) -> Result<UsdtQuote, UsdtError> {
        if amount == 0 {
            return Err(UsdtError::InvalidAmount);
        }
        let recipient = parse_address(recipient.trim())?;
        if [
            self.address,
            TOKEN,
            ENTRY_POINT,
            PAYMASTER,
            super::account::DELEGATE,
        ]
        .contains(&recipient)
        {
            return Err(UsdtError::InvalidAddress);
        }
        self.store.require_no_pending()?;
        self.rpc.verify_chain().await?;
        self.require_balance(amount, 0).await?;
        let calls = vec![(
            TOKEN,
            Erc20::transferCall {
                recipient,
                amount: U256::from(amount),
            }
            .abi_encode()
            .into(),
        )];
        let nonce = self.nonce("latest").await?;
        let authorization = self.authorization().await?;
        let created_block = self.block_number().await?;
        let timestamp = self.block_timestamp(created_block).await?;
        let (operation, maximum_fee, operation_expires_at) = self
            .paymaster
            .prepare(self.address, nonce, authorization, &calls, timestamp)
            .await?;
        let expires_at =
            now().saturating_add(operation_expires_at.saturating_sub(timestamp).min(120));
        self.require_balance(amount, maximum_fee).await?;
        let quote = UsdtQuote {
            id: uuid::Uuid::new_v4().to_string(),
            recipient: recipient.to_checksum(None),
            amount,
            maximum_fee,
            expires_at,
        };
        self.store.save_quote(&QuoteData {
            quote: quote.clone(),
            plan: Plan {
                operation,
                created_block,
                expires_at: operation_expires_at,
            },
        })?;
        Ok(quote)
    }

    pub async fn send(
        &self,
        quote_id: String,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<UsdtTransfer, UsdtError> {
        let mnemonic = zeroize::Zeroizing::new(mnemonic);
        let passphrase = passphrase.map(zeroize::Zeroizing::new);
        let _guard = self.operation.lock().await;
        if let Some(existing) = self.store.transfer(&quote_id)? {
            let key = derive_key(mnemonic, passphrase)?;
            if super::keys::key_address(&key) != self.address {
                return Err(UsdtError::InvalidCredentials);
            }
            return Ok(existing);
        }
        self.store.require_no_pending()?;
        let mut data = self.store.quote(&quote_id)?;
        if data.quote.expires_at <= now() + 5 {
            return Err(UsdtError::QuoteExpired);
        }
        self.rpc.verify_chain().await?;
        if self.nonce("latest").await? != data.plan.operation.nonce {
            return Err(UsdtError::QuoteExpired);
        }
        let authorization = self.authorization().await?;
        if authorization.nonce != data.plan.operation.eip7702_auth.nonce {
            return Err(UsdtError::QuoteExpired);
        }
        self.require_balance(data.quote.amount, data.quote.maximum_fee)
            .await?;
        self.paymaster.validate_gas(&data.plan.operation).await?;
        if self
            .block_timestamp(self.block_number().await?)
            .await?
            .saturating_add(5)
            >= data.plan.expires_at
        {
            return Err(UsdtError::QuoteExpired);
        }
        let key = derive_key(mnemonic, passphrase)?;
        if super::keys::key_address(&key) != self.address {
            return Err(UsdtError::InvalidCredentials);
        }
        if data.quote.expires_at <= now() + 5 {
            return Err(UsdtError::QuoteExpired);
        }
        let (hash, raw) = data.plan.sign(&key)?;
        drop(key);
        let mut transfer = UsdtTransfer {
            id: quote_id,
            tx_hash: String::new(),
            user_operation_hash: Some(format!("{hash:#x}")),
            recipient: data.quote.recipient,
            amount: data.quote.amount,
            received_amount: data.quote.amount,
            fee: None,
            is_incoming: false,
            status: UsdtTransferStatus::Pending,
            timestamp: now(),
            explorer_url: String::new(),
        };
        self.store.record_signed(&transfer, &raw)?;
        // After persistence a lost response is indeterminate. Retry only the identical signed operation.
        if let Err(error) = self.broadcast(&data.plan, hash).await {
            // These errors occur before submission; later retries may already be queued.
            if matches!(
                error,
                UsdtError::QuoteExpired | UsdtError::UnsupportedDelegation
            ) {
                transfer.status = UsdtTransferStatus::Failed;
                transfer.received_amount = 0;
                transfer.fee = Some(0);
                self.store.update_transfer(&transfer)?;
                return Err(error);
            }
        }
        Ok(transfer)
    }

    pub async fn sync_history(&self) -> Result<bool, UsdtError> {
        let _guard = self.operation.lock().await;
        self.rpc.verify_chain().await?;
        self.scan_history().await
    }

    pub fn history(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        self.store.transfers()
    }

    pub async fn refresh_transfers(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        let _guard = self.operation.lock().await;
        let transfers = self.store.unsettled()?;
        if transfers.is_empty() {
            return self.history();
        }
        self.rpc.verify_chain().await?;
        for mut transfer in transfers {
            let Some(plan) = self.store.pending_plan(&transfer.id)? else {
                continue;
            };
            let hash = plan.operation.hash(CHAIN_ID)?;
            let confirmed_tip = self.block_number().await?.saturating_sub(2);
            if confirmed_tip < plan.created_block {
                continue;
            }
            let end = self.pending_search_end(&plan, confirmed_tip).await?;
            let logs: Vec<Value> = match self.rpc.call("eth_getLogs", json!([{
                "address": ENTRY_POINT, "fromBlock": U256::from(plan.created_block), "toBlock": U256::from(end),
                "topics": [EntryPoint::UserOperationEvent::SIGNATURE_HASH, hash, self.address.into_word()]
            }])).await {
                Ok(logs) => logs,
                // The nonce-based lookup below verifies the consuming event within a single block.
                Err(UsdtError::LogRangeTooLarge) => Vec::new(),
                Err(error) => return Err(error),
            };
            if let Some(log) = logs
                .iter()
                .find(|log| log["removed"].as_bool() != Some(true))
            {
                let event = EntryPoint::UserOperationEvent::decode_log_data(&event_data(log)?)
                    .map_err(|_| UsdtError::InvalidResponse)?;
                if serde_json::from_value::<Address>(log["address"].clone())? != ENTRY_POINT
                    || event.userOpHash != hash
                    || event.nonce != plan.operation.nonce
                {
                    return Err(UsdtError::InvalidResponse);
                }
                self.settle_from_log(&mut transfer, log, event).await?;
            } else {
                let nonce = self.nonce(&format!("0x{confirmed_tip:x}")).await?;
                if nonce > plan.operation.nonce {
                    // A nonce advance alone cannot distinguish this payment from a replacement.
                    let block = self.nonce_consumed_block(&plan, confirmed_tip).await?;
                    let candidates: Vec<Value> = match self.rpc.call("eth_getLogs", json!([{
                        "address": ENTRY_POINT, "fromBlock": U256::from(block), "toBlock": U256::from(block),
                        "topics": [EntryPoint::UserOperationEvent::SIGNATURE_HASH, null, self.address.into_word()]
                    }])).await {
                        Ok(logs) => logs,
                        Err(UsdtError::LogRangeTooLarge) => Vec::new(),
                        Err(error) => return Err(error),
                    };
                    let mut matched = false;
                    for log in candidates
                        .iter()
                        .filter(|log| log["removed"].as_bool() != Some(true))
                    {
                        let event =
                            EntryPoint::UserOperationEvent::decode_log_data(&event_data(log)?)
                                .map_err(|_| UsdtError::InvalidResponse)?;
                        if serde_json::from_value::<Address>(log["address"].clone())? != ENTRY_POINT
                            || u64::try_from(serde_json::from_value::<U256>(
                                log["blockNumber"].clone(),
                            )?)
                            .map_err(|_| UsdtError::InvalidResponse)?
                                != block
                            || event.sender != self.address
                        {
                            return Err(UsdtError::InvalidResponse);
                        }
                        if event.nonce != plan.operation.nonce {
                            continue;
                        }
                        if event.userOpHash == hash {
                            self.settle_from_log(&mut transfer, log, event).await?;
                        } else {
                            self.reconcile_consumed_nonce(&mut transfer, &plan, block)
                                .await?;
                        }
                        matched = true;
                        break;
                    }
                    if !matched {
                        self.reconcile_consumed_nonce(&mut transfer, &plan, block)
                            .await?;
                    }
                } else {
                    let expired = self.block_timestamp(confirmed_tip).await? > plan.expires_at;
                    if expired {
                        transfer.status = UsdtTransferStatus::Failed;
                        transfer.received_amount = 0;
                        transfer.fee = Some(0);
                        self.store.update_transfer(&transfer)?;
                    } else {
                        let _ = self.broadcast(&plan, hash).await;
                    }
                }
            }
        }
        self.history()
    }
}

impl UsdtWallet {
    async fn reconcile_consumed_nonce(
        &self,
        transfer: &mut UsdtTransfer,
        plan: &Plan,
        number: u64,
    ) -> Result<(), UsdtError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        let block = self.rpc.block(number).await?;
        let block_hash = format!("{:#x}", block.hash);
        let start = self.store.nonce_recovery(&transfer.id, &block_hash)?;
        if start > block.transactions.len() {
            return Err(UsdtError::InvalidResponse);
        }
        for (index, hash) in block.transactions.iter().enumerate().skip(start) {
            if tokio::time::Instant::now() >= deadline {
                return Ok(());
            }
            let receipt = self.rpc.block_receipt(*hash, block.hash, number).await?;
            for log in receipt["logs"]
                .as_array()
                .ok_or(UsdtError::InvalidResponse)?
            {
                if serde_json::from_value::<Address>(log["address"].clone())? != ENTRY_POINT {
                    continue;
                }
                let Ok(event) = EntryPoint::UserOperationEvent::decode_log_data(&event_data(log)?)
                else {
                    continue;
                };
                if event.sender != self.address || event.nonce != plan.operation.nonce {
                    continue;
                }
                if event.userOpHash == plan.operation.hash(CHAIN_ID)? {
                    if event.paymaster != PAYMASTER {
                        return Err(UsdtError::InvalidResponse);
                    }
                    transfer.tx_hash = format!("{hash:#x}");
                    transfer.explorer_url = format!("{EXPLORER}/tx/{hash:#x}");
                    self.settle(transfer, &receipt)?;
                } else {
                    transfer.status = UsdtTransferStatus::Replaced;
                    transfer.received_amount = 0;
                    transfer.fee = Some(0);
                }
                transfer.timestamp =
                    u64::try_from(block.timestamp).map_err(|_| UsdtError::InvalidResponse)?;
                return self.store.update_transfer(transfer);
            }
            self.store
                .save_nonce_recovery(&transfer.id, &block_hash, index + 1)?;
        }
        // Empty indexed logs are insufficient; every receipt in the consuming block must be checked.
        if self.rpc.block(number).await?.hash != block.hash {
            return Err(UsdtError::NetworkUnavailable);
        }
        transfer.status = UsdtTransferStatus::Replaced;
        transfer.received_amount = 0;
        transfer.fee = Some(0);
        transfer.timestamp =
            u64::try_from(block.timestamp).map_err(|_| UsdtError::InvalidResponse)?;
        self.store.update_transfer(transfer)
    }

    pub(super) async fn block_number(&self) -> Result<u64, UsdtError> {
        u64::try_from(self.rpc.call::<U256>("eth_blockNumber", json!([])).await?)
            .map_err(|_| UsdtError::InvalidResponse)
    }
    pub(super) async fn block_timestamp(&self, number: u64) -> Result<u64, UsdtError> {
        u64::try_from(self.rpc.block(number).await?.timestamp)
            .map_err(|_| UsdtError::InvalidResponse)
    }
    async fn token_balance(&self) -> Result<U256, UsdtError> {
        self.rpc
            .contract(
                TOKEN,
                Erc20::balanceOfCall {
                    owner: self.address,
                },
            )
            .await
    }
    async fn require_balance(&self, amount: u64, fee: u64) -> Result<(), UsdtError> {
        let required = U256::from(amount) + U256::from(fee);
        if self.token_balance().await? < required {
            return Err(UsdtError::InsufficientBalance);
        }
        Ok(())
    }
    async fn nonce(&self, block: &str) -> Result<U256, UsdtError> {
        let bytes: Bytes = self.rpc.call("eth_call", json!([{"to":ENTRY_POINT,"data":Bytes::from(EntryPoint::getNonceCall { sender:self.address, key:Default::default() }.abi_encode())}, block])).await?;
        EntryPoint::getNonceCall::abi_decode_returns(&bytes).map_err(|_| UsdtError::InvalidResponse)
    }
    async fn authorization(&self) -> Result<Authorization, UsdtError> {
        let code: Bytes = self
            .rpc
            .call("eth_getCode", json!([self.address, "latest"]))
            .await?;
        validate_delegation(&code)?;
        let nonce: U256 = self
            .rpc
            .call("eth_getTransactionCount", json!([self.address, "latest"]))
            .await?;
        Ok(Authorization::dummy(
            nonce.try_into().map_err(|_| UsdtError::InvalidResponse)?,
        ))
    }

    async fn broadcast(&self, plan: &Plan, expected_hash: B256) -> Result<(), UsdtError> {
        if self.authorization().await?.nonce != plan.operation.eip7702_auth.nonce {
            return Err(UsdtError::QuoteExpired);
        }
        let result: Result<B256, UsdtError> = self
            .paymaster
            .rpc
            .call(
                "eth_sendUserOperation",
                json!([plan.operation, ENTRY_POINT]),
            )
            .await;
        match result {
            Ok(hash) if hash == expected_hash => Ok(()),
            Ok(_) => {
                log::warn!("USDT submission returned an unexpected operation hash; retaining pending payment");
                Err(UsdtError::InvalidResponse)
            }
            Err(error) => {
                log::warn!("USDT submission could not be confirmed; retaining pending payment");
                Err(error)
            }
        }
    }
    async fn settle_from_log(
        &self,
        transfer: &mut UsdtTransfer,
        log: &Value,
        event: EntryPoint::UserOperationEvent,
    ) -> Result<(), UsdtError> {
        if event.sender != self.address || event.paymaster != PAYMASTER {
            return Err(UsdtError::InvalidResponse);
        }
        let hash: B256 = serde_json::from_value(log["transactionHash"].clone())?;
        transfer.tx_hash = format!("{hash:#x}");
        transfer.explorer_url = format!("{EXPLORER}/tx/{}", transfer.tx_hash);
        let number = u64::try_from(serde_json::from_value::<U256>(log["blockNumber"].clone())?)
            .map_err(|_| UsdtError::InvalidResponse)?;
        let block = self.rpc.block(number).await?;
        if serde_json::from_value::<B256>(log["blockHash"].clone())? != block.hash {
            return Err(UsdtError::NetworkUnavailable);
        }
        let receipt = self.rpc.block_receipt(hash, block.hash, number).await?;
        self.settle(transfer, &receipt)?;
        transfer.timestamp =
            u64::try_from(block.timestamp).map_err(|_| UsdtError::InvalidResponse)?;
        self.store.update_transfer(transfer)
    }

    async fn nonce_consumed_block(&self, plan: &Plan, tip: u64) -> Result<u64, UsdtError> {
        // EntryPoint nonces only increase. Locate the consuming block without a months-long log query.
        let mut low = plan.created_block;
        let mut high = tip;
        while low < high {
            let mid = low + (high - low) / 2;
            if self.nonce(&format!("0x{mid:x}")).await? > plan.operation.nonce {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        Ok(low)
    }

    async fn pending_search_end(&self, plan: &Plan, tip: u64) -> Result<u64, UsdtError> {
        // The paymaster validity window bounds recovery even after a long absence.
        if tip <= plan.created_block + 4096 {
            return Ok(tip);
        }
        let mut low = plan.created_block;
        let mut high = tip;
        while low + 1 < high {
            let mid = low + (high - low) / 2;
            if self.block_timestamp(mid).await? <= plan.expires_at {
                low = mid;
            } else {
                high = mid;
            }
        }
        Ok(high)
    }
    pub(super) fn settle(
        &self,
        transfer: &mut UsdtTransfer,
        receipt: &Value,
    ) -> Result<(), UsdtError> {
        let operation_hash: B256 = transfer
            .user_operation_hash
            .as_ref()
            .ok_or(UsdtError::InvalidResponse)?
            .parse()
            .map_err(|_| UsdtError::InvalidResponse)?;
        let logs = super::transaction::operation_logs(receipt, operation_hash)?;
        let event = EntryPoint::UserOperationEvent::decode_log_data(&event_data(
            logs.last().ok_or(UsdtError::InvalidResponse)?,
        )?)
        .map_err(|_| UsdtError::InvalidResponse)?;
        if event.userOpHash != operation_hash
            || event.sender != self.address
            || event.paymaster != PAYMASTER
        {
            return Err(UsdtError::InvalidResponse);
        }
        let mut gas_fee = None;
        for log in logs {
            let address: Address = serde_json::from_value(log["address"].clone())?;
            let data = event_data(log)?;
            if address == PAYMASTER {
                if let Ok(event) = Paymaster::UserOperationSponsored::decode_log_data(&data) {
                    if event.userOpHash == operation_hash
                        && event.user == self.address
                        && event.token == TOKEN
                        && event.paymasterMode == 1
                    {
                        gas_fee = Some(token_amount(event.tokenAmountPaid)?);
                    }
                }
            }
        }
        transfer.fee = gas_fee;
        transfer.status = if !event.success {
            transfer.received_amount = 0;
            UsdtTransferStatus::Failed
        } else {
            UsdtTransferStatus::Confirmed
        };
        Ok(())
    }
}

pub(super) fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}
