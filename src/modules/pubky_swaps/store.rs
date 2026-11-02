//! One JSON file per swap under `<base>/pubky_swaps/`.
//!
//! A record holds only public data and the swap's key index; the keys come from the seed and the
//! index. The index is synced to disk before the swap request is sent, and the contract before the
//! invoice is handed to the app.

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
    /// The clock index the swap's claim key and preimage derive from (see `keys`).
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
    /// The signed claim, kept so it can be broadcast again while it is unconfirmed.
    #[serde(default)]
    pub claim_tx_hex: Option<String>,
    /// Whether a broadcast of the claim was ever attempted, so its preimage may be public.
    #[serde(default)]
    pub claim_published: bool,
    /// Whether the server accepted the claim or it showed up in the chain's history.
    #[serde(default)]
    pub claim_seen: bool,
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

/// Take the index for a new swap at `now_unix` and sync it to disk before it is used, or `None`
/// while this second already has a swap. Without a stored index, as after a restore, the clock
/// alone is past every index used before.
pub(crate) fn reserve_index(now_unix: u64) -> Result<Option<u64>, PubkySwapError> {
    let _guard = LOCK.lock().unwrap();
    let path = dir()?.join("last_index");
    let last = match fs::read_to_string(&path) {
        Ok(s) => Some(s.trim().parse().map_err(PubkySwapError::storage)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let Some(index) = super::keys::next_index(last, now_unix) else {
        return Ok(None);
    };
    write_atomic(&path, index.to_string().as_bytes())?;
    Ok(Some(index))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_increasing_indexes_and_never_repeats_after_a_wipe() {
        let base = std::env::temp_dir().join(format!("pubky-swaps-index-{}", uuid::Uuid::new_v4()));
        init(base.to_str().unwrap()).unwrap();
        assert_eq!(reserve_index(1_000).unwrap(), Some(1_000));
        assert_eq!(reserve_index(1_000).unwrap(), None);
        assert_eq!(reserve_index(1_001).unwrap(), Some(1_001));
        // A restore starts without the stored index, in a later second.
        fs::remove_dir_all(&base).unwrap();
        init(base.to_str().unwrap()).unwrap();
        assert_eq!(reserve_index(1_002).unwrap(), Some(1_002));
        fs::remove_dir_all(&base).ok();
    }
}
