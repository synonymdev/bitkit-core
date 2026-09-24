use super::{
    account::{SimpleAccount, ENTRY_POINT},
    amount::token_amount,
    transaction::{event_data, BridgeHelper, EntryPoint, Erc20},
    types::{BRIDGE_HELPER, EXPLORER, OFT, TOKEN},
    UsdtDestination, UsdtError, UsdtTransfer, UsdtTransferStatus, UsdtWallet,
};
use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, SolEvent};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::atomic::Ordering};

pub(super) const MAX_LOG_RANGE: u64 = 10_000_000;

impl UsdtWallet {
    pub(super) async fn scan_history(&self) -> Result<bool, UsdtError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        let tip = self.block_number().await?.saturating_sub(2);
        let previous = self.store.synced_block()?;
        let start = self.store.history_progress()?.unwrap_or_else(|| {
            previous
                .map(|block| block.saturating_sub(4096))
                .unwrap_or(0)
        });
        if start > tip {
            return Err(UsdtError::NetworkUnavailable);
        }
        let mut next = start;
        let mut width = self
            .history_range_limit
            .load(Ordering::Relaxed)
            .min(tip - start + 1);
        while next <= tip {
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            let end = next.saturating_add(width - 1).min(tip);
            let query = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                self.history_logs(next, end),
            )
            .await;
            let result = match query {
                Ok(result) => result,
                Err(_) => {
                    self.history_range_limit
                        .store((width / 2).max(1), Ordering::Relaxed);
                    return Ok(false);
                }
            };
            match result {
                Ok(transactions) => {
                    if self.store.history_progress()? != Some(next) {
                        self.store.save_history_progress(next)?;
                    }
                    let mut timestamps = BTreeMap::new();
                    for ((block, hash), logs) in transactions {
                        if self.store.has_history_receipt(&hash)? {
                            continue;
                        }
                        if tokio::time::Instant::now() >= deadline {
                            return Ok(false);
                        }
                        let needs_receipt = logs.iter().any(|log| {
                            serde_json::from_value::<Address>(log["address"].clone())
                                .is_ok_and(|address| address == ENTRY_POINT)
                                || event_data(log)
                                    .ok()
                                    .and_then(|data| Erc20::Transfer::decode_log_data(&data).ok())
                                    .is_some_and(|event| event.from == self.address)
                        });
                        let receipt = if needs_receipt {
                            self.rpc
                                .call("eth_getTransactionReceipt", json!([hash]))
                                .await?
                        } else {
                            json!({"logs": logs})
                        };
                        let timestamp =
                            if let Some(timestamp) = self.store.transaction_timestamp(&hash)? {
                                timestamp
                            } else if let Some(timestamp) = timestamps.get(&block) {
                                *timestamp
                            } else {
                                let timestamp = self.block_timestamp(block).await?;
                                timestamps.insert(block, timestamp);
                                timestamp
                            };
                        self.save_receipt_history(&hash, timestamp, &receipt)
                            .await?;
                    }
                }
                Err(UsdtError::LogRangeTooLarge) if next < end => {
                    width = (width / 2).max(1);
                    self.history_range_limit.store(width, Ordering::Relaxed);
                    continue;
                }
                Err(UsdtError::LogRangeTooLarge) => {
                    if !self.scan_block(next, deadline).await? {
                        return Ok(false);
                    }
                }
                Err(UsdtError::NetworkUnavailable) => {
                    self.history_range_limit
                        .store((width / 2).max(1), Ordering::Relaxed);
                    return Err(UsdtError::NetworkUnavailable);
                }
                Err(error) => return Err(error),
            }
            if end == tip {
                self.store.complete_history(tip)?;
                return Ok(true);
            }
            next = end + 1;
            self.store.save_history_progress(next)?;
            width = (width * 2).min(MAX_LOG_RANGE);
            self.history_range_limit.store(width, Ordering::Relaxed);
            width = width.min(tip - next + 1);
        }
        Ok(true)
    }

    async fn scan_block(
        &self,
        number: u64,
        deadline: tokio::time::Instant,
    ) -> Result<bool, UsdtError> {
        let block = self.rpc.block(number).await?;
        let timestamp = u64::try_from(block.timestamp).map_err(|_| UsdtError::InvalidResponse)?;
        if self.store.history_progress()? != Some(number) {
            self.store.save_history_progress(number)?;
        }
        for hash in &block.transactions {
            let id = format!("{hash:#x}");
            if self.store.has_history_receipt(&id)? {
                continue;
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            let receipt = self.rpc.block_receipt(*hash, &block, number).await?;
            self.save_receipt_history(&id, timestamp, &receipt).await?;
        }
        Ok(true)
    }

    async fn save_receipt_history(
        &self,
        hash: &str,
        timestamp: u64,
        receipt: &Value,
    ) -> Result<(), UsdtError> {
        // Finish and persist a receipt before yielding the work budget.
        let transfers = self.receipt_history(hash, timestamp, receipt).await?;
        self.store.save_history_receipt(&transfers, hash)
    }

    async fn history_logs(
        &self,
        start: u64,
        end: u64,
    ) -> Result<BTreeMap<(u64, String), Vec<Value>>, UsdtError> {
        let address = self.address.into_word();
        let filter = |topics: Value| json!({"address":TOKEN,"fromBlock":U256::from(start),"toBlock":U256::from(end),"topics":topics});
        let (incoming_and_operations, outgoing): (Vec<Value>, Vec<Value>) = tokio::try_join!(
            self.rpc.call("eth_getLogs", json!([{
                "address":[TOKEN, ENTRY_POINT],"fromBlock":U256::from(start),"toBlock":U256::from(end),
                "topics":[[Erc20::Transfer::SIGNATURE_HASH, EntryPoint::UserOperationEvent::SIGNATURE_HASH],null,address]
            }])),
            self.rpc.call("eth_getLogs", json!([filter(json!([Erc20::Transfer::SIGNATURE_HASH, address]))]))
        )?;
        let mut transactions: BTreeMap<(u64, String), Vec<Value>> = BTreeMap::new();
        for log in incoming_and_operations.into_iter().chain(outgoing) {
            if log["removed"].as_bool() == Some(true) {
                continue;
            }
            if serde_json::from_value::<Address>(log["address"].clone())? == TOKEN {
                let event = Erc20::Transfer::decode_log_data(&event_data(&log)?)
                    .map_err(|_| UsdtError::InvalidResponse)?;
                if event.value.is_zero() || event.from == event.to {
                    continue;
                }
            }
            let hash: String = serde_json::from_value(log["transactionHash"].clone())?;
            let block = u64::try_from(serde_json::from_value::<U256>(log["blockNumber"].clone())?)
                .map_err(|_| UsdtError::InvalidResponse)?;
            if !(start..=end).contains(&block) {
                return Err(UsdtError::InvalidResponse);
            }
            transactions.entry((block, hash)).or_default().push(log);
        }
        Ok(transactions)
    }

    async fn receipt_history(
        &self,
        hash: &str,
        timestamp: u64,
        receipt: &Value,
    ) -> Result<Vec<UsdtTransfer>, UsdtError> {
        let logs = receipt["logs"]
            .as_array()
            .ok_or(UsdtError::InvalidResponse)?;
        let mut result = Vec::new();
        let mut owned_operations = Vec::new();
        for log in logs {
            let address: Address = serde_json::from_value(log["address"].clone())?;
            if address == ENTRY_POINT {
                if let Ok(event) =
                    EntryPoint::UserOperationEvent::decode_log_data(&event_data(log)?)
                {
                    if event.sender == self.address {
                        let saved = self
                            .store
                            .transfer_by_hash(&format!("{:#x}", event.userOpHash))?;
                        owned_operations.push((event, saved));
                    }
                }
            }
            if address != TOKEN {
                continue;
            }
            let Ok(event) = Erc20::Transfer::decode_log_data(&event_data(log)?) else {
                continue;
            };
            let incoming = event.to == self.address && event.from != self.address;
            let outgoing = event.from == self.address && event.to != self.address;
            if (!incoming && !outgoing) || event.value.is_zero() {
                continue;
            }
            let index: U256 = serde_json::from_value(log["logIndex"].clone())?;
            result.push(UsdtTransfer {
                id: format!("{hash}:{index}"),
                tx_hash: hash.into(),
                user_operation_hash: None,
                bridge_guid: None,
                recipient: event.to.to_checksum(None),
                destination: UsdtDestination::Arbitrum,
                amount: token_amount(event.value)?,
                received_amount: token_amount(event.value)?,
                fee: None,
                is_incoming: incoming,
                status: UsdtTransferStatus::Confirmed,
                timestamp,
                explorer_url: format!("{EXPLORER}/tx/{hash}"),
            });
        }
        if owned_operations.is_empty() {
            return Ok(result);
        }
        let batch = if owned_operations.iter().any(|(_, saved)| saved.is_none()) {
            let tx: Value = self
                .rpc
                .call("eth_getTransactionByHash", json!([hash]))
                .await?;
            let input: Bytes = serde_json::from_value(tx["input"].clone())?;
            let target: Option<Address> = serde_json::from_value(tx["to"].clone())?;
            // A wrapper's outer calldata need not describe the operation it executes.
            if target == Some(ENTRY_POINT) {
                EntryPoint::handleOpsCall::abi_decode(&input).ok()
            } else {
                None
            }
        } else {
            None
        };
        for (event, saved) in owned_operations {
            let operation_hash = format!("{:#x}", event.userOpHash);
            let (recipient, amount, destination) = if let Some(saved) = saved {
                (saved.recipient, saved.amount, saved.destination)
            } else {
                let Some(op) = batch.as_ref().and_then(|batch| {
                    batch
                        .ops
                        .iter()
                        .find(|op| op.sender == self.address && op.nonce == event.nonce)
                }) else {
                    continue;
                };
                let Some((recipient, amount, destination)) = decode_payment(&op.callData)? else {
                    continue;
                };
                (recipient.to_checksum(None), amount, destination)
            };
            let mut transfer = UsdtTransfer {
                id: operation_hash.clone(),
                tx_hash: hash.into(),
                user_operation_hash: Some(operation_hash),
                bridge_guid: None,
                recipient,
                destination,
                amount,
                received_amount: amount,
                fee: None,
                is_incoming: false,
                status: UsdtTransferStatus::Pending,
                timestamp,
                explorer_url: format!("{EXPLORER}/tx/{hash}"),
            };
            self.settle(&mut transfer, receipt, event.success)?;
            let operation_logs = super::transaction::operation_logs(receipt, event.userOpHash)?;
            let outgoing_ids: Vec<_> = operation_logs
                .iter()
                .filter_map(|log| {
                    let index: U256 = serde_json::from_value(log["logIndex"].clone()).ok()?;
                    Some(format!("{hash}:{index}"))
                })
                .collect();
            result.retain(|transfer| transfer.is_incoming || !outgoing_ids.contains(&transfer.id));
            result.push(transfer);
        }
        Ok(result)
    }
}

fn decode_payment(data: &[u8]) -> Result<Option<(Address, u64, UsdtDestination)>, UsdtError> {
    let Ok(calls) = decode_calls(data) else {
        return Ok(None);
    };
    let mut payment = None;
    let mut payment_count = 0;
    let mut supported = true;
    for (target, data) in calls {
        if target == TOKEN {
            if let Ok(call) = Erc20::transferCall::abi_decode(&data) {
                payment_count += 1;
                payment = Some((
                    call.recipient,
                    token_amount(call.amount)?,
                    UsdtDestination::Arbitrum,
                ));
            } else if Erc20::approveCall::abi_decode(&data).is_err() {
                supported = false;
            }
        } else if target == BRIDGE_HELPER {
            if let Ok(call) = BridgeHelper::sendCall::abi_decode(&data) {
                if call.oft != OFT {
                    supported = false;
                    continue;
                }
                let Some(destination) = [
                    UsdtDestination::Ethereum,
                    UsdtDestination::Polygon,
                    UsdtDestination::Plasma,
                    UsdtDestination::Stable,
                ]
                .into_iter()
                .find(|d| d.endpoint() == Some(call.param.dstEid)) else {
                    supported = false;
                    continue;
                };
                payment_count += 1;
                payment = Some((
                    Address::from_word(call.param.to),
                    token_amount(call.param.amountLD)?,
                    destination,
                ));
            } else {
                supported = false;
            }
        } else {
            supported = false;
        }
    }
    if !supported || payment_count != 1 {
        return Ok(None);
    }
    Ok(payment)
}

pub(super) fn decode_calls(data: &[u8]) -> Result<Vec<(Address, Bytes)>, UsdtError> {
    if let Ok(execute) = SimpleAccount::executeCall::abi_decode(data) {
        if !execute.value.is_zero() {
            return Err(UsdtError::InvalidResponse);
        }
        return Ok(vec![(execute.target, execute.data)]);
    }
    let batch = SimpleAccount::executeBatchCall::abi_decode(data)
        .map_err(|_| UsdtError::InvalidResponse)?;
    batch
        .calls
        .into_iter()
        .map(|call| {
            if !call.value.is_zero() {
                return Err(UsdtError::InvalidResponse);
            }
            Ok((call.target, call.data))
        })
        .collect()
}
