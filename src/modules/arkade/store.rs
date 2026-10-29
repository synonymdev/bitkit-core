//! The wallet's lasting state, on bitkit-core's own rusqlite (ark-client's `sqlite` feature would link a second
//! sqlite through sqlx): every contract (address) the wallet handed out, with its HD key index, which the key
//! provider re-derives at connect. VTXOs, boarding outputs and history are read from the operator and the chain.
//!
//! The dump `export` writes is the portable form the app keeps in its VSS backup.

use super::errors::ArkadeError;
use ark_client::{ContractStore, Error};
use ark_core::contract::{ContractState, StoredContract};
use bitcoin::Script;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub const DB_FILE: &str = "arkade.db";
const DUMP_VERSION: u32 = 1;

pub struct SqliteContracts {
    connection: Mutex<Connection>,
}

impl SqliteContracts {
    pub fn open(data_dir: &Path) -> Result<Self, ArkadeError> {
        std::fs::create_dir_all(data_dir).map_err(ArkadeError::storage)?;
        let connection = Connection::open(data_dir.join(DB_FILE))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS contracts (
                 script_pubkey BLOB PRIMARY KEY NOT NULL,
                 contract TEXT NOT NULL
             );",
        )?;
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

#[derive(Serialize, Deserialize)]
struct Dump {
    version: u32,
    contracts: Vec<StoredContract>,
}

pub fn export(data_dir: &Path) -> Result<String, ArkadeError> {
    let contracts = SqliteContracts::open(data_dir)?
        .list()
        .map_err(ArkadeError::storage)?;
    serde_json::to_string(&Dump {
        version: DUMP_VERSION,
        contracts,
    })
    .map_err(ArkadeError::internal)
}

/// Adds the dump's contracts that the store lacks; returns how many it added.
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
