use crate::modules::boltz::claim::{claim_reverse_swap_guarded, ClaimOutcome};
use crate::modules::boltz::client::build_boltz_client;
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::models::{BoltzDB, SwapRecord};
use crate::modules::boltz::types::{BoltzNetwork, BoltzSwapEvent, BoltzSwapStatus, BoltzSwapType};
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
    ws_task: JoinHandle<()>,
    process_task: JoinHandle<()>,
}

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
/// conservative default). Must be invoked from within a Tokio runtime context
/// (it spawns background tasks).
pub async fn start_swap_updates(
    db: Arc<BoltzDB>,
    network: BoltzNetwork,
    listener: Arc<dyn BoltzEventListener>,
    mnemonic: String,
    bip39_passphrase: Option<String>,
    fee_rate_sat_per_vb: Option<f64>,
) -> Result<(), BoltzError> {
    stop_swap_updates().await;
    let config = AutoClaimConfig {
        mnemonic,
        bip39_passphrase,
        fee_rate_sat_per_vb,
    };

    let boltz_client = build_boltz_client(network);
    let ws = Arc::new(BoltzWsApi::new(
        boltz_client.get_ws_url(),
        BoltzWsConfig::default(),
    ));

    let ws_task = tokio::spawn(ws.clone().run_ws_loop());

    // Subscribe to every non-terminal swap for this network.
    let pending = db.list_pending_swaps().await?;
    for record in pending.iter().filter(|r| r.network == network) {
        if let Err(e) = ws.subscribe_swap(&record.id).await {
            log::warn!("Failed to subscribe to swap {}: {}", record.id, e);
        }
    }

    let process_task = {
        let ws = ws.clone();
        let db = db.clone();
        let listener = listener.clone();
        let config = config.clone();
        tokio::spawn(async move {
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

    let mut guard = updates_cell().lock().await;
    *guard = Some(UpdatesHandle {
        ws,
        network,
        ws_task,
        process_task,
    });
    Ok(())
}

/// Stop the running updates stream, if any, and tear down its tasks.
pub async fn stop_swap_updates() {
    let mut guard = updates_cell().lock().await;
    if let Some(handle) = guard.take() {
        handle.process_task.abort();
        handle.ws_task.abort();
        // Dropping the last `Arc<BoltzWsApi>` triggers its shutdown.
        drop(handle.ws);
    }
}

/// If an updates stream is running for `network`, subscribe it to `swap_id` so
/// newly created swaps are tracked without restarting the stream.
pub async fn subscribe_if_active(network: BoltzNetwork, swap_id: &str) {
    let guard = updates_cell().lock().await;
    if let Some(handle) = guard.as_ref() {
        if handle.network == network {
            if let Err(e) = handle.ws.subscribe_swap(swap_id).await {
                log::warn!("Failed to subscribe to swap {}: {}", swap_id, e);
            }
        }
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

    if should_auto_claim(&record, &raw) {
        auto_claim(db, listener, config, &record).await;
    }
}

/// A reverse swap is auto-claimed once Boltz's lockup *confirms*, provided it
/// hasn't already been claimed.
///
/// Claiming reveals the preimage, which lets Boltz settle the Lightning
/// invoice. Doing that against an unconfirmed (mempool-only) lockup risks the
/// preimage leaking before the lockup confirms — if the lockup were then
/// replaced, the user could be debited on Lightning without receiving onchain
/// funds. We therefore wait for confirmation here; a caller that accepts the
/// 0-conf risk can still claim early via `boltz_claim_reverse_swap`.
fn should_auto_claim(record: &SwapRecord, raw_status: &str) -> bool {
    record.swap_type == BoltzSwapType::Reverse
        && record.claim_tx_id.is_none()
        && raw_status == "transaction.confirmed"
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
