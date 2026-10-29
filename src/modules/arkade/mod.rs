//! Arkade: the Spending wallet's Arkade balance (Ark Labs' `rust-sdk`, ark-client 0.12) and its Lightning address
//! (Ark Labs' lnurl-server). It answers with the method surface of the apps' former TypeScript bridge
//! (`bridge.js`), minus the Lightning swaps, which come with the Arkade Intents client.
//!
//! One wallet runs at a time. Keys come from the Bitkit seed on BIP86 account 1 (`m/86'/0'/1'`), so there is no
//! new phrase and no key shared with Savings (account 0).

mod errors;
mod esplora;
pub mod keys;
mod listener;
pub mod lnurl;
mod store;
#[cfg(test)]
mod tests;
mod types;
mod wallet;

pub use errors::ArkadeError;
pub use listener::{set_listener, ArkadeEventListener};
pub use lnurl::ArkadeLightningAddress;
pub use types::*;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tokio::task::JoinHandle;
use wallet::Wallet;

/// How often the chores board confirmed deposits and renew outputs near expiry.
const CHORES_EVERY: Duration = Duration::from_secs(60);

struct Running {
    wallet: Arc<Wallet>,
    tasks: Vec<JoinHandle<()>>,
}

impl Running {
    fn stop(self) {
        self.tasks.iter().for_each(JoinHandle::abort);
        self.wallet.stop();
    }
}

static RUNNING: TokioMutex<Option<Running>> = TokioMutex::const_new(None);

fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Starts the wallet, or returns its info when it already runs. Must run on a multi-threaded tokio runtime:
/// it spawns the chores and the history watch.
pub async fn start(params: ArkadeStartParams) -> Result<ArkadeInfo, ArkadeError> {
    install_crypto_provider();
    let mut running = RUNNING.lock().await;
    if let Some(r) = running.as_ref() {
        return r.wallet.info().await;
    }
    let wallet = Wallet::start(&params).await?;
    let info = wallet.info().await?;
    let tasks = vec![
        tokio::spawn(chores(wallet.clone())),
        tokio::spawn(listener::watch_history(wallet.clone())),
    ];
    *running = Some(Running { wallet, tasks });
    Ok(info)
}

/// Stops the wallet and its background tasks; its database stays.
pub async fn stop() {
    if let Some(r) = RUNNING.lock().await.take() {
        r.stop();
    }
}

async fn wallet() -> Result<Arc<Wallet>, ArkadeError> {
    RUNNING
        .lock()
        .await
        .as_ref()
        .map(|r| r.wallet.clone())
        .ok_or(ArkadeError::NotStarted)
}

pub async fn info() -> Result<ArkadeInfo, ArkadeError> {
    wallet().await?.info().await
}

pub async fn balance() -> Result<ArkadeBalance, ArkadeError> {
    wallet().await?.balance().await
}

pub async fn vtxos() -> Result<Vec<ArkadeVtxo>, ArkadeError> {
    wallet().await?.vtxos().await
}

pub async fn history() -> Result<Vec<ArkadeHistoryEntry>, ArkadeError> {
    wallet().await?.history().await
}

pub async fn settle() -> Result<Option<String>, ArkadeError> {
    wallet().await?.settle().await
}

pub async fn renew() -> Result<Option<String>, ArkadeError> {
    wallet().await?.renew().await
}

pub async fn offboard_fee() -> Result<ArkadeFees, ArkadeError> {
    wallet().await?.fees().await
}

pub async fn offboard(address: String, amount_sat: Option<u64>) -> Result<String, ArkadeError> {
    wallet().await?.offboard(&address, amount_sat).await
}

pub async fn send_ark(address: String, amount_sat: u64) -> Result<String, ArkadeError> {
    wallet().await?.send_ark(&address, amount_sat).await
}

pub async fn delegate_info() -> Result<bool, ArkadeError> {
    Ok(wallet().await?.delegate)
}

pub async fn lightning_address() -> Result<ArkadeLightningAddress, ArkadeError> {
    wallet().await?.lightning_address().await
}

pub fn export_state(data_dir: &str) -> Result<String, ArkadeError> {
    store::export(Path::new(data_dir))
}

pub fn import_state(data_dir: &str, json: &str) -> Result<u64, ArkadeError> {
    store::import(Path::new(data_dir), json)
}

/// Stops the wallet and deletes its database.
pub async fn wipe(data_dir: &str) -> Result<(), ArkadeError> {
    stop().await;
    store::wipe(Path::new(data_dir))
}

async fn chores(wallet: Arc<Wallet>) {
    let mut every = tokio::time::interval(CHORES_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        wallet.chores().await;
    }
}
