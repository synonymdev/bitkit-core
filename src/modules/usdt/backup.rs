use super::{
    history::decode_payment, keys::parse_address, store::Store, transaction::Plan, types::CHAIN_ID,
    UsdtError, UsdtTransfer, UsdtTransferStatus, UsdtWallet,
};
use alloy_primitives::{Address, B256};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Saves recovery data before a signed payment can be submitted, including automatic retries.
/// Implementations must encrypt the snapshot, persist it remotely with its application payment
/// associations, and return only after acknowledgement. Do not log the snapshot or call mutating
/// wallet methods from this callback. A failed backup leaves the payment pending locally.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait UsdtBackup: Send + Sync {
    async fn persist(&self, snapshot: String) -> Result<(), UsdtError>;
}

#[derive(Serialize, Deserialize)]
struct Backup {
    identity: String,
    transfers: Vec<TransferBackup>,
}

#[derive(Serialize, Deserialize)]
struct TransferBackup {
    transfer: UsdtTransfer,
    plan: Option<Plan>,
    block: Option<(u64, B256)>,
}

#[uniffi::export(async_runtime = "tokio")]
impl UsdtWallet {
    /// Portable recovery data without keys, fee quotes or rebuildable history caches.
    /// Contains signed operations: store only inside an authenticated, encrypted backup.
    pub fn export_backup(&self) -> Result<String, UsdtError> {
        self.store.export_backup()
    }

    /// Atomically merges recovery data for this account. Existing local outcomes take precedence.
    /// Restored signed operations remain pending until chain reconciliation proves their outcome;
    /// recovery may resubmit only the original signed payload through the backup callback.
    pub async fn restore_backup(&self, snapshot: String) -> Result<(), UsdtError> {
        let _guard = self.operation.lock().await;
        self.store.restore_backup(&snapshot, self.address)
    }
}

impl Store {
    pub fn export_backup(&self) -> Result<String, UsdtError> {
        let connection = self.connection()?;
        let identity =
            connection.query_row("SELECT identity FROM usdt_identity WHERE id=1", [], |row| {
                row.get(0)
            })?;
        let mut statement = connection
            .prepare("SELECT data,raw,block_number,block_hash FROM usdt_transfers ORDER BY id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<u64>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let transfers = rows
            .map(|row| {
                let (data, raw, number, hash) = row?;
                let block = match (number, hash) {
                    (Some(number), Some(hash)) => {
                        Some((number, hash.parse().map_err(|_| UsdtError::InvalidBackup)?))
                    }
                    (None, None) => None,
                    _ => return Err(UsdtError::InvalidBackup),
                };
                Ok(TransferBackup {
                    transfer: serde_json::from_str(&data)?,
                    plan: raw.map(|raw| serde_json::from_str(&raw)).transpose()?,
                    block,
                })
            })
            .collect::<Result<_, UsdtError>>()?;
        Ok(serde_json::to_string(&Backup {
            identity,
            transfers,
        })?)
    }

    fn restore_backup(&self, snapshot: &str, owner: Address) -> Result<(), UsdtError> {
        let backup: Backup =
            serde_json::from_str(snapshot).map_err(|_| UsdtError::InvalidBackup)?;
        if backup.identity != format!("{CHAIN_ID}:{owner}") {
            return Err(UsdtError::InvalidCredentials);
        }
        let mut ids = HashSet::new();
        let mut hashes = HashSet::new();
        for item in &backup.transfers {
            let transfer = &item.transfer;
            let hash = transfer
                .user_operation_hash
                .as_deref()
                .unwrap_or(&transfer.id);
            if transfer.id.is_empty()
                || !ids.insert(&transfer.id)
                || !hashes.insert(hash)
                || super::orchestra::validate_recipient(&transfer.recipient, transfer.destination)
                    .is_err()
                || (transfer.status == UsdtTransferStatus::Pending
                    || transfer.orchestra.is_some()
                        && matches!(
                            transfer.status,
                            UsdtTransferStatus::Bridging | UsdtTransferStatus::BridgeNeedsAttention
                        ))
                    && item.plan.is_none()
            {
                return Err(UsdtError::InvalidBackup);
            }
            if let Some(plan) = &item.plan {
                let expected = transfer
                    .user_operation_hash
                    .as_deref()
                    .and_then(|hash| hash.parse::<B256>().ok());
                let payment = decode_payment(&plan.operation.call_data, owner);
                if transfer.is_incoming
                    || plan.operation.sender != owner
                    || plan.operation.hash(CHAIN_ID).ok() != expected
                    || expected.is_none()
                    || payment.map(|(recipient, amount, destination, _)| {
                        if let Some(route) = &plan.orchestra {
                            recipient == parse_address(&route.funding_address).unwrap_or_default()
                                && amount == route.amount
                                && amount == transfer.amount
                                && destination == super::UsdtDestination::Arbitrum
                                && route.recipient == transfer.recipient
                                && route.destination == transfer.destination
                                && transfer.orchestra.as_ref().is_some_and(|bridge| {
                                    bridge.quote_id == route.quote_id
                                        && bridge.funding_address == route.funding_address
                                })
                                && !route.ticket.is_empty()
                                && route.ticket.len() <= 4096
                        } else {
                            transfer.orchestra.is_none()
                                && recipient
                                    == parse_address(&transfer.recipient).unwrap_or_default()
                                && amount == transfer.amount
                                && destination == transfer.destination
                        }
                    }) != Some(true)
                {
                    return Err(UsdtError::InvalidBackup);
                }
            }
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for mut item in backup.transfers {
            if let Some(plan) = item.plan.as_mut() {
                plan.refund_block = None;
            }
            let hash = item
                .transfer
                .user_operation_hash
                .clone()
                .unwrap_or_else(|| item.transfer.id.clone());
            let existing: Option<String> = tx
                .query_row(
                    "SELECT hash FROM usdt_transfers WHERE id=?1",
                    [&item.transfer.id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing != hash {
                    return Err(UsdtError::InvalidBackup);
                }
                continue;
            }
            // Seed recovery can discover a payment before its application ID is restored.
            let discovered: Option<(String, String)> = tx
                .query_row(
                    "SELECT id,data FROM usdt_transfers WHERE hash=?1",
                    [&hash],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((id, data)) = discovered {
                if id != hash {
                    return Err(UsdtError::InvalidBackup);
                }
                let mut transfer: UsdtTransfer = serde_json::from_str(&data)?;
                transfer.id = item.transfer.id;
                if item.transfer.orchestra.is_some() {
                    let bridge = item
                        .transfer
                        .orchestra
                        .as_ref()
                        .ok_or(UsdtError::InvalidBackup)?;
                    if transfer.destination != super::UsdtDestination::Arbitrum
                        || transfer.recipient != bridge.funding_address
                        || transfer.amount != item.transfer.amount
                    {
                        return Err(UsdtError::InvalidBackup);
                    }
                    transfer.destination = item.transfer.destination;
                    transfer.recipient = item.transfer.recipient;
                    transfer.orchestra = item.transfer.orchestra;
                    if transfer.status == UsdtTransferStatus::Confirmed {
                        if let Some(route) =
                            item.plan.as_ref().and_then(|plan| plan.orchestra.as_ref())
                        {
                            transfer.status = UsdtTransferStatus::Bridging;
                            transfer.orchestra = Some(route.transfer());
                            transfer.received_amount = route.received_amount;
                        } else {
                            transfer.status = item.transfer.status;
                            transfer.received_amount = item.transfer.received_amount;
                        }
                    }
                }
                tx.execute(
                    "UPDATE usdt_transfers SET id=?1,data=?2,raw=COALESCE(raw,?4) WHERE id=?3",
                    params![
                        transfer.id,
                        serde_json::to_string(&transfer)?,
                        id,
                        item.plan
                            .map(|plan| serde_json::to_string(&plan))
                            .transpose()?
                    ],
                )?;
                continue;
            }
            if let Some(plan) = &item.plan {
                item.block = None;
                item.transfer.status = UsdtTransferStatus::Pending;
                item.transfer.tx_hash = None;
                item.transfer.bridge_guid = None;
                item.transfer.orchestra = plan.orchestra.as_ref().map(|route| route.transfer());
                item.transfer.fee = None;
                item.transfer.received_amount =
                    if item.transfer.destination == super::UsdtDestination::Arbitrum {
                        item.transfer.amount
                    } else {
                        plan.bridge_received_amount()?
                    };
            }
            tx.execute(
                "INSERT INTO usdt_transfers (id,hash,data,raw,block_number,block_hash) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    item.transfer.id,
                    hash,
                    serde_json::to_string(&item.transfer)?,
                    item.plan
                        .map(|plan| serde_json::to_string(&plan))
                        .transpose()?,
                    item.block.map(|(number, _)| number),
                    item.block.map(|(_, hash)| hash.to_string()),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
