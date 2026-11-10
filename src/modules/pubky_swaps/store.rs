//! One JSON file per swap under `<base>/pubky_swaps/`.
//!
//! A record holds the swap's key index, not its keys, which come from the seed and the index. Its
//! saved claim does carry the preimage, before the claim is broadcast too. The contract is synced
//! to disk before the invoice is handed to the app.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use swap_common::messages::{Quote, SwapAccept};
use swap_common::paykit_cap::ContractReply;

use super::errors::PubkySwapError;

/// The current base path; `init_db` may move it, like every other store it opens.
static BASE: RwLock<Option<PathBuf>> = RwLock::new(None);
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SwapRecord {
    pub swap_id: String,
    /// Hex of the random index the swap's claim key and preimage derive from (see `keys`).
    pub key_index: String,
    pub provider_pubky: String,
    pub network: bitcoin::Network,
    pub electrum_url: String,
    /// The quote this wallet committed to; absent for a swap rebuilt from a provider's listing.
    #[serde(default)]
    pub quote: Option<Quote>,
    /// Lockup confirmations the provider requires before the claim.
    pub required_confirmations: u32,
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
    /// A restored swap whose provider listed a claim it saw, which the chain has not shown, and
    /// whose payment the wallet's node shows settled. Its preimage is public, so the wallet claims
    /// once the lockup has its confirmations, whatever the window.
    #[serde(default)]
    pub claim_listed: bool,
    /// The contract the provider signed over this wallet's commit; absent for a swap rebuilt
    /// from a provider's listing.
    #[serde(default)]
    pub contract: Option<ContractReply>,
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

/// Saves a new swap, refusing an id already on file: the provider names the id, so an acceptance
/// that reused an earlier swap's would otherwise replace its contract, key index and saved claim.
pub(crate) fn create(record: &SwapRecord) -> Result<(), PubkySwapError> {
    let _guard = LOCK.lock().unwrap();
    let path = dir()?.join(format!("{}.json", record.swap_id));
    if path.try_exists()? {
        return Err(PubkySwapError::storage(format!(
            "swap {} is already on file",
            record.swap_id
        )));
    }
    write_atomic(&path, &serde_json::to_vec_pretty(record)?)
}

/// Every record on file. A file that cannot be read is skipped with a warning.
pub(crate) fn list() -> Result<Vec<SwapRecord>, PubkySwapError> {
    let mut records = vec![];
    for entry in fs::read_dir(dir()?)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            match fs::read(&path)
                .map_err(PubkySwapError::from)
                .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?))
            {
                Ok(record) => records.push(record),
                Err(e) => log::warn!("pubky swaps: skipping {}: {e}", path.display()),
            }
        }
    }
    Ok(records)
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

/// Removes every swap record from disk.
pub(crate) fn wipe_all() -> Result<(), PubkySwapError> {
    let _guard = LOCK.lock().unwrap();
    let dir = dir()?;
    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                fs::remove_file(&path)?;
            } else if path.is_dir() {
                fs::remove_dir_all(&path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swap_common::messages::{SwapDirection, SwapScript};
    use uuid::Uuid;

    fn sample_record(swap_id: &str) -> SwapRecord {
        let provider_secret = bitcoin::secp256k1::SecretKey::from_slice(&[9; 32]).unwrap();
        let provider = bitcoin::PublicKey::new(
            provider_secret.public_key(&bitcoin::secp256k1::Secp256k1::new()),
        );
        SwapRecord {
            swap_id: swap_id.to_string(),
            key_index: hex::encode([7; 32]),
            provider_pubky: "pk:test_provider".to_string(),
            network: bitcoin::Network::Regtest,
            electrum_url: "tcp://127.0.0.1:60001".to_string(),
            quote: None,
            required_confirmations: 1,
            accept: SwapAccept {
                script_type: SwapScript::P2wsh,
                swap_tree: None,
                quote_id: Uuid::new_v4(),
                swap_id: Uuid::new_v4(),
                direction: SwapDirection::Reverse,
                htlc_script_hex: String::new(),
                htlc_address: String::new(),
                onchain_amount_sat: 20_000,
                timeout_block_height: 1_200,
                provider_pubkey_hex: provider.to_string(),
                invoice: None,
            },
            destination_address: "bcrt1qtest".to_string(),
            delivered_sat: 20_000,
            claim_fee_sat: 200,
            claim_txid: None,
            claim_tx_hex: None,
            claim_published: false,
            claim_seen: false,
            claim_listed: false,
            contract: None,
            created_at_unix: 1_700_000_000,
        }
    }

    #[test]
    fn test_store_wipe_all() {
        let temp_dir =
            std::env::temp_dir().join(format!("pubky_swaps_store_test_{}", rand::random::<u64>()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        init(temp_dir.to_str().unwrap()).unwrap();

        let rec1 = sample_record("swap-1");
        let rec2 = sample_record("swap-2");
        create(&rec1).unwrap();
        create(&rec2).unwrap();

        let listed = list().unwrap();
        assert_eq!(listed.len(), 2);
        assert!(load("swap-1").is_ok());
        assert!(load("swap-2").is_ok());

        wipe_all().unwrap();

        let listed_after = list().unwrap();
        assert_eq!(listed_after.len(), 0);
        assert!(load("swap-1").is_err());
        assert!(load("swap-2").is_err());

        // Can create again after wipe
        create(&rec1).unwrap();
        assert_eq!(list().unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
