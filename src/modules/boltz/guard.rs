use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex as TokioMutex, OwnedMutexGuard};

/// Per-swap locks that serialize claim and refund attempts.
///
/// Claiming reads the swap, broadcasts, then records the txid. Those steps span
/// `.await` points (Boltz and Electrum round trips), so two callers can
/// interleave: the automatic claim driven by the updates stream and a manual
/// `boltz_claim_reverse_swap` recovery call can both observe no recorded txid
/// and both broadcast. Holding the swap's lock across the whole
/// read-broadcast-record sequence makes it atomic, so the second caller sees the
/// first caller's txid and returns it instead of broadcasting again.
///
/// The map only ever holds one small entry per swap that has had a claim or
/// refund attempted, so it is left to grow rather than reference-counted down;
/// pruning would race with a caller that is waiting to acquire the same lock.
static SWAP_LOCKS: Lazy<StdMutex<HashMap<String, Arc<TokioMutex<()>>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

/// Acquire the lock for `swap_id`, waiting for any in-flight claim or refund on
/// the same swap to finish. The returned guard releases the lock when dropped.
pub(crate) async fn lock_swap(swap_id: &str) -> OwnedMutexGuard<()> {
    let lock = {
        let mut locks = SWAP_LOCKS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.entry(swap_id.to_string()).or_default().clone()
    };
    lock.lock_owned().await
}
