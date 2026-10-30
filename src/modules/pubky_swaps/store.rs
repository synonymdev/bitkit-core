//! One JSON file per swap under `<base>/pubky_swaps/`, plus the next key index.
//!
//! A record holds only public data and the key index; the keys come from the seed. Every
//! record is written before the step it protects: the index before the commit is sent, the
//! contract before the invoice is handed to the app.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use swap_common::messages::{Quote, SwapAccept};

use super::errors::PubkySwapError;

static BASE: OnceCell<PathBuf> = OnceCell::new();
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SwapRecord {
    pub swap_id: String,
    pub key_index: u64,
    pub provider_pubky: String,
    pub network: bitcoin::Network,
    pub electrum_url: String,
    pub quote: Quote,
    pub accept: SwapAccept,
    pub destination_address: String,
    pub delivered_sat: u64,
    pub claim_fee_sat: u64,
    pub claim_txid: Option<String>,
    pub created_at_unix: u64,
}

pub(crate) fn init(base_path: &str) -> Result<(), PubkySwapError> {
    let dir = Path::new(base_path).join("pubky_swaps");
    fs::create_dir_all(&dir)?;
    let _ = BASE.set(dir);
    Ok(())
}

fn dir() -> Result<&'static PathBuf, PubkySwapError> {
    BASE.get()
        .ok_or_else(|| PubkySwapError::storage("Database not initialized. Call init_db first."))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PubkySwapError> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// Reserve the next key index, persisted before it is used.
pub(crate) fn reserve_index() -> Result<u64, PubkySwapError> {
    let _guard = LOCK.lock().unwrap();
    let path = dir()?.join("next_index");
    let index = match fs::read_to_string(&path) {
        Ok(s) => s.trim().parse().map_err(PubkySwapError::storage)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => return Err(e.into()),
    };
    write_atomic(&path, (index + 1).to_string().as_bytes())?;
    Ok(index)
}

pub(crate) fn save(record: &SwapRecord) -> Result<(), PubkySwapError> {
    let _guard = LOCK.lock().unwrap();
    let path = dir()?.join(format!("{}.json", record.swap_id));
    write_atomic(&path, &serde_json::to_vec_pretty(record)?)
}

pub(crate) fn load(swap_id: &str) -> Result<SwapRecord, PubkySwapError> {
    let path = dir()?.join(format!("{swap_id}.json"));
    match fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(PubkySwapError::NotFound {
            error_details: swap_id.to_string(),
        }),
        Err(e) => Err(e.into()),
    }
}
