use super::types::{ArkadeBalance, ArkadeHistoryEntry, ArkadeSwap};
use super::wallet::Wallet;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often history is read for the listener, as the TypeScript bridge polls it.
const HISTORY_EVERY: Duration = Duration::from_secs(15);

/// Callback interface for Arkade wallet events.
///
/// Implement this in Swift/Kotlin/Python and register it via `arkade_set_listener` to hear of history entries
/// that are new, or newly settled, since the last pass: offline Lightning receives the LNURL server claimed,
/// payments to the Arkade address, boarded deposits and settled offboards; and of each Lightning swap whose
/// outcome changed.
#[uniffi::export(with_foreign)]
pub trait ArkadeEventListener: Send + Sync {
    fn on_history(&self, entries: Vec<ArkadeHistoryEntry>, balance: ArkadeBalance);

    fn on_swap(&self, swap: ArkadeSwap);
}

pub(crate) static LISTENER: Mutex<Option<Arc<dyn ArkadeEventListener>>> = Mutex::new(None);

pub fn set_listener(listener: Option<Arc<dyn ArkadeEventListener>>) {
    if let Ok(mut l) = LISTENER.lock() {
        *l = listener;
    }
}

pub(crate) fn listener() -> Option<Arc<dyn ArkadeEventListener>> {
    LISTENER.lock().ok().and_then(|l| l.clone())
}

/// The bridge's `pollHistory`: entries new or newly settled since the last pass go to the listener.
pub(crate) async fn watch_history(wallet: Arc<Wallet>) {
    let mut seen: Option<HashSet<String>> = None;
    let mut every = tokio::time::interval(HISTORY_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        let entries = match wallet.history().await {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("Arkade history poll failed: {e}");
                continue;
            }
        };
        let keys: HashSet<String> = entries
            .iter()
            .map(|e| format!("{}:{}", e.key, e.settled))
            .collect();
        if let Some(seen) = seen.as_ref() {
            let fresh: Vec<_> = entries
                .into_iter()
                .filter(|e| !seen.contains(&format!("{}:{}", e.key, e.settled)))
                .collect();
            if let (false, Some(listener)) = (fresh.is_empty(), listener()) {
                match wallet.balance().await {
                    Ok(balance) => listener.on_history(fresh, balance),
                    Err(e) => {
                        // Keep the entries unseen, so the next pass reports them.
                        log::warn!("Arkade balance for history event failed: {e}");
                        continue;
                    }
                }
            }
        }
        seen = Some(keys);
    }
}
