use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use bdk::bitcoin::ScriptBuf;
use bdk::blockchain::ElectrumBlockchain;
use bdk::database::MemoryDatabase;
use bdk::electrum_client::{self, ConfigBuilder, ElectrumApi};
use bdk::wallet::{AddressIndex as BdkAddressIndex, SyncOptions, Wallet};
use once_cell::sync::OnceCell;
use tokio::sync::{oneshot, watch};

use super::errors::AccountInfoError;
use super::implementation::{
    connect_electrum, create_and_sync_wallet, map_bdk_tx_to_history, resolve_wallet_setup,
    sort_history_transactions,
};
use super::types::{AccountType, Network as OnchainNetwork, WalletBalance, WatcherEvent};

// ============================================================================
// Callback trait
// ============================================================================

/// Callback interface for receiving watcher events.
///
/// Implement this trait in Swift/Kotlin/Python to receive typed notifications
/// from xpub watchers.
#[uniffi::export(with_foreign)]
pub trait EventListener: Send + Sync {
    /// Called when a watcher event occurs.
    ///
    /// `watcher_id` identifies which watcher produced the event.
    /// `event` is a typed enum — no JSON parsing needed.
    fn on_event(&self, watcher_id: String, event: WatcherEvent);
}

// ============================================================================
// Watcher configuration
// ============================================================================

/// Parameters for starting an xpub transaction watcher.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WatcherParams {
    /// Caller-supplied identifier for this watcher.
    pub watcher_id: String,
    /// Extended public key (xpub/ypub/zpub/tpub/upub/vpub).
    pub extended_key: String,
    /// Electrum server URL (e.g. "ssl://electrum.example.com:50002").
    pub electrum_url: String,
    /// Bitcoin network override (auto-detected from key prefix if None).
    pub network: Option<OnchainNetwork>,
    /// Account type override (auto-detected from key prefix if None).
    pub account_type: Option<AccountType>,
    /// Number of unused addresses to monitor beyond the last used (default 20).
    pub gap_limit: Option<u32>,
}

// ============================================================================
// Internal watcher handle
// ============================================================================

struct WatcherHandle {
    shutdown_tx: watch::Sender<bool>,
}

// ============================================================================
// Global state
// ============================================================================

static WATCHERS: OnceCell<StdMutex<HashMap<String, WatcherHandle>>> = OnceCell::new();

fn get_watchers() -> &'static StdMutex<HashMap<String, WatcherHandle>> {
    WATCHERS.get_or_init(|| StdMutex::new(HashMap::new()))
}

// ============================================================================
// Public API
// ============================================================================

/// Start monitoring an xpub for transaction activity.
///
/// Uses Electrum `blockchain.scripthash.subscribe` for near-instant detection.
/// The server pushes notifications when any subscribed script's history changes.
/// A lightweight `ping()` call with a short socket timeout is used to drain
/// those notifications from the socket — no fixed poll interval needed.
///
/// Returns `Err` if the initial connection, wallet sync, or script derivation
/// fails — the caller can rely on `Ok(())` meaning the watcher is running.
pub async fn start_watcher(
    params: WatcherParams,
    listener: Arc<dyn EventListener>,
) -> Result<(), AccountInfoError> {
    let setup = resolve_wallet_setup(
        &params.extended_key,
        params.network,
        params.account_type,
        None,
    )?;

    let watcher_id = params.watcher_id.clone();

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Atomically check-and-insert to avoid race condition
    {
        let mut watchers = get_watchers()
            .lock()
            .map_err(|_| AccountInfoError::WatcherError {
                error_details: "Failed to acquire watchers lock".to_string(),
            })?;
        match watchers.entry(watcher_id.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(AccountInfoError::WatcherError {
                    error_details: format!("Watcher '{}' is already running", watcher_id),
                });
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(WatcherHandle { shutdown_tx });
            }
        }
    }

    // Wallet<MemoryDatabase> and electrum_client::Client are !Send,
    // so init and the poll loop must run on the same blocking thread.
    let (init_tx, init_rx) = oneshot::channel::<Result<(), AccountInfoError>>();

    let watcher_id_for_cleanup = watcher_id.clone();
    tokio::task::spawn_blocking(move || {
        watcher_init_and_loop(params, setup, shutdown_rx, listener, init_tx);
        if let Ok(mut watchers) = get_watchers().lock() {
            watchers.remove(&watcher_id_for_cleanup);
        }
    });

    let init_result = init_rx.await.unwrap_or_else(|_| {
        Err(AccountInfoError::WatcherError {
            error_details: "Watcher task exited before reporting init status".to_string(),
        })
    });

    if init_result.is_err() {
        if let Ok(mut watchers) = get_watchers().lock() {
            watchers.remove(&watcher_id);
        }
    }

    init_result
}

/// Stop a specific watcher by ID.
pub fn stop_watcher(watcher_id: &str) -> Result<(), AccountInfoError> {
    let mut watchers = get_watchers()
        .lock()
        .map_err(|_| AccountInfoError::WatcherError {
            error_details: "Failed to acquire watchers lock".to_string(),
        })?;

    match watchers.remove(watcher_id) {
        Some(handle) => {
            let _ = handle.shutdown_tx.send(true);
            Ok(())
        }
        None => Err(AccountInfoError::WatcherError {
            error_details: format!("No watcher found with ID '{}'", watcher_id),
        }),
    }
}

/// Stop all active watchers.
pub fn stop_all_watchers() {
    if let Ok(mut watchers) = get_watchers().lock() {
        for (_, handle) in watchers.drain() {
            let _ = handle.shutdown_tx.send(true);
        }
    }
}

// ============================================================================
// Watcher internals (runs on a single blocking thread)
// ============================================================================

/// Socket read timeout for the subscription client (seconds).
///
/// Controls the maximum idle interval between `server.ping` calls.
///
/// When `ping()` is called, it blocks on the socket waiting for the server's
/// response. While blocked, any push notification that arrives (from
/// `blockchain.scripthash.subscribe`) is read and queued immediately.
///
/// - If a notification arrives during the window: `ping()` returns as soon as
///   the ping response follows, we process the notification, then send a new
///   `ping()` — which blocks again for up to this duration.
/// - If idle (no notifications): `ping()` times out after this duration, we
///   check for shutdown, and loop back to send another `ping()`.
///
/// Net effect: ~1 ping per SOCKET_TIMEOUT_SECS when idle. After a notification
/// burst, one extra ping is sent immediately, then it settles back to idle rate.
/// This is well within Electrum server rate limits.
const SOCKET_TIMEOUT_SECS: u8 = 10;

/// Create an Electrum client with a read timeout for subscription use.
///
/// The timeout ensures `ping()` returns periodically so we can check for
/// shutdown and drain notification queues, rather than blocking forever.
fn connect_electrum_with_timeout(
    electrum_url: &str,
) -> Result<electrum_client::Client, AccountInfoError> {
    let config = ConfigBuilder::new()
        .timeout(Some(SOCKET_TIMEOUT_SECS))
        .build();
    electrum_client::Client::from_config(electrum_url, config).map_err(|e| {
        AccountInfoError::ElectrumError {
            error_details: format!("Failed to connect to Electrum: {}", e),
        }
    })
}

/// Check whether an electrum_client error is a socket timeout (not a real disconnect).
fn is_timeout_error(err: &electrum_client::Error) -> bool {
    match err {
        electrum_client::Error::IOError(e) => matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ),
        electrum_client::Error::SharedIOError(e) => matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ),
        _ => false,
    }
}

/// Derive scriptPubKeys for external and internal keychains up to the gap limit.
fn derive_scripts(
    wallet: &Wallet<MemoryDatabase>,
    gap: u32,
) -> Result<HashSet<ScriptBuf>, AccountInfoError> {
    let mut scripts = HashSet::new();

    let next_external = wallet
        .get_address(BdkAddressIndex::LastUnused)
        .map_err(|e| AccountInfoError::WalletError {
            error_details: format!("Failed to get last unused external address: {}", e),
        })?
        .index;
    for i in 0..(next_external + gap) {
        let addr = wallet.get_address(BdkAddressIndex::Peek(i)).map_err(|e| {
            AccountInfoError::WalletError {
                error_details: format!("Failed to derive external address {}: {}", i, e),
            }
        })?;
        scripts.insert(addr.address.script_pubkey());
    }

    let next_internal = wallet
        .get_internal_address(BdkAddressIndex::LastUnused)
        .map_err(|e| AccountInfoError::WalletError {
            error_details: format!("Failed to get last unused internal address: {}", e),
        })?
        .index;
    for i in 0..(next_internal + gap) {
        let addr = wallet
            .get_internal_address(BdkAddressIndex::Peek(i))
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to derive internal address {}: {}", i, e),
            })?;
        scripts.insert(addr.address.script_pubkey());
    }

    Ok(scripts)
}

/// Build a TransactionsChanged event from a synced wallet.
fn build_tx_changed_event(
    wallet: &Wallet<MemoryDatabase>,
    tip_height: u32,
    account_type: AccountType,
) -> WatcherEvent {
    let bdk_balance = match wallet.get_balance() {
        Ok(b) => b,
        Err(e) => {
            return WatcherEvent::Error {
                message: format!("Failed to get balance: {}", e),
            };
        }
    };
    let balance: WalletBalance = bdk_balance.into();

    let txs = match wallet.list_transactions(false) {
        Ok(t) => t,
        Err(e) => {
            return WatcherEvent::Error {
                message: format!("Failed to list transactions: {}", e),
            };
        }
    };

    let mut history = txs
        .iter()
        .map(|tx| map_bdk_tx_to_history(tx, tip_height))
        .collect();
    sort_history_transactions(&mut history);

    let tx_count = u32::try_from(history.len()).unwrap_or(u32::MAX);

    WatcherEvent::TransactionsChanged {
        transactions: history,
        balance,
        tx_count,
        block_height: tip_height,
        account_type,
    }
}

/// Subscribe to all derived scripts on the Electrum client.
fn subscribe_scripts(client: &electrum_client::Client, scripts: &HashSet<ScriptBuf>) {
    for script in scripts {
        let _ = client.script_subscribe(script);
    }
}

/// Attempt to reconnect to the Electrum server with exponential backoff.
/// Returns the new client once connected, or None if shutdown was requested.
fn reconnect_loop(
    electrum_url: &str,
    shutdown_rx: &watch::Receiver<bool>,
    scripts: &HashSet<ScriptBuf>,
) -> Option<electrum_client::Client> {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        // Check shutdown before sleeping
        if *shutdown_rx.borrow() {
            return None;
        }

        // Sleep in 1-second chunks so we can respond to shutdown promptly
        let mut remaining = backoff.as_millis() as u64;
        while remaining > 0 {
            if *shutdown_rx.borrow() {
                return None;
            }
            let chunk = remaining.min(1000);
            std::thread::sleep(Duration::from_millis(chunk));
            remaining = remaining.saturating_sub(chunk);
        }

        match connect_electrum_with_timeout(electrum_url) {
            Ok(client) => {
                subscribe_scripts(&client, scripts);
                let _ = client.block_headers_subscribe();
                return Some(client);
            }
            Err(_) => {
                backoff = std::cmp::min(backoff * 2, max_backoff);
            }
        }
    }
}

/// Re-sync an existing wallet with a fresh Electrum connection.
///
/// A new connection is required because `ElectrumBlockchain::from()` consumes
/// the client, and we need to keep the subscription client alive separately.
fn resync_wallet(
    wallet: &Wallet<MemoryDatabase>,
    electrum_url: &str,
) -> Result<(), AccountInfoError> {
    let client = connect_electrum(electrum_url)?;
    let blockchain = ElectrumBlockchain::from(client);
    wallet
        .sync(&blockchain, SyncOptions::default())
        .map_err(|e| AccountInfoError::SyncError {
            error_details: format!("Re-sync failed: {}", e),
        })?;
    Ok(())
}

/// Initialize the watcher and enter the notification loop.
///
/// Reports init success/failure back to the caller via `init_tx`.
/// Runs entirely on a single blocking thread because `Wallet<MemoryDatabase>`
/// and `electrum_client::Client` are `!Send`.
///
/// ## How notifications work
///
/// Electrum's `blockchain.scripthash.subscribe` causes the server to push a
/// notification whenever a subscribed script's history changes (new tx, confirmation, etc.).
/// These arrive as JSON-RPC messages on the same TCP connection.
///
/// The `electrum_client` crate reads notifications off the socket whenever any
/// `call()` is in progress (including `ping()`). Notifications are queued in memory
/// and retrieved via `script_pop()`.
///
/// We use a short socket timeout (`SOCKET_TIMEOUT_SECS`) so that `ping()` blocks
/// on the socket for up to that duration. If a notification arrives during this
/// window, it's processed immediately — giving near-instant detection without
/// CPU-burning polling. The timeout just ensures we periodically wake up to check
/// for shutdown.
fn watcher_init_and_loop(
    params: WatcherParams,
    setup: super::implementation::WalletSetup,
    shutdown_rx: watch::Receiver<bool>,
    listener: Arc<dyn EventListener>,
    init_tx: oneshot::Sender<Result<(), AccountInfoError>>,
) {
    let gap = params.gap_limit.unwrap_or(20);
    let account_type = setup.account_type;
    let watcher_id = params.watcher_id.clone();

    // --- Initialization ---

    let sub_client = match connect_electrum_with_timeout(&params.electrum_url) {
        Ok(c) => c,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let tip_height = match sub_client.block_headers_subscribe() {
        Ok(header) => u32::try_from(header.height).unwrap_or(0),
        Err(e) => {
            let _ = init_tx.send(Err(AccountInfoError::ElectrumError {
                error_details: format!("Failed to subscribe to headers: {}", e),
            }));
            return;
        }
    };

    // Separate client for initial BDK sync (consumed by ElectrumBlockchain::from)
    let sync_client = match connect_electrum(&params.electrum_url) {
        Ok(c) => c,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let wallet = match create_and_sync_wallet(&setup, sync_client) {
        Ok(w) => w,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let mut subscribed_scripts = match derive_scripts(&wallet, gap) {
        Ok(s) => s,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };
    subscribe_scripts(&sub_client, &subscribed_scripts);

    // Init succeeded
    let _ = init_tx.send(Ok(()));

    // Send initial state
    listener.on_event(
        watcher_id.clone(),
        build_tx_changed_event(&wallet, tip_height, account_type),
    );

    // Track the last-used indices to detect gap limit extensions
    let mut last_external = wallet
        .get_address(BdkAddressIndex::LastUnused)
        .map(|info| info.index)
        .unwrap_or(0);
    let mut last_internal = wallet
        .get_internal_address(BdkAddressIndex::LastUnused)
        .map(|info| info.index)
        .unwrap_or(0);

    // --- Notification loop ---
    //
    // Each iteration sends a single `server.ping` which blocks on the socket
    // for up to SOCKET_TIMEOUT_SECS (10s). While blocked, any push notification
    // from `blockchain.scripthash.subscribe` is read off the wire and queued.
    //
    // After ping returns (or times out), we drain the queues. If changes were
    // detected, we resync the wallet and emit an event, then loop back to ping.
    //
    // Server traffic: ~1 ping per 10s when idle. If a notification arrives,
    // one extra ping is sent immediately after processing, then back to idle.
    // No aggressive polling — detection latency ≈ server push delay (<1s).

    let mut sub_client = sub_client;
    let mut tip_height = tip_height;

    loop {
        if *shutdown_rx.borrow() {
            for script in &subscribed_scripts {
                let _ = sub_client.script_unsubscribe(script);
            }
            return;
        }

        // ping() serves two purposes:
        // 1. Keeps the TCP connection alive
        // 2. Blocks on the socket, reading and dispatching any push notifications
        //    that arrive before/with the ping response
        //
        // With SOCKET_TIMEOUT_SECS timeout, this either:
        // - Returns Ok(()) quickly if there's socket activity (notifications!)
        // - Returns Err(timeout) after SOCKET_TIMEOUT_SECS if idle
        // - Returns Err(other) on real connection failure
        match sub_client.ping() {
            Ok(()) => {}
            Err(ref e) if is_timeout_error(e) => {
                // Socket timeout — no activity, just loop again
                continue;
            }
            Err(e) => {
                // Real connection failure
                listener.on_event(
                    watcher_id.clone(),
                    WatcherEvent::Disconnected {
                        message: format!("Connection lost: {}", e),
                    },
                );

                match reconnect_loop(&params.electrum_url, &shutdown_rx, &subscribed_scripts) {
                    Some(new_client) => {
                        sub_client = new_client;
                        listener.on_event(watcher_id.clone(), WatcherEvent::Reconnected);
                        // Force a resync after reconnect
                        if let Err(e) = resync_wallet(&wallet, &params.electrum_url) {
                            listener.on_event(
                                watcher_id.clone(),
                                WatcherEvent::Error {
                                    message: format!("{}", e),
                                },
                            );
                        } else {
                            listener.on_event(
                                watcher_id.clone(),
                                build_tx_changed_event(&wallet, tip_height, account_type),
                            );
                        }
                        continue;
                    }
                    None => return, // Shutdown requested
                }
            }
        }

        // Drain block header notifications
        let mut has_changes = false;
        while let Ok(Some(header)) = sub_client.block_headers_pop_raw() {
            let new_height = u32::try_from(header.height).unwrap_or(tip_height);
            if new_height > tip_height {
                tip_height = new_height;
                has_changes = true;
            }
        }

        // Drain script notifications — these arrive via server push from
        // blockchain.scripthash.subscribe, already queued by the ping() call above
        for script in &subscribed_scripts {
            match sub_client.script_pop(script) {
                Ok(Some(_)) => {
                    has_changes = true;
                }
                Ok(None) => {}
                Err(e) => {
                    listener.on_event(
                        watcher_id.clone(),
                        WatcherEvent::Disconnected {
                            message: format!("script_pop error: {}", e),
                        },
                    );
                    match reconnect_loop(&params.electrum_url, &shutdown_rx, &subscribed_scripts) {
                        Some(new_client) => {
                            sub_client = new_client;
                            listener.on_event(watcher_id.clone(), WatcherEvent::Reconnected);
                            has_changes = true;
                            break;
                        }
                        None => return,
                    }
                }
            }
        }

        if has_changes {
            if let Err(e) = resync_wallet(&wallet, &params.electrum_url) {
                listener.on_event(
                    watcher_id.clone(),
                    WatcherEvent::Error {
                        message: format!("{}", e),
                    },
                );
                continue;
            }

            // Extend gap limit if new addresses were used
            let new_external = wallet
                .get_address(BdkAddressIndex::LastUnused)
                .map(|info| info.index)
                .unwrap_or(last_external);
            let new_internal = wallet
                .get_internal_address(BdkAddressIndex::LastUnused)
                .map(|info| info.index)
                .unwrap_or(last_internal);

            if new_external > last_external {
                for i in (last_external + gap)..(new_external + gap) {
                    if let Ok(addr) = wallet.get_address(BdkAddressIndex::Peek(i)) {
                        let script = addr.address.script_pubkey();
                        let _ = sub_client.script_subscribe(&script);
                        subscribed_scripts.insert(script);
                    }
                }
                last_external = new_external;
            }

            if new_internal > last_internal {
                for i in (last_internal + gap)..(new_internal + gap) {
                    if let Ok(addr) = wallet.get_internal_address(BdkAddressIndex::Peek(i)) {
                        let script = addr.address.script_pubkey();
                        let _ = sub_client.script_subscribe(&script);
                        subscribed_scripts.insert(script);
                    }
                }
                last_internal = new_internal;
            }

            listener.on_event(
                watcher_id.clone(),
                build_tx_changed_event(&wallet, tip_height, account_type),
            );
        }
    }
}
