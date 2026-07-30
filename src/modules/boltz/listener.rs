use crate::modules::boltz::claim::{claim_reverse_swap_guarded, ClaimOutcome};
use crate::modules::boltz::client::build_boltz_client;
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::models::{BoltzDB, SwapRecord};
use crate::modules::boltz::types::{BoltzNetwork, BoltzSwapEvent, BoltzSwapStatus, BoltzSwapType};
use crate::modules::boltz::validation::validate_fee_rate;
use boltz_client::swaps::boltz::{BoltzWsApi, BoltzWsConfig, SwapStatus};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex as TokioMutex;
use tokio::task::JoinHandle;

/// Callback interface for receiving Boltz swap lifecycle events.
///
/// Implement this in Swift/Kotlin/Python and register it via
/// `boltz_start_swap_updates` to receive typed notifications as swaps progress.
/// Reverse swaps are claimed automatically; the [`BoltzSwapEvent::Claimed`]
/// event reports the resulting transaction id.
#[uniffi::export(with_foreign)]
pub trait BoltzEventListener: Send + Sync {
    fn on_event(&self, event: BoltzSwapEvent);
}

struct UpdatesHandle {
    ws: Arc<BoltzWsApi>,
    network: BoltzNetwork,
    db: Arc<BoltzDB>,
    listener: Arc<dyn BoltzEventListener>,
    config: AutoClaimConfig,
    ws_task: JoinHandle<()>,
    process_task: JoinHandle<()>,
    reconcile_task: JoinHandle<()>,
}

impl UpdatesHandle {
    /// Abort the stream's background tasks and release its WebSocket. Dropping
    /// a [`JoinHandle`] alone does NOT stop the task (it detaches), so a
    /// superseded stream must be shut down explicitly or its tasks would keep
    /// running, processing events, forever.
    fn shutdown(self) {
        self.reconcile_task.abort();
        self.process_task.abort();
        self.ws_task.abort();
        // Dropping the last `Arc<BoltzWsApi>` triggers its shutdown.
        drop(self.ws);
    }
}

/// Count of live background tasks across all update streams, maintained by the
/// [`TaskAlive`] markers each task future holds. Lets tests (and debugging)
/// verify that superseded or stopped streams' tasks are actually torn down.
static LIVE_STREAM_TASKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// RAII marker moved into every spawned stream task. Created (and counted)
/// before the spawn, dropped when the task's future is dropped, whether it ran
/// to completion or was aborted.
struct TaskAlive;

impl TaskAlive {
    fn new() -> TaskAlive {
        LIVE_STREAM_TASKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        TaskAlive
    }
}

impl Drop for TaskAlive {
    fn drop(&mut self) {
        LIVE_STREAM_TASKS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// How often the running updates stream re-checks every pending swap against
/// Boltz's REST status, so a missed live event self-heals without a restart.
const RECONCILE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Configuration the background stream needs to perform automatic
/// reverse-swap claims. The wallet credentials (used to re-derive swap keys)
/// are held in memory only for the lifetime of the updates stream (dropped on
/// [`stop_swap_updates`]); never persisted.
#[derive(Clone)]
struct AutoClaimConfig {
    mnemonic: String,
    bip39_passphrase: Option<String>,
    /// Fee rate, in sat/vByte, used for the automatic claim transaction. Supplied
    /// by the caller (Bitkit owns fee estimation); falls back to
    /// [`crate::modules::boltz::claim::DEFAULT_FEERATE_SAT_PER_VB`] when `None`.
    fee_rate_sat_per_vb: Option<f64>,
    /// When `true`, reverse swaps are auto-claimed as soon as the lockup enters
    /// the mempool instead of waiting for its confirmation. See
    /// [`should_auto_claim`] for the risk trade-off.
    accept_zero_conf: bool,
}

static SWAP_UPDATES: OnceCell<TokioMutex<Option<UpdatesHandle>>> = OnceCell::new();

fn updates_cell() -> &'static TokioMutex<Option<UpdatesHandle>> {
    SWAP_UPDATES.get_or_init(|| TokioMutex::new(None))
}

/// Open a Boltz WebSocket for `network`, subscribe to every pending swap, and
/// drive their lifecycle until [`stop_swap_updates`] is called.
///
/// Any previously running updates stream is stopped first; only one stream (for
/// one network) runs at a time. `mnemonic` is held in memory for the lifetime of
/// the stream so confirmed reverse swaps can be auto-claimed (their keys are
/// re-derived on demand). `fee_rate_sat_per_vb` is the fee rate used for those
/// automatic claims (Bitkit supplies the current rate; `None` falls back to a
/// conservative default). `accept_zero_conf` claims reverse swaps as soon as
/// the lockup hits the mempool instead of waiting for its confirmation (see
/// [`should_auto_claim`] for the risk trade-off). Must be invoked from within
/// a Tokio runtime context (it spawns background tasks).
pub async fn start_swap_updates(
    db: Arc<BoltzDB>,
    network: BoltzNetwork,
    listener: Arc<dyn BoltzEventListener>,
    mnemonic: String,
    bip39_passphrase: Option<String>,
    fee_rate_sat_per_vb: Option<f64>,
    accept_zero_conf: bool,
) -> Result<(), BoltzError> {
    // An invalid auto-claim fee rate must fail here, before the stream exists,
    // not later inside a claim attempt.
    validate_fee_rate(fee_rate_sat_per_vb)?;

    // Hold the stream slot's lock across the entire replacement. Two concurrent
    // starts would otherwise both tear down the old stream, both spawn tasks,
    // and then overwrite each other's handle; dropping a handle does not stop
    // its tasks, so the loser's stream would keep running detached.
    let mut guard = updates_cell().lock().await;
    if let Some(handle) = guard.take() {
        handle.shutdown();
    }

    // The only fallible step, done before anything is spawned so a failed start
    // cannot leak a task.
    let pending = db.list_pending_swaps().await?;

    let config = AutoClaimConfig {
        mnemonic,
        bip39_passphrase,
        fee_rate_sat_per_vb,
        accept_zero_conf,
    };

    let boltz_client = build_boltz_client(network);
    let ws = Arc::new(BoltzWsApi::new(
        boltz_client.get_ws_url(),
        BoltzWsConfig::default(),
    ));

    let ws_task = {
        let alive = TaskAlive::new();
        let ws = ws.clone();
        tokio::spawn(async move {
            let _alive = alive;
            ws.run_ws_loop().await;
        })
    };

    // Subscribe to every pending swap for this network. Reconciliation is
    // handled by the periodic task below, whose immediate first tick catches up
    // any status the live stream missed while it was down.
    for record in pending.iter().filter(|r| r.network == network) {
        if let Err(e) = ws.subscribe_swap(&record.id).await {
            log::warn!("Failed to subscribe to swap {}: {}", record.id, e);
        }
    }

    // Reconcile every pending swap against Boltz's REST status once per minute
    // for as long as the stream runs, so a live event that was dropped or lagged
    // (the process loop skips `RecvError::Lagged`) self-heals without an app
    // restart. Swaps are reconciled serially to rate-limit the REST calls.
    let reconcile_task = {
        let alive = TaskAlive::new();
        let db = db.clone();
        let listener = listener.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let _alive = alive;
            let mut interval = tokio::time::interval(RECONCILE_INTERVAL);
            loop {
                interval.tick().await;
                let pending = match db.list_pending_swaps().await {
                    Ok(pending) => pending,
                    Err(e) => {
                        log::warn!("Failed to list pending swaps for reconcile: {}", e);
                        continue;
                    }
                };
                for record in pending.iter().filter(|r| r.network == network) {
                    reconcile_swap(&db, &listener, &config, network, &record.id).await;
                }
            }
        })
    };

    let process_task = {
        let alive = TaskAlive::new();
        let ws = ws.clone();
        let db = db.clone();
        let listener = listener.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let _alive = alive;
            let mut updates = ws.updates();
            loop {
                match updates.recv().await {
                    Ok(status) => process_status(&db, listener.as_ref(), &config, status).await,
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        })
    };

    *guard = Some(UpdatesHandle {
        ws,
        network,
        db,
        listener,
        config,
        ws_task,
        process_task,
        reconcile_task,
    });
    Ok(())
}

/// Stop the running updates stream, if any, and tear down its tasks.
pub async fn stop_swap_updates() {
    let mut guard = updates_cell().lock().await;
    if let Some(handle) = guard.take() {
        handle.shutdown();
    }
}

/// If an updates stream is running for `network`, subscribe it to `swap_id` so
/// newly created swaps are tracked without restarting the stream, then reconcile
/// the swap against Boltz's REST status so a status that predates the
/// subscription is not missed.
pub async fn subscribe_if_active(network: BoltzNetwork, swap_id: &str) {
    // Clone the context we need, then release the global lock before any network
    // round-trip so the lock is not held across REST/WebSocket calls.
    let ctx = {
        let guard = updates_cell().lock().await;
        match guard.as_ref() {
            Some(handle) if handle.network == network => Some((
                handle.ws.clone(),
                handle.db.clone(),
                handle.listener.clone(),
                handle.config.clone(),
            )),
            _ => None,
        }
    };
    let Some((ws, db, listener, config)) = ctx else {
        return;
    };
    if let Err(e) = ws.subscribe_swap(swap_id).await {
        log::warn!("Failed to subscribe to swap {}: {}", swap_id, e);
    }
    reconcile_swap(&db, &listener, &config, network, swap_id).await;
}

/// Fetch a swap's current status from Boltz over REST and run it through
/// [`process_status`], catching up any status the live WebSocket did not deliver
/// (for example a `transaction.confirmed` lockup that fired while the stream was
/// down). This makes every (re)subscribe self-healing: a confirmed reverse-swap
/// lockup is auto-claimed even when its live event was never received.
async fn reconcile_swap(
    db: &Arc<BoltzDB>,
    listener: &Arc<dyn BoltzEventListener>,
    config: &AutoClaimConfig,
    network: BoltzNetwork,
    swap_id: &str,
) {
    let boltz_client = build_boltz_client(network);
    match boltz_client.get_swap(swap_id).await {
        Ok(resp) => {
            let status = SwapStatus {
                id: swap_id.to_string(),
                status: resp.status,
                ..Default::default()
            };
            process_status(db, listener.as_ref(), config, status).await;
        }
        Err(e) => log::warn!("Failed to reconcile swap {}: {}", swap_id, e),
    }
}

/// Handle a single status update: persist it, notify the listener, and trigger
/// an automatic claim for reverse swaps whose lockup is now spendable.
async fn process_status(
    db: &Arc<BoltzDB>,
    listener: &dyn BoltzEventListener,
    config: &AutoClaimConfig,
    status: SwapStatus,
) {
    let swap_id = status.id.clone();
    let raw = status.status.clone();

    if let Err(e) = db.update_status(&swap_id, &raw).await {
        log::warn!("Failed to persist status for swap {}: {}", swap_id, e);
    }

    listener.on_event(BoltzSwapEvent::StatusUpdate {
        swap_id: swap_id.clone(),
        status: BoltzSwapStatus::from_raw(&raw),
    });

    let record = match db.get_swap(&swap_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return,
        Err(e) => {
            log::warn!("Failed to load swap {} after update: {}", swap_id, e);
            return;
        }
    };

    if should_auto_claim(&record, &raw, config.accept_zero_conf) {
        auto_claim(db, listener, config, &record).await;
    }
}

/// A reverse swap is auto-claimed once Boltz's lockup *confirms*, provided it
/// hasn't already been claimed. When `accept_zero_conf` is set it is claimed
/// as soon as the lockup enters the mempool.
///
/// Claiming reveals the preimage, which lets Boltz settle the Lightning
/// invoice. Doing that against an unconfirmed (mempool-only) lockup risks the
/// preimage leaking before the lockup confirms: if the lockup were then
/// replaced, the user could be debited on Lightning without receiving onchain
/// funds. Callers opt into that trade-off via `accept_zero_conf`; the
/// confirmed arm always claims so a swap whose mempool event was missed is
/// still claimed on confirmation.
///
/// `invoice.settled` with no locally recorded claim txid also claims: the
/// cooperative flow discloses the preimage before broadcast, so Boltz can
/// settle the invoice while our claim transaction failed to broadcast. The
/// preimage is already public at that point, so retrying the claim carries no
/// added risk and recovers the onchain funds.
fn should_auto_claim(record: &SwapRecord, raw_status: &str, accept_zero_conf: bool) -> bool {
    record.swap_type == BoltzSwapType::Reverse
        && record.claim_tx_id.is_none()
        && (raw_status == "transaction.confirmed"
            || raw_status == "invoice.settled"
            || (accept_zero_conf && raw_status == "transaction.mempool"))
}

/// Claim through the guarded path, which serializes against a concurrent manual
/// claim and persists the txid while still holding the swap's lock. If that
/// manual claim won the race, this call broadcasts nothing and stays quiet:
/// its caller already received the txid, so re-emitting [`BoltzSwapEvent::Claimed`]
/// would double-report the same claim.
async fn auto_claim(
    db: &Arc<BoltzDB>,
    listener: &dyn BoltzEventListener,
    config: &AutoClaimConfig,
    record: &SwapRecord,
) {
    match claim_reverse_swap_guarded(
        db,
        &record.id,
        &config.mnemonic,
        config.bip39_passphrase.as_deref(),
        config.fee_rate_sat_per_vb,
    )
    .await
    {
        Ok(ClaimOutcome::Broadcast(txid)) => listener.on_event(BoltzSwapEvent::Claimed {
            swap_id: record.id.clone(),
            txid,
        }),
        Ok(ClaimOutcome::AlreadyClaimed(txid)) => {
            log::info!("Swap {} was already claimed by tx {}", record.id, txid);
        }
        Err(e) => listener.on_event(BoltzSwapEvent::Error {
            swap_id: record.id.clone(),
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::should_auto_claim;
    use crate::modules::boltz::models::SwapRecord;
    use crate::modules::boltz::types::{BoltzNetwork, BoltzSwapType};

    fn record(swap_type: BoltzSwapType, claim_tx_id: Option<String>) -> SwapRecord {
        SwapRecord {
            id: "swap-id".to_string(),
            swap_type,
            status: "swap.created".to_string(),
            network: BoltzNetwork::Testnet,
            electrum_url: "ssl://electrum.example.com:50002".to_string(),
            swap_index: 0,
            invoice: Some("lnbc1...".to_string()),
            lockup_address: Some("bc1qlockup".to_string()),
            onchain_address: Some("bc1qclaim".to_string()),
            amount_sat: 100_000,
            onchain_amount_sat: Some(99_000),
            timeout_block_height: 800_000,
            create_response_json: "{}".to_string(),
            claim_tx_id,
            refund_tx_id: None,
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn auto_claims_a_confirmed_unclaimed_reverse_swap() {
        let record = record(BoltzSwapType::Reverse, None);
        assert!(should_auto_claim(&record, "transaction.confirmed", false));
        assert!(should_auto_claim(&record, "transaction.confirmed", true));
    }

    #[test]
    fn auto_claims_a_mempool_lockup_only_with_zero_conf() {
        let record = record(BoltzSwapType::Reverse, None);
        assert!(should_auto_claim(&record, "transaction.mempool", true));
        assert!(!should_auto_claim(&record, "transaction.mempool", false));
    }

    #[test]
    fn does_not_auto_claim_before_lockup() {
        let record = record(BoltzSwapType::Reverse, None);
        assert!(!should_auto_claim(&record, "swap.created", false));
        assert!(!should_auto_claim(&record, "swap.created", true));
    }

    #[test]
    fn does_not_auto_claim_an_already_claimed_swap() {
        let record = record(BoltzSwapType::Reverse, Some("claim-txid".to_string()));
        assert!(!should_auto_claim(&record, "transaction.confirmed", false));
        assert!(!should_auto_claim(&record, "transaction.mempool", true));
        assert!(!should_auto_claim(&record, "invoice.settled", false));
    }

    /// The cooperative claim flow discloses the preimage before broadcast, so a
    /// settled invoice with no locally recorded claim txid means the claim may
    /// never have reached the chain; it must be retried.
    #[test]
    fn auto_claims_a_settled_swap_without_a_local_claim_txid() {
        let record = record(BoltzSwapType::Reverse, None);
        assert!(should_auto_claim(&record, "invoice.settled", false));
        assert!(should_auto_claim(&record, "invoice.settled", true));
    }

    #[test]
    fn does_not_auto_claim_a_submarine_swap() {
        let record = record(BoltzSwapType::Submarine, None);
        assert!(!should_auto_claim(&record, "transaction.confirmed", false));
        assert!(!should_auto_claim(&record, "transaction.mempool", true));
        assert!(!should_auto_claim(&record, "invoice.settled", false));
    }

    struct NoopListener;

    impl super::BoltzEventListener for NoopListener {
        fn on_event(&self, _event: crate::modules::boltz::types::BoltzSwapEvent) {}
    }

    /// Wait until the live-task gauge reaches `expected`. Aborted task futures
    /// are dropped asynchronously by the runtime, so the count converges rather
    /// than changing at the abort call itself.
    async fn wait_for_live_tasks(expected: usize) {
        use std::sync::atomic::Ordering;
        for _ in 0..200 {
            if super::LIVE_STREAM_TASKS.load(Ordering::SeqCst) == expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!(
            "expected {} live stream tasks, found {}",
            expected,
            super::LIVE_STREAM_TASKS.load(Ordering::SeqCst)
        );
    }

    /// Concurrent starts must serialize the stream replacement: whichever start
    /// wins, exactly one stream (three tasks) may remain, every superseded
    /// stream's tasks must be aborted, and a stop must tear down the survivor.
    /// This is the only test that touches the global stream slot, so it cannot
    /// race with other tests.
    #[tokio::test]
    async fn concurrent_starts_leave_exactly_one_stream() {
        use crate::modules::boltz::models::BoltzDB;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            BoltzDB::new(dir.path().join("boltz.db").to_str().unwrap())
                .await
                .unwrap(),
        );

        let starts: Vec<_> = (0..4)
            .map(|_| {
                let db = db.clone();
                tokio::spawn(super::start_swap_updates(
                    db,
                    BoltzNetwork::Regtest,
                    Arc::new(NoopListener),
                    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
                    None,
                    None,
                    false,
                ))
            })
            .collect();
        for start in starts {
            start.await.unwrap().unwrap();
        }

        // All but the last-written stream must have been fully aborted.
        wait_for_live_tasks(3).await;
        assert!(super::updates_cell().lock().await.is_some());

        super::stop_swap_updates().await;
        wait_for_live_tasks(0).await;
        assert!(super::updates_cell().lock().await.is_none());
    }
}
