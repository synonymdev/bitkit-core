//! The wallet's lasting state, on bitkit-core's own rusqlite (ark-client's `sqlite` feature would link a second
//! sqlite through sqlx):
//! - every contract (address) the wallet handed out, with its HD key index, which the key provider re-derives at
//!   connect;
//! - every Lightning swap, with what finishing it needs (a receive's preimage salt, the lockup's parameters). The
//!   seed alone cannot bring an in-flight swap back.
//!
//! VTXOs, boarding outputs and history are read from the operator and the chain. The dump `export` writes is the
//! portable form the app keeps in its VSS backup.

use super::errors::ArkadeError;
use super::intents::{Error as SwapStoreError, Result as SwapResult, SwapRecord, SwapStore};
use ark_client::{ContractStore, Error};
use ark_core::contract::{ContractState, StoredContract};
use bitcoin::Script;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub const DB_FILE: &str = "arkade.db";
const DUMP_VERSION: u32 = 1;

fn open_db(data_dir: &Path) -> Result<Connection, ArkadeError> {
    std::fs::create_dir_all(data_dir).map_err(ArkadeError::storage)?;
    let connection = Connection::open(data_dir.join(DB_FILE))?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS contracts (
             script_pubkey BLOB PRIMARY KEY NOT NULL,
             contract TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS swaps (
             id TEXT PRIMARY KEY NOT NULL,
             record TEXT NOT NULL
         );",
    )?;
    Ok(connection)
}

pub struct SqliteContracts {
    connection: Mutex<Connection>,
}

impl SqliteContracts {
    pub fn open(data_dir: &Path) -> Result<Self, ArkadeError> {
        let connection = open_db(data_dir)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, Error> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| Error::consumer("contract store lock poisoned"))?;
        f(&connection).map_err(|e| Error::consumer(format!("contract store: {e}")))
    }

    fn put(&self, contract: &StoredContract) -> Result<(), Error> {
        let json = serde_json::to_string(contract).map_err(|e| Error::consumer(e.to_string()))?;
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO contracts (script_pubkey, contract) VALUES (?1, ?2)",
                params![contract.script_pubkey.as_bytes(), json],
            )
            .map(|_| ())
        })
    }
}

fn decode(json: String) -> Result<StoredContract, Error> {
    serde_json::from_str(&json).map_err(|e| Error::consumer(format!("stored contract: {e}")))
}

impl ContractStore for SqliteContracts {
    fn insert(&mut self, contract: StoredContract) -> Result<(), Error> {
        if self.get_by_script(&contract.script_pubkey)?.is_some() {
            return Err(Error::consumer("contract script already exists"));
        }
        self.put(&contract)
    }

    fn get_by_script(&self, script_pubkey: &Script) -> Result<Option<StoredContract>, Error> {
        self.with(|c| {
            c.query_row(
                "SELECT contract FROM contracts WHERE script_pubkey = ?1",
                params![script_pubkey.as_bytes()],
                |row| row.get::<_, String>(0),
            )
            .optional()
        })?
        .map(decode)
        .transpose()
    }

    fn list(&self) -> Result<Vec<StoredContract>, Error> {
        self.with(|c| {
            let mut statement = c.prepare("SELECT contract FROM contracts")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()
        })?
        .into_iter()
        .map(decode)
        .collect()
    }

    fn update_state(&mut self, script_pubkey: &Script, state: ContractState) -> Result<(), Error> {
        let mut contract = self
            .get_by_script(script_pubkey)?
            .ok_or_else(|| Error::consumer("unknown contract script"))?;
        contract.state = state;
        self.put(&contract)
    }
}

/// The Lightning swaps' records, in the same database.
pub struct SqliteSwaps {
    connection: Mutex<Connection>,
}

impl SqliteSwaps {
    pub fn open(data_dir: &Path) -> Result<Self, ArkadeError> {
        Ok(Self {
            connection: Mutex::new(open_db(data_dir)?),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> SwapResult<T> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SwapStoreError::Store("swap store lock poisoned".into()))?;
        f(&connection).map_err(|e| SwapStoreError::Store(e.to_string()))
    }
}

fn decode_swap(json: String) -> SwapResult<SwapRecord> {
    serde_json::from_str(&json).map_err(|e| SwapStoreError::Store(format!("stored swap: {e}")))
}

impl SwapStore for SqliteSwaps {
    fn put(&self, record: &SwapRecord) -> SwapResult<()> {
        let json =
            serde_json::to_string(record).map_err(|e| SwapStoreError::Store(e.to_string()))?;
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO swaps (id, record) VALUES (?1, ?2)",
                params![record.id, json],
            )
            .map(|_| ())
        })
    }

    fn get(&self, id: &str) -> SwapResult<Option<SwapRecord>> {
        self.with(|c| {
            c.query_row(
                "SELECT record FROM swaps WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        })?
        .map(decode_swap)
        .transpose()
    }

    fn all(&self) -> SwapResult<Vec<SwapRecord>> {
        let mut all = self
            .with(|c| {
                let mut statement = c.prepare("SELECT record FROM swaps")?;
                let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<Vec<String>>>()
            })?
            .into_iter()
            .map(decode_swap)
            .collect::<SwapResult<Vec<_>>>()?;
        all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Ok(all)
    }
}

#[derive(Serialize, Deserialize)]
struct Dump {
    version: u32,
    contracts: Vec<StoredContract>,
    swaps: Vec<SwapRecord>,
}

pub fn export(data_dir: &Path) -> Result<String, ArkadeError> {
    let contracts = SqliteContracts::open(data_dir)?
        .list()
        .map_err(ArkadeError::storage)?;
    let swaps = SqliteSwaps::open(data_dir)?
        .all()
        .map_err(ArkadeError::storage)?;
    serde_json::to_string(&Dump {
        version: DUMP_VERSION,
        contracts,
        swaps,
    })
    .map_err(ArkadeError::internal)
}

/// Adds the dump's contracts that the store lacks and its swaps that are new or newer; returns how many rows it
/// wrote.
pub fn import(data_dir: &Path, json: &str) -> Result<u64, ArkadeError> {
    let dump: Dump = serde_json::from_str(json)
        .map_err(|e| ArkadeError::invalid(format!("Arkade backup: {e}")))?;
    if dump.version > DUMP_VERSION {
        return Err(ArkadeError::invalid(format!(
            "unknown Arkade backup version {}",
            dump.version
        )));
    }
    let mut store = SqliteContracts::open(data_dir)?;
    let mut added = 0;
    for contract in dump.contracts {
        if store
            .get_by_script(&contract.script_pubkey)
            .map_err(ArkadeError::storage)?
            .is_none()
        {
            store.insert(contract).map_err(ArkadeError::storage)?;
            added += 1;
        }
    }
    // A swap the store already holds is replaced only by a newer copy.
    let swaps = SqliteSwaps::open(data_dir)?;
    for record in dump.swaps {
        let newer = match swaps.get(&record.id).map_err(ArkadeError::storage)? {
            Some(held) => record.updated_at > held.updated_at,
            None => true,
        };
        if newer {
            swaps.put(&record).map_err(ArkadeError::storage)?;
            added += 1;
        }
    }
    Ok(added)
}

pub fn wipe(data_dir: &Path) -> Result<(), ArkadeError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let path = data_dir.join(format!("{DB_FILE}{suffix}"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(ArkadeError::storage)?;
        }
    }
    Ok(())
}
