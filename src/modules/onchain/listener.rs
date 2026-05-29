use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use bdk::bitcoin::ScriptBuf;
use bdk::blockchain::{ConfigurableBlockchain, ElectrumBlockchain, ElectrumBlockchainConfig};
use bdk::database::MemoryDatabase;
use bdk::electrum_client::{self, ConfigBuilder, ElectrumApi};
use bdk::wallet::{AddressIndex as BdkAddressIndex, Wallet};
use once_cell::sync::OnceCell;
use tokio::sync::{oneshot, watch};

use super::errors::AccountInfoError;
use super::implementation::{
    create_wallet, map_bdk_tx_to_history, resolve_wallet_setup, sort_history_transactions,
    sync_wallet,
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
    /// Distinguishes watcher generations that reuse the same `watcher_id`, so a
    /// watcher's own teardown can't evict a newer watcher with the same id.
    generation: u64,
}

// ============================================================================
// Global state
// ============================================================================

static WATCHERS: OnceCell<StdMutex<HashMap<String, WatcherHandle>>> = OnceCell::new();
static WATCHER_GENERATION: AtomicU64 = AtomicU64::new(0);

fn get_watchers() -> &'static StdMutex<HashMap<String, WatcherHandle>> {
    WATCHERS.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Remove a watcher entry, but only if it still belongs to `generation`.
///
/// Guards against a watcher's own teardown evicting a newer watcher that was
/// started under the same `watcher_id` in the teardown window.
fn remove_watcher_if_generation(watcher_id: &str, generation: u64) {
    if let Ok(mut watchers) = get_watchers().lock() {
        if watchers.get(watcher_id).map(|h| h.generation) == Some(generation) {
            watchers.remove(watcher_id);
        }
    }
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
    let generation = WATCHER_GENERATION.fetch_add(1, Ordering::Relaxed);

    // Atomically check-and-insert to avoid a duplicate-id race.
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
                entry.insert(WatcherHandle {
                    shutdown_tx,
                    generation,
                });
            }
        }
    }

    // `Wallet<MemoryDatabase>` and `electrum_client::Client` are `!Send`, so the
    // entire init + poll loop runs on one dedicated OS thread. We use a plain
    // `std` thread rather than `tokio::task::spawn_blocking` because the loop
    // runs for the watcher's whole lifetime — a forever-task would permanently
    // occupy a slot in Tokio's bounded blocking pool.
    let (init_tx, init_rx) = oneshot::channel::<Result<(), AccountInfoError>>();

    let watcher_id_for_cleanup = watcher_id.clone();
    let spawn_result = std::thread::Builder::new()
        .name(format!("watcher-{}", watcher_id))
        .spawn(move || {
            watcher_init_and_loop(params, setup, shutdown_rx, listener, init_tx);
            remove_watcher_if_generation(&watcher_id_for_cleanup, generation);
        });

    if let Err(e) = spawn_result {
        remove_watcher_if_generation(&watcher_id, generation);
        return Err(AccountInfoError::WatcherError {
            error_details: format!("Failed to spawn watcher thread: {}", e),
        });
    }

    let init_result = init_rx.await.unwrap_or_else(|_| {
        Err(AccountInfoError::WatcherError {
            error_details: "Watcher task exited before reporting init status".to_string(),
        })
    });

    if init_result.is_err() {
        remove_watcher_if_generation(&watcher_id, generation);
    }

    init_result
}

/// Stop a specific watcher by ID.
///
/// Signals shutdown and returns immediately. The watcher thread observes the
/// signal at the top of its poll loop, so teardown completes within one
/// `SOCKET_TIMEOUT_SECS` window (a blocking socket read can't be interrupted
/// mid-call).
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
///
/// Like [`stop_watcher`], each watcher tears down within one
/// `SOCKET_TIMEOUT_SECS` window after the signal.
pub fn stop_all_watchers() {
    if let Ok(mut watchers) = get_watchers().lock() {
        for (_, handle) in watchers.drain() {
            let _ = handle.shutdown_tx.send(true);
        }
    }
}

// ============================================================================
// Watcher internals (runs on a single dedicated thread)
// ============================================================================

/// Socket read timeout for the subscription client (seconds).
///
/// Bounds how long a blocking socket read waits before returning, which in turn
/// bounds detection of a dead connection and shutdown latency (a blocking read
/// can't be interrupted mid-call; see [`stop_watcher`]).
///
/// Note: `ping()` returns as soon as the server answers `server.ping` (≈one
/// round-trip), *not* after this timeout — the timeout only fires when the
/// server sends nothing at all. The idle loop cadence is therefore governed by
/// [`IDLE_POLL_INTERVAL_SECS`], not by this value.
const SOCKET_TIMEOUT_SECS: u8 = 5;

/// How long to wait between polls when a loop iteration finds no activity.
///
/// Each iteration sends `server.ping`, whose round-trip also pulls any pushed
/// `blockchain.scripthash.subscribe` notifications off the socket into the
/// client's queues. Because `ping()` returns immediately on reply, without this
/// wait an idle watcher would spin — hammering the server with pings and
/// risking rate-limiting/disconnect. Trade-off: a notification arriving during
/// the wait is detected on the next ping, so this also bounds idle detection
/// latency.
const IDLE_POLL_INTERVAL_SECS: u64 = 2;

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
        electrum_client::Error::IOError(e) => {
            matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            )
        }
        electrum_client::Error::SharedIOError(e) => {
            matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            )
        }
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
        .collect::<Vec<_>>();
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

/// Subscribe to all scripts in a single batched RPC.
///
/// The failure is surfaced rather than ignored: `electrum-client` registers the
/// local notification queue per script *before* the RPC and doesn't roll it back
/// on error, so a dropped failure would leave `script_pop` returning `Ok(None)`
/// (looks idle) while the server-side subscription is not actually active.
/// Callers must treat an error as "not subscribed" — fail startup (init) or keep
/// retrying the reconnect. (Empty set: no-op.)
fn subscribe_scripts(
    client: &electrum_client::Client,
    scripts: &HashSet<ScriptBuf>,
) -> Result<(), AccountInfoError> {
    if scripts.is_empty() {
        return Ok(());
    }
    client
        .batch_script_subscribe(scripts.iter().map(|s| s.as_script()))
        .map(|_| ())
        .map_err(|e| AccountInfoError::ElectrumError {
            error_details: format!("Failed to subscribe to scripts: {}", e),
        })
}

/// Build a sync blockchain whose stop gap covers the watcher's subscribed range.
///
/// BDK's `ElectrumBlockchain::from(client)` hardcodes a stop gap of 20. A
/// watcher started with a larger `gap_limit` subscribes to addresses past that,
/// so a notification for a far address could fire while wallet sync stopped
/// short of it — emitting a `TransactionsChanged` that omits the detected tx.
/// Building via `from_config` lets sync scan at least as far as we subscribe.
///
/// All other settings match `connect_electrum`'s defaults (`Client::new`).
fn connect_sync_blockchain(
    electrum_url: &str,
    stop_gap: usize,
) -> Result<ElectrumBlockchain, AccountInfoError> {
    let config = ElectrumBlockchainConfig {
        url: electrum_url.to_string(),
        socks5: None,
        retry: 1,
        timeout: None,
        stop_gap,
        validate_domain: true,
    };
    ElectrumBlockchain::from_config(&config).map_err(|e| AccountInfoError::ElectrumError {
        error_details: format!("Failed to connect to Electrum: {}", e),
    })
}

/// Sync the wallet, reusing a persistent `ElectrumBlockchain` across calls to
/// avoid opening a fresh TCP connection on every resync.
///
/// `stop_gap` must cover the watcher's gap limit so sync scans at least as far
/// as the subscribed scripts (see [`connect_sync_blockchain`]).
///
/// On sync failure the (possibly dead) connection is dropped so the next call
/// rebuilds it — making the loop self-healing without a dedicated reconnect for
/// the sync channel.
fn ensure_synced(
    wallet: &Wallet<MemoryDatabase>,
    blockchain: &mut Option<ElectrumBlockchain>,
    electrum_url: &str,
    stop_gap: usize,
) -> Result<(), AccountInfoError> {
    if blockchain.is_none() {
        *blockchain = Some(connect_sync_blockchain(electrum_url, stop_gap)?);
    }
    let bc = blockchain.as_ref().expect("blockchain set above");
    match sync_wallet(wallet, bc) {
        Ok(()) => Ok(()),
        Err(e) => {
            *blockchain = None;
            Err(e)
        }
    }
}

/// Sleep for `duration`, waking early if shutdown is requested.
///
/// Returns `true` if shutdown was observed (the caller should tear down), or
/// `false` if the full duration elapsed. Sleeps in short chunks so shutdown is
/// honored promptly regardless of the interval.
fn sleep_unless_shutdown(shutdown_rx: &watch::Receiver<bool>, duration: Duration) -> bool {
    let mut remaining = duration.as_millis() as u64;
    while remaining > 0 {
        if *shutdown_rx.borrow() {
            return true;
        }
        let chunk = remaining.min(1000);
        std::thread::sleep(Duration::from_millis(chunk));
        remaining = remaining.saturating_sub(chunk);
    }
    *shutdown_rx.borrow()
}

/// Attempt to reconnect the subscription client with exponential backoff.
///
/// Returns the new client and the current tip height once connected, or `None`
/// if shutdown was requested.
fn reconnect_loop(
    electrum_url: &str,
    shutdown_rx: &watch::Receiver<bool>,
    scripts: &HashSet<ScriptBuf>,
) -> Option<(electrum_client::Client, u32)> {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        // Check shutdown before sleeping.
        if *shutdown_rx.borrow() {
            return None;
        }

        // Sleep in 1-second chunks so we can respond to shutdown promptly.
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
                // Require both script and header subscriptions to succeed before
                // declaring the reconnect good — otherwise we'd silently lose
                // push updates (a failed subscribe still leaves local queues, so
                // script_pop looks idle). Retry with backoff until both stick.
                if subscribe_scripts(&client, scripts).is_ok() {
                    if let Ok(header) = client.block_headers_subscribe() {
                        let tip = u32::try_from(header.height).unwrap_or(0);
                        return Some((client, tip));
                    }
                }
                backoff = std::cmp::min(backoff * 2, max_backoff);
            }
            Err(_) => {
                backoff = std::cmp::min(backoff * 2, max_backoff);
            }
        }
    }
}

/// Initialize the watcher and enter the notification loop.
///
/// Reports init success/failure back to the caller via `init_tx`.
/// Runs entirely on a single dedicated thread because `Wallet<MemoryDatabase>`
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
///
/// ## Init ordering (sync → subscribe → sync)
///
/// The wallet is synced once so address usage is known before deriving scripts,
/// then subscribed, then synced again. The second sync brackets the subscribe,
/// so a tx arriving between the first sync and the subscribe baseline is still
/// captured — closing the sync/subscribe race window.
fn watcher_init_and_loop(
    params: WatcherParams,
    setup: super::implementation::WalletSetup,
    shutdown_rx: watch::Receiver<bool>,
    listener: Arc<dyn EventListener>,
    init_tx: oneshot::Sender<Result<(), AccountInfoError>>,
) {
    let gap = params.gap_limit.unwrap_or(20);
    // Wallet sync must scan at least as far as the subscribed scripts (last used
    // + gap). Never drop below BDK's default of 20.
    let sync_stop_gap = (gap as usize).max(20);
    let account_type = setup.account_type;
    let watcher_id = params.watcher_id.clone();

    // --- Initialization ---

    // Subscription client (persistent push channel) with a short read timeout.
    let mut sub_client = match connect_electrum_with_timeout(&params.electrum_url) {
        Ok(c) => c,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let mut tip_height = match sub_client.block_headers_subscribe() {
        Ok(header) => u32::try_from(header.height).unwrap_or(0),
        Err(e) => {
            let _ = init_tx.send(Err(AccountInfoError::ElectrumError {
                error_details: format!("Failed to subscribe to headers: {}", e),
            }));
            return;
        }
    };

    let wallet = match create_wallet(&setup) {
        Ok(w) => w,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    // Persistent blockchain reused for every resync (see `ensure_synced`).
    let mut blockchain: Option<ElectrumBlockchain> = None;

    // First sync: establishes address usage so `derive_scripts` covers the
    // full used range.
    if let Err(e) = ensure_synced(
        &wallet,
        &mut blockchain,
        &params.electrum_url,
        sync_stop_gap,
    ) {
        let _ = init_tx.send(Err(e));
        return;
    }

    let mut subscribed_scripts = match derive_scripts(&wallet, gap) {
        Ok(s) => s,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    // A failed subscribe means no live push updates, so fail startup rather than
    // report success for a watcher that can only refresh on new blocks.
    if let Err(e) = subscribe_scripts(&sub_client, &subscribed_scripts) {
        let _ = init_tx.send(Err(e));
        return;
    }

    // Second sync: brackets the subscribe to close the sync/subscribe race.
    if let Err(e) = ensure_synced(
        &wallet,
        &mut blockchain,
        &params.electrum_url,
        sync_stop_gap,
    ) {
        let _ = init_tx.send(Err(e));
        return;
    }

    // A stop can arrive (handle removed + shutdown signaled) while we were still
    // initializing. If so, don't report success or call the listener for an
    // already-stopped watcher — report the cancellation and tear down.
    if *shutdown_rx.borrow() {
        let _ = init_tx.send(Err(AccountInfoError::WatcherError {
            error_details: "Watcher stopped during startup".to_string(),
        }));
        for script in &subscribed_scripts {
            let _ = sub_client.script_unsubscribe(script);
        }
        return;
    }

    // Init succeeded.
    let _ = init_tx.send(Ok(()));

    // Send initial state.
    listener.on_event(
        watcher_id.clone(),
        build_tx_changed_event(&wallet, tip_height, account_type),
    );

    // Track the last-used indices to detect gap limit extensions.
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
    // Each iteration sends a single `server.ping`. The ping round-trip also
    // pulls any pushed `blockchain.scripthash.subscribe` notifications off the
    // socket into the client's queues, which we then drain. If changes were
    // detected — or a previous resync failed — we resync the wallet and emit an
    // event. When an iteration finds no activity, we wait IDLE_POLL_INTERVAL_SECS
    // before pinging again so an idle watcher doesn't spin on `server.ping`.

    let mut needs_resync = false;
    // Backoff for repeated resync failures while the subscription stays up, so a
    // persistently failing sync doesn't spin or spam `Error` events. Reset on a
    // successful sync; escalates like `reconnect_loop`.
    let mut resync_backoff = Duration::from_secs(1);
    let max_resync_backoff = Duration::from_secs(60);

    loop {
        if *shutdown_rx.borrow() {
            for script in &subscribed_scripts {
                let _ = sub_client.script_unsubscribe(script);
            }
            return;
        }

        // ping() keeps the connection alive and, during its round-trip, reads
        // and queues any pushed notifications already on the socket. It returns
        // as soon as the server replies — it does NOT wait out the socket
        // timeout when idle — so the idle wait at the bottom of the loop, not
        // this call, paces an otherwise-quiet watcher. Outcomes:
        // - Ok(()): normal reply (any notifications are now queued),
        // - Err(timeout): server sent nothing within SOCKET_TIMEOUT_SECS,
        // - Err(other): a real connection failure → reconnect.
        let mut activity = false;
        match sub_client.ping() {
            Ok(()) => {}
            Err(ref e) if is_timeout_error(e) => {
                // Idle timeout. Fall through: the drains below are cheap local
                // queue checks, and falling through lets a previously-failed
                // resync (needs_resync == true) retry even while idle.
            }
            Err(e) => {
                listener.on_event(
                    watcher_id.clone(),
                    WatcherEvent::Disconnected {
                        message: format!("Connection lost: {}", e),
                    },
                );
                match reconnect_loop(&params.electrum_url, &shutdown_rx, &subscribed_scripts) {
                    Some((new_client, new_tip)) => {
                        sub_client = new_client;
                        // Adopt the reconnected server's tip; keeping a higher
                        // stale tip would overstate confirmations.
                        tip_height = new_tip;
                        listener.on_event(watcher_id.clone(), WatcherEvent::Reconnected);
                        // Drop the sync connection so the resync below rebuilds
                        // it, and force a resync to refresh post-reconnect state.
                        blockchain = None;
                        needs_resync = true;
                        activity = true;
                    }
                    None => return, // Shutdown requested
                }
            }
        }

        // Drain block header notifications.
        while let Ok(Some(header)) = sub_client.block_headers_pop_raw() {
            let new_height = u32::try_from(header.height).unwrap_or(tip_height);
            if new_height > tip_height {
                tip_height = new_height;
                needs_resync = true;
                activity = true;
            }
        }

        // Drain script notifications — these arrive via server push from
        // blockchain.scripthash.subscribe, already queued by the ping() above.
        for script in &subscribed_scripts {
            match sub_client.script_pop(script) {
                Ok(Some(_)) => {
                    needs_resync = true;
                    activity = true;
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
                        Some((new_client, new_tip)) => {
                            sub_client = new_client;
                            // Adopt the reconnected server's tip (see above).
                            tip_height = new_tip;
                            listener.on_event(watcher_id.clone(), WatcherEvent::Reconnected);
                            blockchain = None;
                            needs_resync = true;
                            activity = true;
                            break;
                        }
                        None => return,
                    }
                }
            }
        }

        if needs_resync {
            if let Err(e) = ensure_synced(
                &wallet,
                &mut blockchain,
                &params.electrum_url,
                sync_stop_gap,
            ) {
                listener.on_event(
                    watcher_id.clone(),
                    WatcherEvent::Error {
                        message: format!("{}", e),
                    },
                );
                // Leave needs_resync set so the next loop retries; a transient
                // sync failure must not drop an already-popped notification.
                // Back off first so a persistent failure doesn't busy-loop or
                // spam Error events (the `continue` would otherwise skip the
                // idle wait at the bottom of the loop).
                if sleep_unless_shutdown(&shutdown_rx, resync_backoff) {
                    for script in &subscribed_scripts {
                        let _ = sub_client.script_unsubscribe(script);
                    }
                    return;
                }
                resync_backoff = (resync_backoff * 2).min(max_resync_backoff);
                continue;
            }
            needs_resync = false;
            resync_backoff = Duration::from_secs(1);

            // Extend the gap limit if new addresses were used.
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
                        // Only track scripts we actually subscribed; a failed
                        // subscribe falls back to the per-block resync.
                        if sub_client.script_subscribe(&script).is_ok() {
                            subscribed_scripts.insert(script);
                        }
                    }
                }
                last_external = new_external;
            }

            if new_internal > last_internal {
                for i in (last_internal + gap)..(new_internal + gap) {
                    if let Ok(addr) = wallet.get_internal_address(BdkAddressIndex::Peek(i)) {
                        let script = addr.address.script_pubkey();
                        // Only track scripts we actually subscribed (see above).
                        if sub_client.script_subscribe(&script).is_ok() {
                            subscribed_scripts.insert(script);
                        }
                    }
                }
                last_internal = new_internal;
            }

            listener.on_event(
                watcher_id.clone(),
                build_tx_changed_event(&wallet, tip_height, account_type),
            );
        }

        // Idle: nothing was popped and no resync ran this round. Wait before the
        // next ping so a quiet watcher doesn't spin on `server.ping`. A
        // notification arriving during the wait is picked up by the next ping.
        if !activity
            && sleep_unless_shutdown(&shutdown_rx, Duration::from_secs(IDLE_POLL_INTERVAL_SECS))
        {
            for script in &subscribed_scripts {
                let _ = sub_client.script_unsubscribe(script);
            }
            return;
        }
    }
}
