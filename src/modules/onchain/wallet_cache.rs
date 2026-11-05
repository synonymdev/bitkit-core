//! Process-wide cache of synced watch-only wallets for offline transaction composition.
//!
//! A cold BDK sync fetches every script history, every wallet transaction and
//! every input's parent transaction. Keeping the synced in-memory database lets
//! the next call do an incremental sync: BDK reuses the stored transactions and
//! only fetches what changed. Every caller still syncs before reading the
//! wallet, so UTXOs are never older than the call.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Instant;

use bdk::bitcoin::Network as BdkNetwork;
use bdk::blockchain::ElectrumBlockchain;
use bdk::database::{BatchOperations, Database, MemoryDatabase};
use bdk::wallet::Wallet;
use bdk::KeychainKind;
use once_cell::sync::OnceCell;

use super::errors::AccountInfoError;
use super::implementation::{connect_electrum, create_wallet, sync_wallet, WalletSetup};

/// Maximum number of wallets kept. The least recently used one is dropped
/// when a new key would exceed it.
pub(crate) const MAX_CACHED_WALLETS: usize = 8;

/// Identifies one cached wallet. The descriptors carry the extended key and
/// script type; key origins are left out so callers with and without a
/// fingerprint share one wallet.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WalletCacheKey {
    external_desc: String,
    internal_desc: String,
    network: BdkNetwork,
    electrum_url: String,
}

impl WalletCacheKey {
    /// `sync_setup` must be resolved without a fingerprint.
    pub(crate) fn new(sync_setup: &WalletSetup, electrum_url: &str) -> Self {
        Self {
            external_desc: sync_setup.external_desc.clone(),
            internal_desc: sync_setup.internal_desc.clone(),
            network: sync_setup.network,
            electrum_url: electrum_url.to_string(),
        }
    }
}

type Slot = Arc<StdMutex<Option<Wallet<MemoryDatabase>>>>;

struct Entry {
    slot: Slot,
    last_used: Instant,
}

static CACHE: OnceCell<StdMutex<HashMap<WalletCacheKey, Entry>>> = OnceCell::new();

fn cache() -> &'static StdMutex<HashMap<WalletCacheKey, Entry>> {
    CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn lock_map() -> MutexGuard<'static, HashMap<WalletCacheKey, Entry>> {
    // The map is only mutated by short, non-panicking sections; recover from
    // poisoning instead of failing every later compose.
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Get the slot for `key`, inserting an empty one and evicting the least
/// recently used entry if needed.
fn slot_for(key: &WalletCacheKey) -> Slot {
    let mut map = lock_map();
    if let Some(entry) = map.get_mut(key) {
        entry.last_used = Instant::now();
        return entry.slot.clone();
    }
    while map.len() >= MAX_CACHED_WALLETS {
        let oldest = map
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(k, _)| k.clone());
        match oldest {
            Some(k) => map.remove(&k),
            None => break,
        };
    }
    let slot: Slot = Arc::new(StdMutex::new(None));
    map.insert(
        key.clone(),
        Entry {
            slot: slot.clone(),
            last_used: Instant::now(),
        },
    );
    slot
}

/// Sync the cached wallet for `sync_setup` (creating it on first use) and
/// return a copy of its database taken right after the sync.
///
/// Calls for the same key are serialized on the wallet, so concurrent
/// composes never sync one wallet twice at once. The copy lets callers build
/// transactions without touching the cached wallet: building a transaction
/// advances the change index, and the cached wallet must keep the index the
/// server reports. A failed sync drops the cached wallet so the next call
/// starts cold. A new Electrum connection is opened per call, as before;
/// only the wallet data is reused.
pub(crate) fn synced_database_snapshot(
    sync_setup: &WalletSetup,
    electrum_url: &str,
) -> Result<MemoryDatabase, AccountInfoError> {
    let key = WalletCacheKey::new(sync_setup, electrum_url);
    let slot = slot_for(&key);
    let mut guard = slot.lock().unwrap_or_else(|poisoned| {
        // A panic while syncing may have left a half-updated wallet.
        let mut guard = poisoned.into_inner();
        *guard = None;
        guard
    });

    // Connect before taking the wallet so a connection failure keeps it.
    let blockchain = ElectrumBlockchain::from(connect_electrum(electrum_url)?);
    let wallet = match guard.take() {
        Some(wallet) => wallet,
        None => create_wallet(sync_setup)?,
    };
    sync_wallet(&wallet, &blockchain)?;

    let snapshot = copy_database(&wallet.database())?;
    *guard = Some(wallet);
    Ok(snapshot)
}

/// Copy everything a [`Wallet`] needs to build and sign transactions:
/// cached scripts, UTXOs, transactions, last derivation indexes and the
/// sync time. Descriptor checksums are left out so the copy can back a
/// wallet whose descriptors carry key origins.
pub(crate) fn copy_database(src: &MemoryDatabase) -> Result<MemoryDatabase, AccountInfoError> {
    let map_err = |e: bdk::Error| AccountInfoError::WalletError {
        error_details: format!("Failed to copy wallet database: {}", e),
    };
    let mut dst = MemoryDatabase::new();

    for script in src.iter_script_pubkeys(None).map_err(map_err)? {
        if let Some((keychain, child)) =
            src.get_path_from_script_pubkey(&script).map_err(map_err)?
        {
            dst.set_script_pubkey(&script, keychain, child)
                .map_err(map_err)?;
        }
    }
    for utxo in src.iter_utxos().map_err(map_err)? {
        dst.set_utxo(&utxo).map_err(map_err)?;
    }
    for tx in src.iter_raw_txs().map_err(map_err)? {
        dst.set_raw_tx(&tx).map_err(map_err)?;
    }
    for tx in src.iter_txs(false).map_err(map_err)? {
        dst.set_tx(&tx).map_err(map_err)?;
    }
    for keychain in [KeychainKind::External, KeychainKind::Internal] {
        if let Some(index) = src.get_last_index(keychain).map_err(map_err)? {
            dst.set_last_index(keychain, index).map_err(map_err)?;
        }
    }
    if let Some(sync_time) = src.get_sync_time().map_err(map_err)? {
        dst.set_sync_time(sync_time).map_err(map_err)?;
    }
    Ok(dst)
}

#[cfg(test)]
pub(crate) fn cached_wallet_count() -> usize {
    lock_map().len()
}

#[cfg(test)]
pub(crate) fn contains_key(key: &WalletCacheKey) -> bool {
    lock_map().contains_key(key)
}

#[cfg(test)]
pub(crate) fn insert_empty_for_test(key: &WalletCacheKey) {
    slot_for(key);
}
