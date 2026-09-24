use super::{transaction::Plan, UsdtError, UsdtQuote, UsdtTransfer, UsdtTransferStatus};
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
            CREATE TABLE IF NOT EXISTS usdt_history_receipts (hash TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS usdt_quotes (id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_nonce_recovery (id TEXT PRIMARY KEY, block_hash TEXT NOT NULL, next_transaction INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS usdt_transfers (id TEXT PRIMARY KEY, hash TEXT NOT NULL, raw TEXT, data TEXT NOT NULL);")?;
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
        let data: Option<String> = self
            .connection()?
            .query_row(
                "SELECT data FROM usdt_transfers WHERE hash=?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|data| decode(&data)).transpose()
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
        let settled = matches!(
            transfer.status,
            UsdtTransferStatus::Confirmed
                | UsdtTransferStatus::Failed
                | UsdtTransferStatus::Replaced
        );
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        tx.execute("UPDATE usdt_transfers SET data=?1, raw=CASE WHEN ?2 THEN NULL ELSE raw END WHERE id=?3",
            params![serde_json::to_string(transfer)?, settled, transfer.id])?;
        if settled {
            tx.execute(
                "DELETE FROM usdt_nonce_recovery WHERE id=?1",
                [&transfer.id],
            )?;
        }
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
        tx.execute("DELETE FROM usdt_history_receipts", [])?;
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
        tx.execute("INSERT INTO usdt_history_progress VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET next=excluded.next", [next])?;
        tx.execute("DELETE FROM usdt_history_receipts", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn transaction_timestamp(&self, hash: &str) -> Result<Option<u64>, UsdtError> {
        Ok(self.connection()?.query_row(
            "SELECT json_extract(data, '$.timestamp') FROM usdt_transfers WHERE json_extract(data, '$.tx_hash')=?1 LIMIT 1",
            [hash], |row| row.get(0),
        ).optional()?)
    }

    pub fn has_history_receipt(&self, hash: &str) -> Result<bool, UsdtError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM usdt_history_receipts WHERE hash=?1)",
            [hash],
            |row| row.get(0),
        )?)
    }

    pub fn save_history_receipt(
        &self,
        transfers: &[UsdtTransfer],
        hash: &str,
    ) -> Result<(), UsdtError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        Self::merge_history(&tx, transfers)?;
        tx.execute(
            "INSERT OR IGNORE INTO usdt_history_receipts VALUES (?1)",
            [hash],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn merge_history(tx: &Transaction<'_>, transfers: &[UsdtTransfer]) -> Result<(), UsdtError> {
        for transfer in transfers {
            let hash = transfer
                .user_operation_hash
                .as_deref()
                .unwrap_or(&transfer.id);
            let existing: Option<String> = tx
                .query_row(
                    "SELECT data FROM usdt_transfers WHERE hash=?1",
                    [hash],
                    |row| row.get(0),
                )
                .optional()?;
            let mut transfer = transfer.clone();
            if let Some(data) = existing {
                let saved: UsdtTransfer = decode(&data)?;
                transfer.id = saved.id;
                tx.execute(
                    "UPDATE usdt_transfers SET data=?1, raw=NULL WHERE id=?2",
                    params![serde_json::to_string(&transfer)?, transfer.id],
                )?;
                tx.execute(
                    "DELETE FROM usdt_nonce_recovery WHERE id=?1",
                    [&transfer.id],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO usdt_transfers (id,hash,data) VALUES (?1,?2,?3)",
                    params![transfer.id, hash, serde_json::to_string(&transfer)?],
                )?;
            }
        }
        Ok(())
    }

    pub fn unsettled(&self) -> Result<Vec<UsdtTransfer>, UsdtError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT data FROM usdt_transfers")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut result = Vec::new();
        for row in rows {
            let transfer: UsdtTransfer = decode(&row?)?;
            if transfer.status == UsdtTransferStatus::Pending {
                result.push(transfer);
            }
        }
        Ok(result)
    }

    pub fn pending_plan(&self, id: &str) -> Result<Option<Plan>, UsdtError> {
        let raw: Option<String> = self
            .connection()?
            .query_row("SELECT raw FROM usdt_transfers WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        raw.map(|raw| decode(&raw)).transpose()
    }
}

fn decode<T: DeserializeOwned>(data: &str) -> Result<T, UsdtError> {
    serde_json::from_str(data).map_err(|error| UsdtError::Storage {
        reason: error.to_string(),
    })
}

fn require_no_pending(connection: &Connection) -> Result<(), UsdtError> {
    let pending: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM usdt_transfers WHERE raw IS NOT NULL)",
        [],
        |row| row.get(0),
    )?;
    if pending {
        return Err(UsdtError::PendingTransfer);
    }
    Ok(())
}
