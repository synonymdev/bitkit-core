//! One JSON file per swap under `<base>/pubky_swaps/`.
//!
//! A record holds only public data and the swap's key nonce; the keys come from the seed and the
//! nonce. The contract is written, and synced to disk, before the invoice is handed to the app.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use swap_common::messages::{Quote, SwapAccept};

use super::errors::PubkySwapError;

/// The current base path; `init_db` may move it, like every other store it opens.
static BASE: RwLock<Option<PathBuf>> = RwLock::new(None);
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SwapRecord {
    pub swap_id: String,
    /// Hex of the random nonce the swap's claim key and preimage derive from.
    pub key_nonce: String,
    pub provider_pubky: String,
    pub network: bitcoin::Network,
    pub electrum_url: String,
    pub quote: Quote,
    pub accept: SwapAccept,
    pub destination_address: String,
    pub delivered_sat: u64,
    pub claim_fee_sat: u64,
    pub claim_txid: Option<String>,
    /// The signed claim, kept so it can be broadcast again while it is unconfirmed.
    #[serde(default)]
    pub claim_tx_hex: Option<String>,
    pub created_at_unix: u64,
}

pub(crate) fn init(base_path: &str) -> Result<(), PubkySwapError> {
    let dir = Path::new(base_path).join("pubky_swaps");
    fs::create_dir_all(&dir)?;
    *BASE.write().unwrap() = Some(dir);
    Ok(())
}

fn dir() -> Result<PathBuf, PubkySwapError> {
    BASE.read()
        .unwrap()
        .clone()
        .ok_or_else(|| PubkySwapError::storage("Database not initialized. Call init_db first."))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PubkySwapError> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
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
