use super::{
    history::HISTORY_REVISIT_BLOCKS, transaction::Plan, UsdtError, UsdtQuote, UsdtTransfer,
    UsdtTransferStatus,
};
use alloy_primitives::B256;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct QuoteData {
    pub quote: UsdtQuote,
    pub plan: Plan,
}

pub(super) struct Store(Mutex<Connection>);

impl Store {
    pub fn open(path: &str, identity: &str) -> Result<Self, UsdtError> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| UsdtError::Storage {
                reason: e.to_string(),
            })?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS usdt_identity (id INTEGER PRIMARY KEY CHECK(id=1), identity TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_sync (id INTEGER PRIMARY KEY CHECK(id=1), newest INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_history_progress (id INTEGER PRIMARY KEY CHECK(id=1), next INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_history_receipts (hash TEXT PRIMARY KEY, block_number INTEGER NOT NULL, block_hash TEXT NOT NULL, complete_receipt INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_history_blocks (number INTEGER PRIMARY KEY, hash TEXT NOT NULL, complete INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_quotes (id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_nonce_recovery (id TEXT PRIMARY KEY, block_hash TEXT NOT NULL, next_transaction INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_transfers (id TEXT PRIMARY KEY, hash TEXT NOT NULL, raw TEXT, data TEXT NOT NULL, block_number INTEGER, block_hash TEXT);
            CREATE INDEX IF NOT EXISTS usdt_transfers_hash ON usdt_transfers(hash);
            CREATE INDEX IF NOT EXISTS usdt_transfers_block ON usdt_transfers(block_number);")?;
        connection.execute(
            "INSERT OR IGNORE INTO usdt_identity VALUES (1,?1)",
            [identity],
        )?;
        let stored: String =
            connection.query_row("SELECT identity FROM usdt_identity WHERE id=1", [], |r| {
                r.get(0)
            })?;
        if stored != identity {
            return Err(UsdtError::InvalidCredentials);
        }
        Ok(Self(Mutex::new(connection)))
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, UsdtError> {
        self.0.lock().map_err(|_| UsdtError::Storage {
            reason: "USDT storage lock unavailable".into(),
        })
    }

    pub fn save_quote(&self, data: &QuoteData) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        tx.execute(
            "DELETE FROM usdt_quotes WHERE json_extract(data, '$.quote.expires_at') <= ?1",
            [super::wallet::now()],
        )?;
        tx.execute(
            "INSERT INTO usdt_quotes VALUES (?1,?2)",
            params![data.quote.id, serde_json::to_string(data)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn quote(&self, id: &str) -> Result<QuoteData, UsdtError> {
        let data: Option<String> = self
            .connection()?
            .query_row("SELECT data FROM usdt_quotes WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        decode(&data.ok_or(UsdtError::QuoteExpired)?)
    }

    pub fn transfer(&self, id: &str) -> Result<Option<UsdtTransfer>, UsdtError> {
        let data: Option<String> = self
            .connection()?
            .query_row("SELECT data FROM usdt_transfers WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        data.map(|data| decode(&data)).transpose()
    }

    pub fn transfer_by_hash(&self, hash: &str) -> Result<Option<UsdtTransfer>, UsdtError> {
        find_transfer_by_hash(&*self.connection()?, hash)
    }

    pub fn transfers(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT data FROM usdt_transfers ORDER BY json_extract(data, '$.timestamp') DESC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| decode(&row?)).collect()
    }

    pub fn require_no_pending(&self) -> Result<(), UsdtError> {
        require_no_pending(&*self.connection()?)
    }

    pub fn record_signed(&self, transfer: &UsdtTransfer, raw: &str) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require_no_pending(&tx)?;
        tx.execute(
            "INSERT INTO usdt_transfers (id,hash,raw,data) VALUES (?1,?2,?3,?4)",
            params![
                transfer.id,
                transfer.user_operation_hash,
                raw,
                serde_json::to_string(transfer)?
            ],
        )?;
        tx.execute("DELETE FROM usdt_quotes WHERE id=?1", [&transfer.id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_transfer(&self, transfer: &UsdtTransfer) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        write_transfer(&tx, transfer)?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_delivery(
        &self,
        transfer: &UsdtTransfer,
        refund_block: Option<(u64, B256)>,
    ) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        if let Some(block) = refund_block {
            tx.execute(
                "UPDATE usdt_transfers SET raw=json_set(raw, '$.refund_block', json(?1)) WHERE id=?2",
                params![serde_json::to_string(&block)?, transfer.id],
            )?;
        }
        write_transfer(&tx, transfer)?;
        tx.commit()?;
        Ok(())
    }

    pub fn settle_transfer(
        &self,
        transfer: &UsdtTransfer,
        number: u64,
        hash: &str,
    ) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        reconcile_block(&tx, number, hash)?;
        write_settlement(&tx, transfer, number, hash)?;
        tx.commit()?;
        Ok(())
    }

    pub fn settlement_blocks(&self, start: u64, end: u64) -> Result<Vec<u64>, UsdtError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT block_number FROM usdt_transfers WHERE block_number BETWEEN ?1 AND ?2 UNION SELECT json_extract(raw, '$.refund_block[0]') FROM usdt_transfers WHERE json_extract(raw, '$.refund_block[0]') BETWEEN ?1 AND ?2 ORDER BY 1",
        )?;
        let rows = statement.query_map(params![start, end], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn reconcile_block(&self, number: u64, hash: &str) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        reconcile_block(&tx, number, hash)?;
        tx.commit()?;
        Ok(())
    }

    pub fn nonce_recovery(&self, id: &str, block_hash: &str) -> Result<usize, UsdtError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT next_transaction FROM usdt_nonce_recovery WHERE id=?1 AND block_hash=?2",
                params![id, block_hash],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn save_nonce_recovery(
        &self,
        id: &str,
        block_hash: &str,
        next_transaction: usize,
    ) -> Result<(), UsdtError> {
        self.connection()?.execute(
            "INSERT INTO usdt_nonce_recovery VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET block_hash=excluded.block_hash,next_transaction=excluded.next_transaction",
            params![id, block_hash, next_transaction],
        )?;
        Ok(())
    }

    pub fn synced_block(&self) -> Result<Option<u64>, UsdtError> {
        Ok(self
            .connection()?
            .query_row("SELECT newest FROM usdt_sync WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn complete_history(&self, newest: u64) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM usdt_history_progress", [])?;
        prune_settlement_proofs(&tx, newest.saturating_sub(HISTORY_REVISIT_BLOCKS))?;
        tx.execute(
            "DELETE FROM usdt_history_receipts WHERE complete_receipt=0 OR block_number < ?1",
            [newest.saturating_sub(HISTORY_REVISIT_BLOCKS)],
        )?;
        tx.execute(
            "DELETE FROM usdt_history_blocks WHERE number < ?1",
            [newest.saturating_sub(HISTORY_REVISIT_BLOCKS)],
        )?;
        tx.execute("INSERT INTO usdt_sync (id,newest) VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET newest=excluded.newest", [newest])?;
        tx.commit()?;
        Ok(())
    }

    pub fn history_progress(&self) -> Result<Option<u64>, UsdtError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT next FROM usdt_history_progress WHERE id=1",
                [],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn save_history_progress(&self, next: u64) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        prune_settlement_proofs(&tx, next.saturating_sub(HISTORY_REVISIT_BLOCKS))?;
        tx.execute(
            "DELETE FROM usdt_history_blocks WHERE number < ?1",
            [next.saturating_sub(HISTORY_REVISIT_BLOCKS)],
        )?;
        tx.execute("INSERT INTO usdt_history_progress VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET next=excluded.next", [next])?;
        tx.execute(
            "DELETE FROM usdt_history_receipts WHERE complete_receipt=0 OR block_number < ?1",
            [next.saturating_sub(HISTORY_REVISIT_BLOCKS)],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn begin_history_block(&self, number: u64, hash: &str) -> Result<bool, UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        reconcile_block(&tx, number, hash)?;
        let complete: Option<bool> = tx
            .query_row(
                "SELECT complete FROM usdt_history_blocks WHERE number=?1 AND hash=?2",
                params![number, hash],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(complete) = complete {
            return Ok(complete);
        }
        tx.execute("INSERT INTO usdt_history_blocks VALUES (?1,?2,0) ON CONFLICT(number) DO UPDATE SET hash=excluded.hash,complete=0", params![number, hash])?;
        tx.commit()?;
        Ok(false)
    }

    pub fn complete_history_block(&self, number: u64, hash: &str) -> Result<(), UsdtError> {
        self.connection()?.execute(
            "UPDATE usdt_history_blocks SET complete=1 WHERE number=?1 AND hash=?2",
            params![number, hash],
        )?;
        Ok(())
    }

    pub fn has_history_receipt(
        &self,
        hash: &str,
        block_hash: &str,
        require_complete: bool,
    ) -> Result<bool, UsdtError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM usdt_history_receipts WHERE hash=?1 AND block_hash=?2 AND (complete_receipt=1 OR ?3=0))",
            params![hash, block_hash, require_complete],
            |row| row.get(0),
        )?)
    }

    pub fn save_history_receipt(
        &self,
        transfers: &[UsdtTransfer],
        hash: &str,
        block_number: u64,
        block_hash: &str,
        complete_receipt: bool,
    ) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        reconcile_block(&tx, block_number, block_hash)?;
        Self::merge_history(&tx, transfers, block_number, block_hash)?;
        tx.execute(
            "INSERT INTO usdt_history_receipts VALUES (?1,?2,?3,?4) ON CONFLICT(hash) DO UPDATE SET block_number=excluded.block_number,block_hash=excluded.block_hash,complete_receipt=excluded.complete_receipt",
            params![hash, block_number, block_hash, complete_receipt],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn merge_history(
        tx: &Transaction<'_>,
        transfers: &[UsdtTransfer],
        number: u64,
        block_hash: &str,
    ) -> Result<(), UsdtError> {
        for transfer in transfers {
            let hash = transfer
                .user_operation_hash
                .as_deref()
                .unwrap_or(&transfer.id);
            let existing = find_transfer_by_hash(tx, hash)?;
            let mut transfer = transfer.clone();
            if let Some(saved) = existing {
                transfer.id = saved.id;
                if saved
                    .tx_hash
                    .as_deref()
                    .zip(transfer.tx_hash.as_deref())
                    .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
                    && ((saved
                        .bridge_guid
                        .as_ref()
                        .zip(transfer.bridge_guid.as_ref())
                        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b)))
                        || saved
                            .orchestra
                            .as_ref()
                            .zip(transfer.orchestra.as_ref())
                            .is_some_and(|(a, b)| {
                                a.quote_id == b.quote_id && a.funding_address == b.funding_address
                            }))
                    && transfer.status == UsdtTransferStatus::Bridging
                    && matches!(
                        saved.status,
                        UsdtTransferStatus::Confirmed
                            | UsdtTransferStatus::BridgeNeedsAttention
                            | UsdtTransferStatus::BridgeFailed
                            | UsdtTransferStatus::BridgeRefunded
                    )
                {
                    transfer.status = saved.status;
                    transfer.received_amount = saved.received_amount;
                    transfer.orchestra = saved.orchestra;
                }
                write_settlement(tx, &transfer, number, block_hash)?;
            } else {
                tx.execute(
                    "INSERT INTO usdt_transfers (id,hash,data,block_number,block_hash) VALUES (?1,?2,?3,?4,?5)",
                    params![transfer.id, hash, serde_json::to_string(&transfer)?, number, block_hash],
                )?;
            }
        }
        Ok(())
    }

    pub fn awaiting_delivery(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT data FROM usdt_transfers WHERE json_extract(data, '$.status') IN ('Bridging','BridgeNeedsAttention') AND (json_extract(data, '$.bridge_guid') IS NOT NULL OR json_extract(data, '$.orchestra') IS NOT NULL) ORDER BY json_extract(data, '$.timestamp') DESC, id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| decode(&row?)).collect()
    }

    pub fn pending_operations(&self) -> Result<Vec<(UsdtTransfer, Plan)>, UsdtError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT data,raw FROM usdt_transfers WHERE raw IS NOT NULL AND json_extract(data, '$.status')='Pending'",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut pending: Vec<(UsdtTransfer, Plan)> = rows
            .map(|row| {
                let (data, raw) = row?;
                Ok((decode(&data)?, decode(&raw)?))
            })
            .collect::<Result<_, UsdtError>>()?;
        pending.sort_by_key(|(_, plan)| plan.operation.nonce);
        Ok(pending)
    }

    pub fn orchestra_plan(&self, id: &str) -> Result<super::orchestra::OrchestraPlan, UsdtError> {
        let raw: Option<String> = self.connection()?.query_row(
            "SELECT json_extract(raw, '$.orchestra') FROM usdt_transfers WHERE id=?1",
            [id],
            |row| row.get(0),
        )?;
        decode(&raw.ok_or(UsdtError::InvalidResponse)?)
    }

    pub fn pending_plan(&self, id: &str) -> Result<Option<Plan>, UsdtError> {
        let raw: Option<String> = self
            .connection()?
            .query_row("SELECT raw FROM usdt_transfers WHERE id=?1 AND json_extract(data, '$.status')='Pending'", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        raw.map(|raw| decode(&raw)).transpose()
    }
}

fn write_transfer(connection: &Connection, transfer: &UsdtTransfer) -> Result<(), UsdtError> {
    let settled = transfer.status != UsdtTransferStatus::Pending;
    let keep_delivery = transfer.orchestra.is_some()
        && matches!(
            transfer.status,
            UsdtTransferStatus::Bridging | UsdtTransferStatus::BridgeNeedsAttention
        );
    // Chain-backed outcomes retain the signed operation until their reorg window passes.
    connection.execute(
        "UPDATE usdt_transfers SET data=?1, raw=CASE WHEN ?2 AND block_number IS NULL AND json_extract(raw, '$.refund_block') IS NULL THEN NULL ELSE raw END WHERE id=?3",
        params![serde_json::to_string(transfer)?, settled && !keep_delivery, transfer.id],
    )?;
    if settled {
        connection.execute(
            "DELETE FROM usdt_nonce_recovery WHERE id=?1",
            [&transfer.id],
        )?;
    }
    Ok(())
}

fn write_settlement(
    connection: &Connection,
    transfer: &UsdtTransfer,
    number: u64,
    hash: &str,
) -> Result<(), UsdtError> {
    connection.execute(
        "UPDATE usdt_transfers SET block_number=?1,block_hash=?2 WHERE id=?3",
        params![number, hash, transfer.id],
    )?;
    write_transfer(connection, transfer)
}

fn reconcile_block(tx: &Transaction<'_>, number: u64, hash: &str) -> Result<(), UsdtError> {
    // Only a different canonical block proves disappearance; missing logs alone do not.
    let mut statement =
        tx.prepare("SELECT data,raw FROM usdt_transfers WHERE block_number=?1 AND block_hash!=?2")?;
    let rows = statement.query_map(params![number, hash], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let orphaned = rows.collect::<Result<Vec<_>, _>>()?;
    for (data, raw) in orphaned {
        let mut transfer: UsdtTransfer = decode(&data)?;
        if let Some(raw) = raw {
            transfer.status = UsdtTransferStatus::Pending;
            transfer.tx_hash = None;
            transfer.bridge_guid = None;
            let mut plan: Plan = decode(&raw)?;
            plan.refund_block = None;
            tx.execute(
                "UPDATE usdt_transfers SET raw=?1 WHERE id=?2",
                params![serde_json::to_string(&plan)?, transfer.id],
            )?;
            transfer.orchestra = plan.orchestra.map(|plan| plan.transfer());
            transfer.received_amount = if transfer.destination == super::UsdtDestination::Arbitrum {
                transfer.amount
            } else {
                decode::<Plan>(&raw)?.bridge_received_amount()?
            };
            transfer.fee = None;
            write_transfer(tx, &transfer)?;
            tx.execute(
                "UPDATE usdt_transfers SET block_number=NULL,block_hash=NULL WHERE id=?1",
                [&transfer.id],
            )?;
        } else {
            tx.execute("DELETE FROM usdt_transfers WHERE id=?1", [&transfer.id])?;
        }
    }
    // A refund is a separate transaction: its reorg must not reopen the original funding send.
    tx.execute(
        "UPDATE usdt_transfers SET data=json_set(data, '$.status', 'Bridging', '$.orchestra.refund_tx', NULL, '$.orchestra.refund_amount', NULL, '$.received_amount', json_extract(raw, '$.orchestra.received_amount')),raw=json_remove(raw, '$.refund_block') WHERE json_extract(raw, '$.refund_block[0]')=?1 AND json_extract(raw, '$.refund_block[1]')!=?2",
        params![number, hash],
    )?;
    tx.execute(
        "DELETE FROM usdt_history_receipts WHERE block_number=?1 AND block_hash!=?2",
        params![number, hash],
    )?;
    tx.execute(
        "DELETE FROM usdt_history_blocks WHERE number=?1 AND hash!=?2",
        params![number, hash],
    )?;
    Ok(())
}

fn prune_settlement_proofs(connection: &Connection, before: u64) -> Result<(), UsdtError> {
    connection.execute(
        "UPDATE usdt_transfers SET block_number=NULL,block_hash=NULL WHERE block_number < ?1",
        [before],
    )?;
    connection.execute(
        "UPDATE usdt_transfers SET raw=NULL WHERE block_number IS NULL AND json_extract(data, '$.status')!='Pending' AND NOT (json_extract(data, '$.orchestra') IS NOT NULL AND json_extract(data, '$.status') IN ('Bridging','BridgeNeedsAttention')) AND COALESCE(json_extract(raw, '$.refund_block[0]'),0) < ?1",
        [before],
    )?;
    Ok(())
}

fn find_transfer_by_hash(
    connection: &Connection,
    hash: &str,
) -> Result<Option<UsdtTransfer>, UsdtError> {
    let data: Option<String> = connection
        .query_row(
            "SELECT data FROM usdt_transfers WHERE hash=?1",
            [hash],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| decode(&data)).transpose()
}

fn decode<T: DeserializeOwned>(data: &str) -> Result<T, UsdtError> {
    serde_json::from_str(data).map_err(|error| UsdtError::Storage {
        reason: error.to_string(),
    })
}

fn require_no_pending(connection: &Connection) -> Result<(), UsdtError> {
    let pending: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM usdt_transfers WHERE raw IS NOT NULL AND json_extract(data, '$.status')='Pending')",
        [],
        |row| row.get(0),
    )?;
    if pending {
        return Err(UsdtError::PendingTransfer);
    }
    Ok(())
}
