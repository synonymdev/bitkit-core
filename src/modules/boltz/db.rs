use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::models::{
    BoltzDB, SwapRecord, CREATE_META_TABLE, CREATE_SWAPS_TABLE, SCHEMA_VERSION,
};
use crate::modules::boltz::types::{BoltzNetwork, BoltzSwapType};
use rusqlite::{params, Connection, OptionalExtension, Row};

/// Counter key in `swap_meta` for the next deterministic swap index.
const NEXT_SWAP_INDEX_KEY: &str = "next_swap_index";

impl BoltzDB {
    /// Open (or create) the swaps database at `db_path` and run migrations.
    pub async fn new(db_path: &str) -> Result<BoltzDB, BoltzError> {
        let conn = Connection::open(db_path).map_err(|e| BoltzError::InitializationError {
            error_details: format!("Error opening database: {}", e),
        })?;
        let db = BoltzDB {
            conn: tokio::sync::Mutex::new(conn),
        };
        db.initialize().await?;
        Ok(db)
    }

    async fn initialize(&self) -> Result<(), BoltzError> {
        let conn = self.conn.lock().await;
        conn.execute(CREATE_SWAPS_TABLE, [])
            .map_err(|e| BoltzError::InitializationError {
                error_details: format!("Failed to create swaps table: {}", e),
            })?;
        conn.execute(CREATE_META_TABLE, [])
            .map_err(|e| BoltzError::InitializationError {
                error_details: format!("Failed to create swap_meta table: {}", e),
            })?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| BoltzError::InitializationError {
                error_details: format!("Failed to set schema version: {}", e),
            })?;
        Ok(())
    }

    /// Atomically reserve the next deterministic swap index. The connection
    /// mutex serializes all access, so the read-then-write below cannot
    /// interleave with another reservation. Indices are monotonic and never
    /// reused, so each swap derives a unique key even if a creation later fails.
    pub async fn reserve_swap_index(&self) -> Result<u64, BoltzError> {
        let conn = self.conn.lock().await;
        let current: Option<i64> = conn
            .query_row(
                "SELECT value FROM swap_meta WHERE key = ?1",
                params![NEXT_SWAP_INDEX_KEY],
                |row| row.get(0),
            )
            .optional()?;
        let index = current.unwrap_or(0);
        conn.execute(
            "INSERT INTO swap_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![NEXT_SWAP_INDEX_KEY, index + 1],
        )?;
        Ok(index as u64)
    }

    /// Insert a newly-created swap.
    pub async fn insert_swap(&self, record: &SwapRecord) -> Result<(), BoltzError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO swaps (
                id, swap_type, status, network, electrum_url, swap_index,
                invoice, lockup_address, onchain_address, amount_sat, onchain_amount_sat,
                timeout_block_height, create_response_json, claim_tx_id, refund_tx_id, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                record.id,
                record.swap_type.as_str(),
                record.status,
                record.network.as_str(),
                record.electrum_url,
                record.swap_index as i64,
                record.invoice,
                record.lockup_address,
                record.onchain_address,
                record.amount_sat as i64,
                record.onchain_amount_sat.map(|v| v as i64),
                record.timeout_block_height as i64,
                record.create_response_json,
                record.claim_tx_id,
                record.refund_tx_id,
                record.created_at as i64,
            ],
        )
        .map_err(|e| BoltzError::DatabaseError {
            error_details: format!("Failed to insert swap: {}", e),
        })?;
        Ok(())
    }

    /// Update the raw status string of a swap.
    ///
    /// Once a claim or refund txid has been recorded locally, the locally set
    /// terminal status is ground truth for what happened onchain; a delayed or
    /// re-ordered server update (the WebSocket and the reconcile loop both feed
    /// this) must not regress it, so completed swaps are left untouched.
    pub async fn update_status(&self, swap_id: &str, status: &str) -> Result<(), BoltzError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE swaps SET status = ?1
             WHERE id = ?2 AND claim_tx_id IS NULL AND refund_tx_id IS NULL",
            params![status, swap_id],
        )?;
        Ok(())
    }

    /// Record the broadcast claim transaction id for a swap and mark it claimed.
    ///
    /// The `status` column is normally advanced only by the live updates stream, so
    /// claiming (manual or automatic) would otherwise leave a swap showing its
    /// pre-claim status. Setting the terminal `transaction.claimed` status here keeps
    /// the persisted state truthful and drops the swap from the pending set.
    pub async fn set_claim_tx(&self, swap_id: &str, txid: &str) -> Result<(), BoltzError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE swaps SET claim_tx_id = ?1, status = 'transaction.claimed' WHERE id = ?2",
            params![txid, swap_id],
        )?;
        Ok(())
    }

    /// Record the broadcast refund transaction id for a swap and mark it refunded.
    pub async fn set_refund_tx(&self, swap_id: &str, txid: &str) -> Result<(), BoltzError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE swaps SET refund_tx_id = ?1, status = 'transaction.refunded' WHERE id = ?2",
            params![txid, swap_id],
        )?;
        Ok(())
    }

    /// Fetch a single swap by id.
    pub async fn get_swap(&self, swap_id: &str) -> Result<Option<SwapRecord>, BoltzError> {
        let conn = self.conn.lock().await;
        let record = conn
            .query_row(
                "SELECT id, swap_type, status, network, electrum_url, swap_index,
                        invoice, lockup_address, onchain_address, amount_sat, onchain_amount_sat,
                        timeout_block_height, create_response_json, claim_tx_id, refund_tx_id,
                        created_at
                 FROM swaps WHERE id = ?1",
                params![swap_id],
                row_to_record,
            )
            .optional()?;
        record.transpose()
    }

    /// List every persisted swap, newest first.
    pub async fn list_swaps(&self) -> Result<Vec<SwapRecord>, BoltzError> {
        self.query_swaps(
            "SELECT id, swap_type, status, network, electrum_url, swap_index,
                invoice, lockup_address, onchain_address, amount_sat, onchain_amount_sat,
                timeout_block_height, create_response_json, claim_tx_id, refund_tx_id, created_at
             FROM swaps ORDER BY created_at DESC",
        )
        .await
    }

    /// List swaps that are not locally complete, for recovery and for
    /// resubscribing to status updates after a restart. Completion is judged by
    /// [`SwapRecord::is_locally_complete`], not server status alone, so a
    /// reverse swap whose invoice settled but whose claim never broadcast stays
    /// recoverable.
    pub async fn list_pending_swaps(&self) -> Result<Vec<SwapRecord>, BoltzError> {
        Ok(self
            .list_swaps()
            .await?
            .into_iter()
            .filter(|r| !r.is_locally_complete())
            .collect())
    }

    async fn query_swaps(&self, sql: &str) -> Result<Vec<SwapRecord>, BoltzError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], row_to_record)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row??);
        }
        Ok(records)
    }
}

/// Map a SQLite row to a [`SwapRecord`]. The outer `rusqlite::Result` covers
/// column-access failures; the inner `Result<SwapRecord, BoltzError>` covers
/// decoding of persisted enum strings (`swap_type`, `network`). No secrets are
/// stored, so there is no key material to decode here.
fn row_to_record(row: &Row) -> rusqlite::Result<Result<SwapRecord, BoltzError>> {
    let swap_type_str: String = row.get(1)?;
    let network_str: String = row.get(3)?;
    let amount_sat: i64 = row.get(9)?;
    let onchain_amount_sat: Option<i64> = row.get(10)?;
    let timeout_block_height: i64 = row.get(11)?;
    let created_at: i64 = row.get(15)?;
    let swap_index: i64 = row.get(5)?;

    let swap_type = match BoltzSwapType::from_str(&swap_type_str) {
        Some(t) => t,
        None => {
            return Ok(Err(BoltzError::DatabaseError {
                error_details: format!("Unknown swap_type: {}", swap_type_str),
            }))
        }
    };
    let network = match BoltzNetwork::from_str(&network_str) {
        Some(n) => n,
        None => {
            return Ok(Err(BoltzError::DatabaseError {
                error_details: format!("Unknown network: {}", network_str),
            }))
        }
    };

    Ok(Ok(SwapRecord {
        id: row.get(0)?,
        swap_type,
        status: row.get(2)?,
        network,
        electrum_url: row.get(4)?,
        swap_index: swap_index as u64,
        invoice: row.get(6)?,
        lockup_address: row.get(7)?,
        onchain_address: row.get(8)?,
        amount_sat: amount_sat as u64,
        onchain_amount_sat: onchain_amount_sat.map(|v| v as u64),
        timeout_block_height: timeout_block_height as u64,
        create_response_json: row.get(12)?,
        claim_tx_id: row.get(13)?,
        refund_tx_id: row.get(14)?,
        created_at: created_at as u64,
    }))
}
