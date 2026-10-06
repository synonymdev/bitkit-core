use super::{
    account::{validate_delegation, ENTRY_POINT},
    amount::{token_amount, with_margin},
    keys::{derive_owner_key, parse_address},
    paymaster::{Pimlico, PAYMASTER},
    rpc::Rpc,
    store::{QuoteData, Store},
    transaction::{
        entry_point_event, event_data, BridgeHelper, EntryPoint, Erc20, Oft, Paymaster, Plan,
        SendParam,
    },
    types::{BRIDGE_HELPER, CHAIN_ID, OFT, TOKEN},
    user_operation::Authorization,
    UsdtDestination, UsdtError, UsdtQuote, UsdtTransfer, UsdtTransferStatus,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_sol_types::{SolCall, SolEvent};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use tokio::{sync::Mutex, time::Instant};

const RECENT_EXECUTION_BLOCKS: u64 = 64;
const RECENT_EXECUTION_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);
const NONCE_RECOVERY_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);
const EXPIRY_SEARCH_BLOCKS: u64 = 4096;
const QUOTE_LIFETIME_SECONDS: u64 = 120;
const BRIDGE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const BRIDGE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(uniffi::Object)]
pub struct UsdtWallet {
    pub(super) address: Address,
    pub(super) rpc: Rpc,
    pub(super) paymaster: Pimlico,
    pub(super) store: Store,
    orchestra: Option<super::orchestra::Orchestra>,
    pub(super) operation: Mutex<()>,
    bridge_poll_offset: AtomicUsize,
    bridge_retry_after: Mutex<HashMap<String, Instant>>,
    pub(super) history_range_limit: AtomicU64,
}

#[uniffi::export(async_runtime = "tokio")]
impl UsdtWallet {
    /// Creates the sole owner of this wallet's database; reuse it for all calls until it is dropped.
    #[uniffi::constructor]
    pub fn new(
        address: String,
        storage_path: String,
        rpc_url: String,
        bundler_url: String,
        bridge_url: Option<String>,
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
            orchestra: bridge_url
                .map(super::orchestra::Orchestra::new)
                .transpose()?,
            operation: Mutex::new(()),
            bridge_poll_offset: AtomicUsize::new(0),
            bridge_retry_after: Mutex::new(HashMap::new()),
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
        destination: UsdtDestination,
    ) -> Result<UsdtQuote, UsdtError> {
        if amount == 0 {
            return Err(UsdtError::InvalidAmount);
        }
        let recipient = super::orchestra::validate_recipient(recipient.trim(), destination)?;
        if recipient == self.receive_address()
            || (destination == UsdtDestination::Arbitrum
                && [OFT, BRIDGE_HELPER].contains(&parse_address(&recipient)?))
        {
            return Err(UsdtError::InvalidAddress);
        }
        self.store.require_no_pending()?;
        self.rpc.verify_chain().await?;
        self.require_balance(amount, 0).await?;
        let direct = self.prepare_quote(&recipient, amount, destination, false);
        let routed = async {
            if destination == UsdtDestination::Arbitrum
                || destination == UsdtDestination::Stable
                || self.orchestra.is_none()
            {
                return Err(UsdtError::UnsupportedRoute);
            }
            self.prepare_quote(&recipient, amount, destination, true)
                .await
        };
        let (direct, routed) = tokio::join!(direct, routed);
        let fresh = |candidate: Result<QuoteData, UsdtError>| {
            candidate.and_then(|data| {
                if data.quote.expires_at <= now() + 5 {
                    Err(UsdtError::QuoteExpired)
                } else {
                    Ok(data)
                }
            })
        };
        let selected = match (fresh(direct), fresh(routed)) {
            (Ok(a), Ok(b)) => {
                if better_quote(&b.quote, &a.quote)? {
                    b
                } else {
                    a
                }
            }
            (Ok(a), Err(_)) | (Err(_), Ok(a)) => a,
            (Err(a), Err(b)) => {
                return Err(
                    if destination.endpoint().is_some() || destination == UsdtDestination::Arbitrum
                    {
                        a
                    } else {
                        b
                    },
                )
            }
        };
        if selected.quote.expires_at <= now() + 5 {
            return Err(UsdtError::QuoteExpired);
        }
        self.store.save_quote(&selected)?;
        Ok(selected.quote)
    }

    /// Available Orchestra destinations. USDT0 destinations retain the app's existing configuration.
    pub async fn orchestra_destinations(&self) -> Result<Vec<UsdtDestination>, UsdtError> {
        match &self.orchestra {
            Some(client) => client.networks().await,
            None => Ok(Vec::new()),
        }
    }

    /// Repeating a quote ID returns its stored outcome, which may already be failed or replaced.
    /// A pending outcome is durable and retryable; it does not imply bundler acceptance.
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
            derive_owner_key(mnemonic, passphrase, self.address)?;
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
        self.validate_bridge(&data.plan).await?;
        self.paymaster.validate_gas(&data.plan.operation).await?;
        let block = self.block_number().await?;
        if self.block_timestamp(block).await?.saturating_add(5) >= data.plan.expires_at {
            return Err(UsdtError::QuoteExpired);
        }
        let key = derive_owner_key(mnemonic, passphrase, self.address)?;
        if data.quote.expires_at <= now() + 5 {
            return Err(UsdtError::QuoteExpired);
        }
        // A head retreat can place execution before the block used to prepare the quote.
        data.plan.created_block = data
            .plan
            .created_block
            .min(block.saturating_sub(super::history::HISTORY_REVISIT_BLOCKS));
        let (hash, raw) = data.plan.sign(&key)?;
        drop(key);
        let mut transfer = UsdtTransfer {
            id: quote_id,
            tx_hash: None,
            user_operation_hash: Some(format!("{hash:#x}")),
            bridge_guid: None,
            orchestra: data.plan.orchestra.as_ref().map(|plan| plan.transfer()),
            recipient: data.quote.recipient,
            destination: data.quote.destination,
            amount: data.quote.amount,
            received_amount: data.quote.received_amount,
            fee: None,
            is_incoming: false,
            status: UsdtTransferStatus::Pending,
            timestamp: now(),
        };
        self.store.record_signed(&transfer, &raw)?;
        // After persistence a lost response is indeterminate. Retry only the identical signed operation.
        if let Err(error) = self.broadcast(&data.plan, hash).await {
            // These errors occur before submission; later retries may already be queued.
            if matches!(
                error,
                UsdtError::QuoteExpired | UsdtError::UnsupportedDelegation
            ) {
                transfer.mark_unexecuted(UsdtTransferStatus::Failed);
                self.store.update_transfer(&transfer)?;
                return Err(error);
            }
        }
        Ok(transfer)
    }

    /// Checks recent direct-payment execution with a bounded request budget.
    /// Requires the expected operation and transfer in a canonical receipt; current-tip execution is provisional.
    /// Does not rebroadcast, expire payments or reconcile nonces. Missing evidence leaves the payment pending.
    /// Returns stored activity immediately when another wallet operation is in progress.
    pub async fn check_recent_execution(
        &self,
        id: String,
    ) -> Result<Option<UsdtTransfer>, UsdtError> {
        let Ok(_guard) = self.operation.try_lock() else {
            return self.store.transfer(&id);
        };
        let check = async {
            let Some(mut transfer) = self.store.transfer(&id)? else {
                return Ok(None);
            };
            if transfer.destination != UsdtDestination::Arbitrum {
                return Ok(Some(transfer));
            }
            let Some(plan) = self.store.pending_plan(&id)? else {
                return Ok(Some(transfer));
            };
            self.rpc.verify_chain().await?;
            let hash = plan.operation.hash(CHAIN_ID)?;
            let tip = self.block_number().await?;
            let start = plan
                .created_block
                .max(tip.saturating_sub(RECENT_EXECUTION_BLOCKS - 1));
            if start <= tip {
                let logs: Vec<Value> = self.operation_logs_in(start, tip, Some(hash)).await?;
                if let Some(log) = logs
                    .iter()
                    .find(|log| log["removed"].as_bool() != Some(true))
                {
                    let event = entry_point_event(log)?.ok_or(UsdtError::InvalidResponse)?;
                    let number =
                        u64::try_from(serde_json::from_value::<U256>(log["blockNumber"].clone())?)
                            .map_err(|_| UsdtError::InvalidResponse)?;
                    if event.userOpHash != hash
                        || event.nonce != plan.operation.nonce
                        || number < start
                        || number > tip
                    {
                        return Err(UsdtError::InvalidResponse);
                    }
                    self.settle_from_log(&mut transfer, log, event).await?;
                }
            }
            Ok(Some(transfer))
        };
        match tokio::time::timeout(RECENT_EXECUTION_BUDGET, check).await {
            Ok(result) => result,
            Err(_) => Err(UsdtError::NetworkUnavailable),
        }
    }

    /// Saves resumable history progress; returns true when caught up and false when more work remains.
    /// Call between send flows. The soft budget permits an in-flight receipt to finish before yielding.
    pub async fn sync_history(&self) -> Result<bool, UsdtError> {
        let _guard = self.operation.lock().await;
        self.rpc.verify_chain().await?;
        self.scan_history().await
    }

    pub fn history(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        self.store.transfers()
    }

    /// Reconciles pending execution using chain proofs and may rebroadcast the identical signed operation.
    pub async fn refresh_transfers(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        let pending = self.refresh_pending_transfers().await;
        self.refresh_bridges(&self.store.awaiting_delivery()?)
            .await?;
        pending?;
        self.history()
    }
}

impl UsdtWallet {
    async fn prepare_quote(
        &self,
        recipient: &str,
        amount: u64,
        destination: UsdtDestination,
        use_orchestra: bool,
    ) -> Result<QuoteData, UsdtError> {
        let orchestra = if use_orchestra {
            Some(
                self.orchestra
                    .as_ref()
                    .ok_or(UsdtError::UnsupportedRoute)?
                    .quote(self.address, recipient.into(), amount, destination)
                    .await?,
            )
        } else {
            None
        };
        let (calls, received_amount, bridge_fee) = if let Some(plan) = &orchestra {
            (
                vec![(
                    TOKEN,
                    Erc20::transferCall {
                        recipient: parse_address(&plan.funding_address)?,
                        amount: U256::from(amount),
                    }
                    .abi_encode()
                    .into(),
                )],
                plan.received_amount,
                0,
            )
        } else {
            if destination != UsdtDestination::Arbitrum && destination.endpoint().is_none() {
                return Err(UsdtError::UnsupportedRoute);
            }
            self.transfer_calls(parse_address(recipient)?, amount, destination)
                .await?
        };
        let nonce = self.nonce("latest").await?;
        let authorization = self.authorization().await?;
        let created_block = self.block_number().await?;
        let timestamp = self.block_timestamp(created_block).await?;
        let (operation, gas_fee, operation_expires_at) = self
            .paymaster
            .prepare(self.address, nonce, authorization, &calls, timestamp)
            .await?;
        let mut expires_at = now().saturating_add(
            operation_expires_at
                .saturating_sub(timestamp)
                .min(QUOTE_LIFETIME_SECONDS),
        );
        if let Some(plan) = &orchestra {
            expires_at = expires_at.min(plan.expires_at);
        }
        let maximum_fee = gas_fee
            .checked_add(bridge_fee)
            .ok_or(UsdtError::InvalidAmount)?;
        amount
            .checked_add(maximum_fee)
            .ok_or(UsdtError::InvalidAmount)?;
        self.require_balance(amount, maximum_fee).await?;
        let bridge_provider = if orchestra.is_some() {
            Some(super::UsdtBridgeProvider::Orchestra)
        } else if destination == UsdtDestination::Arbitrum {
            None
        } else {
            Some(super::UsdtBridgeProvider::Usdt0)
        };
        Ok(QuoteData {
            quote: UsdtQuote {
                id: uuid::Uuid::new_v4().to_string(),
                recipient: recipient.into(),
                destination,
                amount,
                received_amount,
                maximum_fee,
                expires_at,
                bridge_provider,
            },
            plan: Plan {
                refund_block: None,
                operation,
                orchestra,
                created_block,
                expires_at: operation_expires_at,
            },
        })
    }

    async fn refresh_pending_transfers(&self) -> Result<(), UsdtError> {
        let _guard = self.operation.lock().await;
        for (mut transfer, plan) in self.store.pending_operations()? {
            self.recover_pending(&mut transfer, &plan).await?;
            if transfer.status == UsdtTransferStatus::Pending {
                break;
            }
        }
        Ok(())
    }

    async fn recover_pending(
        &self,
        transfer: &mut UsdtTransfer,
        plan: &Plan,
    ) -> Result<(), UsdtError> {
        self.rpc.verify_chain().await?;
        let hash = plan.operation.hash(CHAIN_ID)?;
        let confirmed_tip = self.block_number().await?.saturating_sub(2);
        if confirmed_tip < plan.created_block {
            return Ok(());
        }
        let end = self.pending_search_end(plan, confirmed_tip).await?;
        let logs = match self
            .operation_logs_in(plan.created_block, end, Some(hash))
            .await
        {
            Ok(logs) => logs,
            // Discovery can be unavailable while independent nonce/receipt proofs still work.
            Err(UsdtError::LogRangeTooLarge | UsdtError::NetworkUnavailable) => Vec::new(),
            Err(error) => return Err(error),
        };
        if let Some(log) = logs
            .iter()
            .find(|log| log["removed"].as_bool() != Some(true))
        {
            let event = entry_point_event(log)?.ok_or(UsdtError::InvalidResponse)?;
            if event.userOpHash != hash || event.nonce != plan.operation.nonce {
                return Err(UsdtError::InvalidResponse);
            }
            return self.settle_from_log(transfer, log, event).await;
        }
        let block = self.rpc.block(confirmed_tip).await?;
        let nonce = self.nonce(&format!("0x{confirmed_tip:x}")).await?;
        if nonce <= plan.operation.nonce {
            if block.timestamp()? > plan.expires_at {
                if self.rpc.block(confirmed_tip).await?.hash != block.hash {
                    return Err(UsdtError::NetworkUnavailable);
                }
                transfer.mark_unexecuted(UsdtTransferStatus::Failed);
                self.store.settle_transfer(
                    transfer,
                    confirmed_tip,
                    &format!("{:#x}", block.hash),
                )?;
            } else if self.validate_bridge(plan).await.is_ok() {
                let _ = self.broadcast(plan, hash).await;
            }
            return Ok(());
        }
        // A nonce advance alone cannot distinguish this payment from a replacement.
        let block = self.nonce_consumed_block(plan, confirmed_tip).await?;
        let candidates = match self.operation_logs_in(block, block, None).await {
            Ok(logs) => logs,
            Err(UsdtError::LogRangeTooLarge | UsdtError::NetworkUnavailable) => Vec::new(),
            Err(error) => return Err(error),
        };
        for log in candidates
            .iter()
            .filter(|log| log["removed"].as_bool() != Some(true))
        {
            let event = entry_point_event(log)?.ok_or(UsdtError::InvalidResponse)?;
            if u64::try_from(serde_json::from_value::<U256>(log["blockNumber"].clone())?)
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
                return self.settle_from_log(transfer, log, event).await;
            }
            break;
        }
        self.reconcile_consumed_nonce(transfer, plan, block).await
    }

    async fn operation_logs_in(
        &self,
        start: u64,
        end: u64,
        hash: Option<B256>,
    ) -> Result<Vec<Value>, UsdtError> {
        self.rpc.call("eth_getLogs", json!([{
            "address": ENTRY_POINT, "fromBlock": U256::from(start), "toBlock": U256::from(end),
            "topics": [EntryPoint::UserOperationEvent::SIGNATURE_HASH, hash, self.address.into_word()]
        }])).await
    }

    async fn reconcile_consumed_nonce(
        &self,
        transfer: &mut UsdtTransfer,
        plan: &Plan,
        number: u64,
    ) -> Result<(), UsdtError> {
        let deadline = tokio::time::Instant::now() + NONCE_RECOVERY_BUDGET;
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
                let Some(event) = entry_point_event(log)? else {
                    continue;
                };
                if event.sender != self.address || event.nonce != plan.operation.nonce {
                    continue;
                }
                if event.userOpHash == plan.operation.hash(CHAIN_ID)? {
                    if event.paymaster != PAYMASTER {
                        return Err(UsdtError::InvalidResponse);
                    }
                    transfer.tx_hash = Some(format!("{hash:#x}"));
                    self.settle(transfer, &receipt)?;
                } else {
                    transfer.mark_unexecuted(UsdtTransferStatus::Replaced);
                }
                transfer.timestamp = block.timestamp()?;
                return self.store.settle_transfer(transfer, number, &block_hash);
            }
            self.store
                .save_nonce_recovery(&transfer.id, &block_hash, index + 1)?;
        }
        // Empty indexed logs are insufficient; every receipt in the consuming block must be checked.
        if self.rpc.block(number).await?.hash != block.hash {
            return Err(UsdtError::NetworkUnavailable);
        }
        transfer.mark_unexecuted(UsdtTransferStatus::Replaced);
        transfer.timestamp = block.timestamp()?;
        self.store.settle_transfer(transfer, number, &block_hash)
    }

    async fn refresh_bridges(&self, transfers: &[UsdtTransfer]) -> Result<(), UsdtError> {
        let mut retry_after = self.bridge_retry_after.lock().await;
        retry_after.retain(|_, deadline| *deadline > Instant::now());
        let mut bridges: Vec<_> = transfers
            .iter()
            .filter(|transfer| !retry_after.contains_key(&transfer.id))
            .collect();
        drop(retry_after);
        if bridges.is_empty() {
            return Ok(());
        }
        let batch = [0, 1, 2];
        let offset = self
            .bridge_poll_offset
            .fetch_add(batch.len(), Ordering::Relaxed)
            % bridges.len();
        bridges.rotate_left(offset);
        let bridges = &bridges;
        let check = |index: usize| async move {
            let transfer = bridges.get(index).copied()?;
            Some((
                transfer,
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    self.bridge_delivery(transfer),
                )
                .await,
            ))
        };
        let (first, second, third) =
            tokio::join!(check(batch[0]), check(batch[1]), check(batch[2]));
        for (previous, result) in [first, second, third].into_iter().flatten() {
            let delay = match &result {
                Ok(Ok((updated, _))) if updated.status == UsdtTransferStatus::Bridging => {
                    Some(BRIDGE_POLL_INTERVAL)
                }
                Ok(Ok((updated, _)))
                    if matches!(
                        updated.status,
                        UsdtTransferStatus::Confirmed
                            | UsdtTransferStatus::BridgeFailed
                            | UsdtTransferStatus::BridgeRefunded
                    ) =>
                {
                    None
                }
                _ => Some(BRIDGE_RETRY_DELAY),
            };
            if let Some(delay) = delay {
                self.bridge_retry_after
                    .lock()
                    .await
                    .insert(previous.id.clone(), Instant::now() + delay);
            }
            match result {
                Ok(Ok((updated, refund_block))) => {
                    let _guard = self.operation.lock().await;
                    let Some(mut current) = self.store.transfer(&previous.id)? else {
                        continue;
                    };
                    if current
                        .tx_hash
                        .as_deref()
                        .zip(previous.tx_hash.as_deref())
                        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
                        && current.bridge_guid == previous.bridge_guid
                        && current.orchestra == previous.orchestra
                        && current.status == previous.status
                    {
                        current.status = updated.status;
                        current.orchestra = updated.orchestra;
                        current.received_amount = updated.received_amount;
                        self.store.update_delivery(&current, refund_block)?;
                    }
                }
                _ => log::warn!(
                    "USDT bridge delivery lookup unavailable; retaining last known status"
                ),
            }
        }
        Ok(())
    }

    async fn bridge_delivery(
        &self,
        transfer: &UsdtTransfer,
    ) -> Result<(UsdtTransfer, Option<(u64, B256)>), UsdtError> {
        let mut updated = transfer.clone();
        let Some(bridge) = updated.orchestra.as_mut() else {
            updated.status = self.rpc.bridge_status(transfer).await?;
            return Ok((updated, None));
        };
        let plan = self.store.orchestra_plan(&transfer.id)?;
        if plan.quote_id != bridge.quote_id
            || plan.funding_address != bridge.funding_address
            || plan.recipient != transfer.recipient
            || plan.destination != transfer.destination
            || plan.amount != transfer.amount
        {
            return Err(UsdtError::InvalidResponse);
        }
        let delivery = self
            .orchestra
            .as_ref()
            .ok_or(UsdtError::NotConfigured)?
            .status(
                &plan,
                transfer
                    .tx_hash
                    .as_deref()
                    .ok_or(UsdtError::InvalidResponse)?,
            )
            .await?;
        let mut refund_block = None;
        updated.status = match delivery.status.as_str() {
            "bridging" => UsdtTransferStatus::Bridging,
            "needs_attention" => UsdtTransferStatus::BridgeNeedsAttention,
            "completed" => {
                let amount = delivery
                    .received_amount
                    .filter(|amount| *amount > 0)
                    .ok_or(UsdtError::InvalidResponse)?;
                let hash = delivery
                    .destination_tx
                    .filter(|hash| !hash.is_empty() && hash.len() <= 128)
                    .ok_or(UsdtError::InvalidResponse)?;
                bridge.destination_tx = Some(hash);
                updated.received_amount = amount;
                UsdtTransferStatus::Confirmed
            }
            "refunded" => {
                let amount = delivery
                    .refund_amount
                    .filter(|amount| *amount > 0 && *amount <= transfer.amount)
                    .ok_or(UsdtError::InvalidResponse)?;
                let hash = delivery.refund_tx.ok_or(UsdtError::InvalidResponse)?;
                refund_block = Some(self.verify_refund(&hash, amount).await?);
                bridge.refund_tx = Some(hash);
                bridge.refund_amount = Some(amount);
                updated.received_amount = 0;
                UsdtTransferStatus::BridgeRefunded
            }
            _ => return Err(UsdtError::InvalidResponse),
        };
        Ok((updated, refund_block))
    }

    async fn verify_refund(&self, hash: &str, amount: u64) -> Result<(u64, B256), UsdtError> {
        let hash: B256 = hash.parse().map_err(|_| UsdtError::InvalidResponse)?;
        let receipt: Value = self
            .rpc
            .call("eth_getTransactionReceipt", json!([hash]))
            .await?;
        let number = token_amount(serde_json::from_value(receipt["blockNumber"].clone())?)?;
        if number > self.block_number().await?.saturating_sub(2) {
            return Err(UsdtError::NetworkUnavailable);
        }
        let block = self.rpc.block(number).await?;
        let receipt = self.rpc.block_receipt(hash, block.hash, number).await?;
        if serde_json::from_value::<U256>(receipt["status"].clone())? != U256::from(1) {
            return Err(UsdtError::InvalidResponse);
        }
        for log in receipt["logs"]
            .as_array()
            .ok_or(UsdtError::InvalidResponse)?
        {
            if serde_json::from_value::<Address>(log["address"].clone())? == TOKEN {
                if let Ok(event) = Erc20::Transfer::decode_log_data(&event_data(log)?) {
                    if event.to == self.address && event.value == U256::from(amount) {
                        return Ok((number, block.hash));
                    }
                }
            }
        }
        Err(UsdtError::InvalidResponse)
    }

    pub(super) async fn block_number(&self) -> Result<u64, UsdtError> {
        u64::try_from(self.rpc.call::<U256>("eth_blockNumber", json!([])).await?)
            .map_err(|_| UsdtError::InvalidResponse)
    }
    pub(super) async fn block_timestamp(&self, number: u64) -> Result<u64, UsdtError> {
        self.rpc.block(number).await?.timestamp()
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
        self.rpc
            .contract_at(
                ENTRY_POINT,
                EntryPoint::getNonceCall {
                    sender: self.address,
                    key: Default::default(),
                },
                block,
            )
            .await
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
        if plan
            .orchestra
            .as_ref()
            .is_some_and(|quote| quote.expires_at <= now() + 5)
        {
            return Err(UsdtError::QuoteExpired);
        }
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
        transfer.tx_hash = Some(format!("{hash:#x}"));
        let number = u64::try_from(serde_json::from_value::<U256>(log["blockNumber"].clone())?)
            .map_err(|_| UsdtError::InvalidResponse)?;
        let block = self.rpc.block(number).await?;
        if serde_json::from_value::<B256>(log["blockHash"].clone())? != block.hash {
            return Err(UsdtError::NetworkUnavailable);
        }
        let receipt = self.rpc.block_receipt(hash, block.hash, number).await?;
        self.settle(transfer, &receipt)?;
        transfer.timestamp = block.timestamp()?;
        self.store
            .settle_transfer(transfer, number, &format!("{:#x}", block.hash))
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
        if tip <= plan.created_block + EXPIRY_SEARCH_BLOCKS {
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
        let event = entry_point_event(logs.last().ok_or(UsdtError::InvalidResponse)?)?
            .ok_or(UsdtError::InvalidResponse)?;
        if event.sender != self.address || event.paymaster != PAYMASTER {
            return Err(UsdtError::InvalidResponse);
        }
        let funding_recipient = if !event.success {
            Address::ZERO
        } else if let Some(orchestra) = &transfer.orchestra {
            parse_address(&orchestra.funding_address)?
        } else if transfer.destination == UsdtDestination::Arbitrum {
            parse_address(&transfer.recipient)?
        } else {
            Address::ZERO
        };
        let mut transfer_proven = false;
        let mut gas_fee = None;
        let mut bridge_fee = None;
        for log in logs {
            let address: Address = serde_json::from_value(log["address"].clone())?;
            let data = event_data(log)?;
            if event.success && address == TOKEN {
                if let Ok(payment) = Erc20::Transfer::decode_log_data(&data) {
                    transfer_proven |= payment.from == self.address
                        && payment.to == funding_recipient
                        && payment.value == U256::from(transfer.amount);
                }
            }
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
            if event.success && transfer.orchestra.is_none() && address == BRIDGE_HELPER {
                if let Ok(event) = BridgeHelper::LogSend::decode_log_data(&data) {
                    if event.sender == self.address
                        && event.oft == OFT
                        && event.amountLD == U256::from(transfer.amount)
                    {
                        bridge_fee = Some(token_amount(event.feeInToken)?);
                    }
                }
            }
            if event.success && transfer.orchestra.is_none() && address == OFT {
                if let Ok(event) = Oft::OFTSent::decode_log_data(&data) {
                    if event.fromAddress == BRIDGE_HELPER
                        && transfer.destination.endpoint() == Some(event.dstEid)
                        && event.amountSentLD == U256::from(transfer.amount)
                    {
                        transfer.bridge_guid = Some(format!("{:#x}", event.guid));
                        transfer.received_amount = token_amount(event.amountReceivedLD)?;
                    }
                }
            }
        }
        if event.success
            && (transfer.destination == UsdtDestination::Arbitrum || transfer.orchestra.is_some())
            && !transfer_proven
        {
            return Err(UsdtError::InvalidResponse);
        }
        transfer.fee = if event.success
            && transfer.destination != UsdtDestination::Arbitrum
            && transfer.orchestra.is_none()
        {
            gas_fee
                .zip(bridge_fee)
                .and_then(|(gas, bridge)| gas.checked_add(bridge))
        } else {
            gas_fee
        };
        transfer.status = if !event.success {
            transfer.received_amount = 0;
            UsdtTransferStatus::Failed
        } else if transfer.destination == UsdtDestination::Arbitrum {
            UsdtTransferStatus::Confirmed
        } else if transfer.bridge_guid.is_some() || transfer.orchestra.is_some() {
            UsdtTransferStatus::Bridging
        } else {
            UsdtTransferStatus::BridgeNeedsAttention
        };
        Ok(())
    }
    async fn validate_bridge(&self, plan: &Plan) -> Result<(), UsdtError> {
        if plan
            .orchestra
            .as_ref()
            .is_some_and(|quote| quote.expires_at <= now() + 5)
        {
            return Err(UsdtError::QuoteExpired);
        }
        let calls = super::history::decode_calls(&plan.operation.call_data)?;
        let Some((_, data)) = calls.iter().find(|(target, _)| *target == BRIDGE_HELPER) else {
            return Ok(());
        };
        let send =
            BridgeHelper::sendCall::abi_decode(data).map_err(|_| UsdtError::InvalidResponse)?;
        let requote = |error| match error {
            UsdtError::UnsupportedRoute => UsdtError::QuoteExpired,
            error => error,
        };
        let required = self
            .rpc
            .contract(
                OFT,
                Oft::quoteSendCall {
                    param: send.param.clone(),
                    payInLzToken: false,
                },
            )
            .await
            .map_err(requote)?;
        if !required.lzTokenFee.is_zero() || required.nativeFee > send.fee.nativeFee {
            return Err(UsdtError::QuoteExpired);
        }
        if self.rpc.balance(BRIDGE_HELPER).await? < send.fee.nativeFee {
            return Err(UsdtError::UnsupportedRoute);
        }
        let total = self
            .rpc
            .contract(
                BRIDGE_HELPER,
                BridgeHelper::quoteSendCall {
                    param: send.param,
                    fee: send.fee,
                },
            )
            .await
            .map_err(requote)?;
        let allowance = calls
            .iter()
            .filter(|(target, _)| *target == TOKEN)
            .filter_map(|(_, data)| Erc20::approveCall::abi_decode(data).ok())
            .find(|call| call.spender == BRIDGE_HELPER)
            .ok_or(UsdtError::InvalidResponse)?;
        if total > allowance.amount {
            return Err(UsdtError::QuoteExpired);
        }
        Ok(())
    }

    async fn transfer_calls(
        &self,
        recipient: Address,
        amount: u64,
        destination: UsdtDestination,
    ) -> Result<(Vec<(Address, Bytes)>, u64, u64), UsdtError> {
        let Some(eid) = destination.endpoint() else {
            return Ok((
                vec![(
                    TOKEN,
                    Erc20::transferCall {
                        recipient,
                        amount: U256::from(amount),
                    }
                    .abi_encode()
                    .into(),
                )],
                amount,
                0,
            ));
        };
        let token = self.rpc.contract(OFT, Oft::tokenCall {}).await?;
        let helper_token = self
            .rpc
            .contract(BRIDGE_HELPER, BridgeHelper::tokenCall {})
            .await?;
        let peer = self.rpc.contract(OFT, Oft::peersCall { eid }).await?;
        if token != TOKEN || helper_token != TOKEN || peer.is_zero() {
            return Err(UsdtError::UnsupportedRoute);
        }
        if recipient.into_word() == peer {
            return Err(UsdtError::InvalidAddress);
        }
        let mut param = SendParam {
            dstEid: eid,
            to: recipient.into_word(),
            amountLD: U256::from(amount),
            minAmountLD: U256::ZERO,
            extraOptions: Bytes::new(),
            composeMsg: Bytes::new(),
            oftCmd: Bytes::new(),
        };
        let oft = self
            .rpc
            .contract(
                OFT,
                Oft::quoteOFTCall {
                    param: param.clone(),
                },
            )
            .await?;
        if U256::from(amount) < oft.limit.minAmountLD
            || U256::from(amount) > oft.limit.maxAmountLD
            || oft.receipt.amountSentLD != U256::from(amount)
            || oft.receipt.amountReceivedLD.is_zero()
            || oft.receipt.amountReceivedLD > U256::from(amount)
        {
            return Err(UsdtError::InvalidAmount);
        }
        param.minAmountLD = oft.receipt.amountReceivedLD;
        let mut fee = self
            .rpc
            .contract(
                OFT,
                Oft::quoteSendCall {
                    param: param.clone(),
                    payInLzToken: false,
                },
            )
            .await?;
        // Native headroom is quoted into the approved USDT maximum.
        fee.nativeFee = with_margin(fee.nativeFee, 10)?;
        let maximum_native = self
            .rpc
            .contract(BRIDGE_HELPER, BridgeHelper::maxGasCall {})
            .await?;
        if !fee.lzTokenFee.is_zero()
            || fee.nativeFee > maximum_native
            || self.rpc.balance(BRIDGE_HELPER).await? < fee.nativeFee
        {
            return Err(UsdtError::UnsupportedRoute);
        }
        let total = self
            .rpc
            .contract(
                BRIDGE_HELPER,
                BridgeHelper::quoteSendCall {
                    param: param.clone(),
                    fee: fee.clone(),
                },
            )
            .await?;
        let token_fee = total
            .checked_sub(U256::from(amount))
            .ok_or(UsdtError::InvalidResponse)?;
        let token_fee = with_margin(token_fee, 20)?;
        let approval = U256::from(amount)
            .checked_add(token_fee)
            .ok_or(UsdtError::InvalidResponse)?;
        Ok((
            vec![
                (
                    TOKEN,
                    Erc20::approveCall {
                        spender: BRIDGE_HELPER,
                        amount: approval,
                    }
                    .abi_encode()
                    .into(),
                ),
                (
                    BRIDGE_HELPER,
                    BridgeHelper::sendCall {
                        oft: OFT,
                        param,
                        fee,
                    }
                    .abi_encode()
                    .into(),
                ),
                (
                    TOKEN,
                    Erc20::approveCall {
                        spender: BRIDGE_HELPER,
                        amount: U256::ZERO,
                    }
                    .abi_encode()
                    .into(),
                ),
            ],
            token_amount(oft.receipt.amountReceivedLD)?,
            token_amount(token_fee)?,
        ))
    }
}

pub(super) fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

// Compare the expected receipt per maximum USDT debit without floating-point rates.
// A tie keeps the USDT0 route; only usable, funded-balance candidates reach this comparison.
fn better_quote(candidate: &UsdtQuote, current: &UsdtQuote) -> Result<bool, UsdtError> {
    let candidate_total = candidate
        .amount
        .checked_add(candidate.maximum_fee)
        .ok_or(UsdtError::InvalidAmount)?;
    let current_total = current
        .amount
        .checked_add(current.maximum_fee)
        .ok_or(UsdtError::InvalidAmount)?;
    Ok(
        u128::from(candidate.received_amount) * u128::from(current_total)
            > u128::from(current.received_amount) * u128::from(candidate_total),
    )
}
