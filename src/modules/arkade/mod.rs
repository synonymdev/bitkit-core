//! Arkade: the Spending wallet's Arkade balance (Ark Labs' `rust-sdk`, ark-client 0.12) and its Lightning address
//! (Ark Labs' lnurl-server). It answers with the method surface of the apps' former TypeScript bridge
//! (`bridge.js`), minus the Lightning swaps, which come with the Arkade Intents client.
//!
//! One wallet runs at a time. Keys come from the Bitkit seed on BIP86 account 1 (`m/86'/0'/1'`), so there is no
//! new phrase and no key shared with Savings (account 0).

mod errors;
mod esplora;
pub mod intents;
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
/// How often open Lightning swaps are driven: funded receives claimed, expired sends refunded.
const DRIVE_EVERY: Duration = Duration::from_secs(10);

type Swaps = intents::IntentsClient<Wallet, store::SqliteSwaps>;

struct Running {
    wallet: Arc<Wallet>,
    /// `None` where no solver serves Lightning swaps (mutinynet).
    swaps: Option<Arc<Swaps>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Running {
    fn stop(self) {
        self.tasks.iter().for_each(JoinHandle::abort);
        self.wallet.stop();
    }
}

static RUNNING: TokioMutex<Option<Running>> = TokioMutex::const_new(None);

pub(crate) fn install_crypto_provider() {
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
    let mut tasks = vec![
        tokio::spawn(chores(wallet.clone())),
        tokio::spawn(listener::watch_history(wallet.clone())),
    ];
    let swaps = match params.network {
        ArkadeNetwork::Bitcoin => {
            let store = Arc::new(store::SqliteSwaps::open(Path::new(&params.data_dir))?);
            let client = Arc::new(Swaps::new(
                wallet.clone(),
                store,
                intents::SolverCard::beta_solver(),
            ));
            client.on_update(Arc::new(|record: &intents::SwapRecord| {
                if let Some(listener) = listener::listener() {
                    listener.on_swap(ArkadeSwap::from(record));
                }
            }));
            tasks.push(client.drive(DRIVE_EVERY));
            Some(client)
        }
        ArkadeNetwork::Mutinynet => None,
    };
    *running = Some(Running {
        wallet,
        swaps,
        tasks,
    });
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

async fn swaps_client() -> Result<Arc<Swaps>, ArkadeError> {
    let running = RUNNING.lock().await;
    let running = running.as_ref().ok_or(ArkadeError::NotStarted)?;
    running
        .swaps
        .clone()
        .ok_or_else(|| ArkadeError::invalid("Lightning swaps are not available on this network"))
}

/// An invoice for exactly `amount_sat`: the payer pays it, the wallet gets it less the solver's fee. The swap is
/// stored before the invoice is returned; its claim happens on the drive loop once the solver funds it.
pub async fn receive(amount_sat: u64) -> Result<ArkadeSwap, ArkadeError> {
    Ok(ArkadeSwap::from(
        &swaps_client().await?.receive(amount_sat).await?,
    ))
}

/// What paying `bolt11` would cost.
pub async fn quote_pay(bolt11: String) -> Result<ArkadePayQuote, ArkadeError> {
    Ok(swaps_client().await?.quote_pay(&bolt11).await?.into())
}

/// Funds the lockup for `bolt11`, refusing a fee above `max_fee_sat`; the outcome arrives as a swap event.
pub async fn pay(bolt11: String, max_fee_sat: Option<u64>) -> Result<ArkadeSwap, ArkadeError> {
    Ok(ArkadeSwap::from(
        &swaps_client().await?.pay(&bolt11, max_fee_sat).await?,
    ))
}

/// Every Lightning swap, newest first.
pub async fn swaps() -> Result<Vec<ArkadeSwap>, ArkadeError> {
    Ok(swaps_client()
        .await?
        .swaps()?
        .iter()
        .map(ArkadeSwap::from)
        .collect())
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
