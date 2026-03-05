uniffi::setup_scaffolding!();

// Initialize Android logger so Rust log::info! calls appear in logcat
#[cfg(target_os = "android")]
fn init_android_logger() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Debug)
                .with_tag("BitkitRust"),
        );
        log::info!("[BitkitRust] Android logger initialized");
    });
}

mod modules;

use std::sync::Arc;
use once_cell::sync::OnceCell;

// Re-export Trezor callback types and traits so UniFFI discovers them at the crate root
pub use crate::modules::trezor::{
    TrezorTransportReadResult, TrezorTransportWriteResult, TrezorCallMessageResult,
    NativeDeviceInfo, TrezorTransportCallback,
    trezor_set_transport_callback, get_transport_callback,
    trezor_is_ble_available,
    TrezorUiCallback, trezor_set_ui_callback,
};
pub use modules::scanner::{
    Scanner,
    DecodingError
};
pub use modules::lnurl;
pub use modules::onchain;
pub use modules::activity;
use crate::modules::pubky::PubkyError;
use crate::activity::{ActivityError, ActivityDB, OnchainActivity, LightningActivity, Activity, ActivityFilter, SortDirection, PaymentType, DbError, ClosedChannelDetails, ActivityTags, PreActivityMetadata, TransactionDetails, TxInput, TxOutput};
use crate::modules::blocktank::{BlocktankDB, BlocktankError, IBtInfo, IBtOrder, CreateOrderOptions, BtOrderState2, IBt0ConfMinTxFeeWindow, IBtEstimateFeeResponse, IBtEstimateFeeResponse2, CreateCjitOptions, ICJitEntry, CJitStateEnum, IBtBolt11Invoice, IGift, ChannelLiquidityOptions, ChannelLiquidityParams, DefaultLspBalanceParams};
use crate::onchain::{AddressError, ValidationResult, GetAddressResponse, Network, GetAddressesResponse, SweepError, SweepResult, SweepTransactionPreview, SweepableBalances};
use crate::modules::trezor::{TrezorError, TrezorDeviceInfo, TrezorTransportType, TrezorFeatures, TrezorGetAddressParams, TrezorAddressResponse, TrezorGetPublicKeyParams, TrezorPublicKeyResponse, TrezorScriptType, TrezorManager, TrezorSignMessageParams, TrezorSignedMessageResponse, TrezorVerifyMessageParams, TrezorSignTxParams, TrezorSignedTx, TrezorTxInput, TrezorTxOutput, TrezorCoinType, AddressInfo, AccountAddresses};
use crate::modules::trezor::{AccountInfoError, AccountInfoResult, SingleAddressInfoResult, AccountType, AccountUtxo, ComposeAccount, get_account_info, get_address_info, account_type_to_script_type, fetch_prev_txs, broadcast_raw_tx, TrezorFeeLevel, TrezorSortingStrategy, TrezorPrecomposeOutput, TrezorPrecomposeParams, TrezorPrecomposedInput, TrezorPrecomposedOutput, TrezorPrecomposedResult, precompose_transaction, precomposed_to_sign_params, TrezorPrevTx};
pub use crate::onchain::WordCount;

use std::sync::Mutex as StdMutex;
use tokio::runtime::Runtime;
use tokio::sync::Mutex as TokioMutex;
use bip39::Mnemonic;
use bitcoin::bip32::Xpriv;
use bitcoin::Network as BitcoinNetwork;
use std::str::FromStr;

pub struct DatabaseConnections {
    activity_db: Option<ActivityDB>,
}

pub struct AsyncDatabaseConnections {
    blocktank_db: Option<BlocktankDB>,
}
// Two separate global states for sync and async connections
static DB: OnceCell<StdMutex<DatabaseConnections>> = OnceCell::new();
static ASYNC_DB: OnceCell<TokioMutex<AsyncDatabaseConnections>> = OnceCell::new();
static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static TREZOR_MANAGER: OnceCell<TrezorManager> = OnceCell::new();

fn ensure_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Failed to create Tokio runtime")
    })
}

/// Helper function to get a reference to the activity database connections
fn get_activity_db() -> Result<std::sync::MutexGuard<'static, DatabaseConnections>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    Ok(cell.lock().unwrap())
}

#[uniffi::export]
pub async fn decode(invoice: String) -> Result<Scanner, DecodingError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        Scanner::decode(invoice).await
    }).await.unwrap()
}

#[uniffi::export]
pub async fn get_lnurl_invoice(address: String, amount_satoshis: u64) -> Result<String, lnurl::LnurlError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        lnurl::get_lnurl_invoice(&address, amount_satoshis).await
    }).await.unwrap()
}

#[uniffi::export]
pub fn create_channel_request_url(
    k1: String,
    callback: String,
    local_node_id: String,
    is_private: bool,
    cancel: bool,
) -> Result<String, lnurl::LnurlError> {
    let params = lnurl::ChannelRequestParams {
        k1,
        callback,
        local_node_id,
        is_private,
        cancel,
    };
    lnurl::create_channel_request_url(params)
}

#[uniffi::export]
pub fn create_withdraw_callback_url(
    k1: String,
    callback: String,
    payment_request: String,
) -> Result<String, lnurl::LnurlError> {
    let params = lnurl::WithdrawCallbackParams {
        k1,
        callback,
        payment_request,
    };
    lnurl::create_withdraw_callback_url(params)
}

#[uniffi::export]
pub async fn lnurl_auth(
    domain: String,
    k1: String,
    callback: String,
    bip32_mnemonic: String,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
) -> Result<String, lnurl::LnurlError> {
    let mnemonic = Mnemonic::parse(&bip32_mnemonic)
        .map_err(|_| lnurl::LnurlError::AuthenticationFailed)?;
    
    let bitcoin_network = match network.unwrap_or(Network::Bitcoin) {
        Network::Bitcoin => BitcoinNetwork::Bitcoin,
        Network::Testnet => BitcoinNetwork::Testnet,
        Network::Testnet4 => BitcoinNetwork::Testnet,
        Network::Signet => BitcoinNetwork::Signet,
        Network::Regtest => BitcoinNetwork::Regtest,
    };
    
    let seed = mnemonic.to_seed(bip39_passphrase.as_deref().unwrap_or(""));
    let root = Xpriv::new_master(bitcoin_network, &seed)
        .map_err(|_| lnurl::LnurlError::AuthenticationFailed)?;
    
    // Derive hashing key using m/138'/0 path (as per LUD-05)
    let hashing_path = bitcoin::bip32::DerivationPath::from_str("m/138'/0")
        .map_err(|_| lnurl::LnurlError::AuthenticationFailed)?;
    
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let hashing_key_xpriv = root.derive_priv(&secp, &hashing_path)
        .map_err(|_| lnurl::LnurlError::AuthenticationFailed)?;
    
    let hashing_key_bytes = hashing_key_xpriv.private_key.secret_bytes();
    
    let params = lnurl::LnurlAuthParams {
        domain,
        k1,
        callback,
        hashing_key: hashing_key_bytes,
    };
    
    let rt = ensure_runtime();
    rt.spawn(async move {
        lnurl::lnurl_auth(params).await
    }).await.unwrap()
}

#[uniffi::export]
pub fn validate_bitcoin_address(address: String) -> Result<ValidationResult, AddressError> {
    onchain::BitcoinAddressValidator::validate_address(&address)
}

#[uniffi::export]
pub fn generate_mnemonic(word_count: Option<WordCount>) -> Result<String, AddressError> {
    let external_word_count = word_count.map(|wc| wc.into());
    onchain::BitcoinAddressValidator::genenerate_mnemonic(external_word_count)
}

#[uniffi::export]
pub fn derive_bitcoin_address(
    mnemonic_phrase: String,
    derivation_path_str: Option<String>,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
) -> Result<GetAddressResponse, AddressError> {
    onchain::BitcoinAddressValidator::derive_bitcoin_address(
        &mnemonic_phrase,
        derivation_path_str.as_deref(),
        network.map(|n| n.into()),
        bip39_passphrase.as_deref(),
    )
}

#[uniffi::export]
pub fn derive_bitcoin_addresses(
    mnemonic_phrase: String,
    derivation_path_str: Option<String>,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
    is_change: Option<bool>,
    start_index: Option<u32>,
    count: Option<u32>,
) -> Result<GetAddressesResponse, AddressError> {
    onchain::BitcoinAddressValidator::derive_bitcoin_addresses(
        &mnemonic_phrase,
        derivation_path_str.as_deref(),
        network.map(|n| n.into()),
        bip39_passphrase.as_deref(),
        is_change,
        start_index,
        count,
    )
}

#[uniffi::export]
pub fn derive_private_key(
    mnemonic_phrase: String,
    derivation_path_str: Option<String>,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
) -> Result<String, AddressError> {
    onchain::BitcoinAddressValidator::derive_private_key(
        &mnemonic_phrase,
        derivation_path_str.as_deref(),
        network.map(|n| n.into()),
        bip39_passphrase.as_deref(),
    )
}

#[uniffi::export]
pub fn validate_mnemonic(mnemonic_phrase: String) -> Result<(), AddressError> {
    onchain::BitcoinAddressValidator::validate_mnemonic(&mnemonic_phrase)
}

#[uniffi::export]
pub fn is_valid_bip39_word(word: String) -> bool {
    onchain::BitcoinAddressValidator::is_valid_bip39_word(&word)
}

#[uniffi::export]
pub fn get_bip39_suggestions(partial_word: String, limit: u32) -> Vec<String> {
    onchain::BitcoinAddressValidator::get_bip39_suggestions(&partial_word, limit as usize)
}

#[uniffi::export]
pub fn get_bip39_wordlist() -> Vec<String> {
    onchain::BitcoinAddressValidator::get_bip39_wordlist()
}

#[uniffi::export]
pub fn mnemonic_to_entropy(mnemonic_phrase: String) -> Result<Vec<u8>, AddressError> {
    onchain::BitcoinAddressValidator::mnemonic_to_entropy(&mnemonic_phrase)
}

#[uniffi::export]
pub fn entropy_to_mnemonic(entropy: Vec<u8>) -> Result<String, AddressError> {
    onchain::BitcoinAddressValidator::entropy_to_mnemonic(&entropy)
}

#[uniffi::export]
pub fn mnemonic_to_seed(mnemonic_phrase: String, passphrase: Option<String>) -> Result<Vec<u8>, AddressError> {
    onchain::BitcoinAddressValidator::mnemonic_to_seed(&mnemonic_phrase, passphrase.as_deref())
}

#[uniffi::export]
pub async fn check_sweepable_balances(
    mnemonic_phrase: String,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
    electrum_url: String,
) -> Result<SweepableBalances, SweepError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        onchain::BitcoinAddressValidator::check_sweepable_balances(
            &mnemonic_phrase,
            network.unwrap_or(Network::Bitcoin).into(),
            bip39_passphrase.as_deref(),
            &electrum_url,
        )
        .await
    })
    .await
    .unwrap()
}

#[uniffi::export]
pub async fn prepare_sweep_transaction(
    mnemonic_phrase: String,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
    electrum_url: String,
    destination_address: String,
    fee_rate_sats_per_vbyte: Option<u32>,
) -> Result<SweepTransactionPreview, SweepError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        onchain::BitcoinAddressValidator::prepare_sweep_transaction(
            &mnemonic_phrase,
            network.unwrap_or(Network::Bitcoin).into(),
            bip39_passphrase.as_deref(),
            &electrum_url,
            &destination_address,
            fee_rate_sats_per_vbyte,
        )
        .await
    })
    .await
    .unwrap()
}

#[uniffi::export]
pub async fn broadcast_sweep_transaction(
    psbt: String,
    mnemonic_phrase: String,
    network: Option<Network>,
    bip39_passphrase: Option<String>,
    electrum_url: String,
) -> Result<SweepResult, SweepError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        onchain::BitcoinAddressValidator::broadcast_sweep_transaction(
            &psbt,
            &mnemonic_phrase,
            network.unwrap_or(Network::Bitcoin).into(),
            bip39_passphrase.as_deref(),
            &electrum_url,
        )
        .await
    })
    .await
    .unwrap()
}

#[uniffi::export]
pub fn init_db(base_path: String) -> Result<String, DbError> {
    // Initialize sync database state
    DB.get_or_init(|| {
        StdMutex::new(DatabaseConnections {
            activity_db: None,
        })
    });

    // Initialize async database state
    ASYNC_DB.get_or_init(|| {
        TokioMutex::new(AsyncDatabaseConnections {
            blocktank_db: None,
        })
    });

    // Create runtime for async operations
    let rt = ensure_runtime();
    // Create database connections
    let activity_db = ActivityDB::new(&format!("{}/activity.db", base_path))?;
    let blocktank_db = rt.block_on(async {
        BlocktankDB::new(&format!("{}/blocktank.db", base_path), None).await
    })?;

    // Initialize sync database
    {
        let mut guard = DB.get().unwrap().lock().unwrap();
        guard.activity_db = Some(activity_db);
    }

    // Initialize async database
    {
        let async_db = ASYNC_DB.get().unwrap();
        rt.block_on(async {
            let mut guard = async_db.lock().await;
            guard.blocktank_db = Some(blocktank_db);
        });
    }

    Ok("Databases initialized successfully".to_string())
}

#[uniffi::export]
pub fn get_activities(
    filter: Option<ActivityFilter>,
    tx_type: Option<PaymentType>,
    tags: Option<Vec<String>>,
    search: Option<String>,
    min_date: Option<u64>,
    max_date: Option<u64>,
    limit: Option<u32>,
    sort_direction: Option<SortDirection>
) -> Result<Vec<Activity>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities(filter, tx_type, tags, search, min_date, max_date, limit, sort_direction)
}

#[uniffi::export]
pub fn upsert_activity(activity: Activity) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_activity(&activity)
}

#[uniffi::export]
pub fn insert_activity(activity: Activity) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    match activity {
        Activity::Onchain(onchain) => db.insert_onchain_activity(&onchain),
        Activity::Lightning(lightning) => db.insert_lightning_activity(&lightning),
    }
}

#[uniffi::export]
pub fn update_activity(activity_id: String, activity: Activity) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    match activity {
        Activity::Onchain(onchain) => db.update_onchain_activity_by_id(&activity_id, &onchain),
        Activity::Lightning(lightning) => db.update_lightning_activity_by_id(&activity_id, &lightning),
    }
}

#[uniffi::export]
pub fn get_activity_by_id(activity_id: String) -> Result<Option<Activity>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activity_by_id(&activity_id)
}

#[uniffi::export]
pub fn get_activity_by_tx_id(tx_id: String) -> Result<Option<OnchainActivity>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activity_by_tx_id(&tx_id)
}

#[uniffi::export]
pub fn delete_activity_by_id(activity_id: String) -> Result<bool, ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.delete_activity_by_id(&activity_id)
}

#[uniffi::export]
pub fn add_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.add_tags(&activity_id, &tags)
}

#[uniffi::export]
pub fn remove_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.remove_tags(&activity_id, &tags)
}

#[uniffi::export]
pub fn get_tags(activity_id: String) -> Result<Vec<String>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_tags(&activity_id)
}

#[uniffi::export]
pub fn get_activities_by_tag(tag: String, limit: Option<u32>, sort_direction: Option<SortDirection>) -> Result<Vec<Activity>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities_by_tag(&tag, limit, sort_direction)
}

#[uniffi::export]
pub fn get_all_unique_tags() -> Result<Vec<String>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_unique_tags()
}

#[uniffi::export]
pub fn get_all_activities_tags() -> Result<Vec<ActivityTags>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_activities_tags()
}

#[uniffi::export]
pub fn upsert_tags(activity_tags: Vec<ActivityTags>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_tags(&activity_tags)
}

#[uniffi::export]
pub fn add_pre_activity_metadata(pre_activity_metadata: PreActivityMetadata) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.add_pre_activity_metadata(&pre_activity_metadata)
}

#[uniffi::export]
pub fn add_pre_activity_metadata_tags(payment_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.add_pre_activity_metadata_tags(&payment_id, &tags)
}

#[uniffi::export]
pub fn remove_pre_activity_metadata_tags(payment_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.remove_pre_activity_metadata_tags(&payment_id, &tags)
}

#[uniffi::export]
pub fn reset_pre_activity_metadata_tags(payment_id: String) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.reset_pre_activity_metadata_tags(&payment_id)
}

#[uniffi::export]
pub fn delete_pre_activity_metadata(payment_id: String) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.delete_pre_activity_metadata(&payment_id)
}

#[uniffi::export]
pub fn upsert_pre_activity_metadata(pre_activity_metadata: Vec<PreActivityMetadata>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_pre_activity_metadata(&pre_activity_metadata)
}

#[uniffi::export]
pub fn get_pre_activity_metadata(search_key: String, search_by_address: bool) -> Result<Option<PreActivityMetadata>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_pre_activity_metadata(&search_key, search_by_address)
}

#[uniffi::export]
pub fn get_all_pre_activity_metadata() -> Result<Vec<PreActivityMetadata>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_pre_activity_metadata()
}

#[uniffi::export]
pub fn upsert_closed_channel(channel: ClosedChannelDetails) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
        db.upsert_closed_channel(&channel)
}

#[uniffi::export]
pub fn upsert_closed_channels(channels: Vec<ClosedChannelDetails>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_closed_channels(&channels)
}

#[uniffi::export]
pub fn upsert_onchain_activities(activities: Vec<OnchainActivity>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_onchain_activities(&activities)
}

#[uniffi::export]
pub fn upsert_lightning_activities(activities: Vec<LightningActivity>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_lightning_activities(&activities)
}

#[uniffi::export]
pub fn upsert_activities(activities: Vec<Activity>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;

    let mut onchain_list: Vec<OnchainActivity> = Vec::new();
    let mut lightning_list: Vec<LightningActivity> = Vec::new();

    for activity in activities {
        match activity {
            Activity::Onchain(a) => onchain_list.push(a),
            Activity::Lightning(a) => lightning_list.push(a),
        }
    }

    if !onchain_list.is_empty() {
        db.upsert_onchain_activities(&onchain_list)?;
    }
    if !lightning_list.is_empty() {
        db.upsert_lightning_activities(&lightning_list)?;
    }

    Ok(())
}

#[uniffi::export]
pub fn get_closed_channel_by_id(channel_id: String) -> Result<Option<ClosedChannelDetails>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_closed_channel_by_id(&channel_id)
}

#[uniffi::export]
pub fn get_all_closed_channels(sort_direction: Option<SortDirection>) -> Result<Vec<ClosedChannelDetails>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_closed_channels(sort_direction)
}

#[uniffi::export]
pub fn remove_closed_channel_by_id(channel_id: String) -> Result<bool, ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.remove_closed_channel_by_id(&channel_id)
}

#[uniffi::export]
pub fn wipe_all_closed_channels() -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.wipe_all_closed_channels()
}

#[uniffi::export]
pub async fn update_blocktank_url(new_url: String) -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    // Use spawn_blocking instead of block_on to avoid deadlocks
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let mut guard = cell.lock().await;
        let db = guard.blocktank_db.as_mut().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        db.update_blocktank_url(&new_url).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn get_info(refresh: Option<bool>) -> Result<Option<IBtInfo>, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        if refresh.unwrap_or(false) {
            Ok(Some(db.fetch_and_store_info().await?.into()))
        } else {
            let info = db.get_info().await?;
            Ok(info.map(|info| info.into()))
        }
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn create_order(
    lsp_balance_sat: u64,
    channel_expiry_weeks: u32,
    options: Option<CreateOrderOptions>,
) -> Result<IBtOrder, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        // Convert the options to the external type using .into()
        let external_options = options.map(|opt| opt.into());

        // Convert the result to our local IBtOrder type
        db.create_and_store_order(lsp_balance_sat, channel_expiry_weeks, external_options).await.map(|order| order.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn open_channel(
    order_id: String,
    connection_string: String,
) -> Result<IBtOrder, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.open_channel(order_id, connection_string).await.map(|order| order.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn get_orders(
    order_ids: Option<Vec<String>>,
    filter: Option<BtOrderState2>,
    refresh: bool,
) -> Result<Vec<IBtOrder>, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        // If refresh is true and we have order_ids, refresh those specific orders
        if refresh && order_ids.is_some() {
            let ids = order_ids.unwrap();
            db.refresh_orders(&ids).await.map(|orders| {
                orders.into_iter().map(|order| order.into()).collect()
            })
        } else {
            // Otherwise get orders from the database
            db.get_orders(order_ids.as_deref(), filter.map(|f| f.into())).await.map(|orders| {
                orders.into_iter().map(|order| order.into()).collect()
            })
        }
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

/// Refresh all active orders in the database with latest data from the LSP
#[uniffi::export]
pub async fn refresh_active_orders() -> Result<Vec<IBtOrder>, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        db.refresh_active_orders().await.map(|orders| {
            orders.into_iter().map(|order| order.into()).collect()
        })
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn get_min_zero_conf_tx_fee(
    order_id: String,
) -> Result<IBt0ConfMinTxFeeWindow, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.get_min_zero_conf_tx_fee(order_id).await.map(|fee| fee.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn estimate_order_fee(
    lsp_balance_sat: u64,
    channel_expiry_weeks: u32,
    options: Option<CreateOrderOptions>,
) -> Result<IBtEstimateFeeResponse, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        let external_options = options.map(|opt| opt.into());

        db.estimate_order_fee(lsp_balance_sat, channel_expiry_weeks, external_options).await.map(|response| response.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn estimate_order_fee_full(
    lsp_balance_sat: u64,
    channel_expiry_weeks: u32,
    options: Option<CreateOrderOptions>,
) -> Result<IBtEstimateFeeResponse2, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        let external_options = options.map(|opt| opt.into());

        db.estimate_order_fee_full(lsp_balance_sat, channel_expiry_weeks, external_options).await.map(|response| response.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn create_cjit_entry(
    channel_size_sat: u64,
    invoice_sat: u64,
    invoice_description: String,
    node_id: String,
    channel_expiry_weeks: u32,
    options: Option<CreateCjitOptions>,
) -> Result<ICJitEntry, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        let external_options = options.map(|opt| opt.into());

        db.create_cjit_entry(
            channel_size_sat,
            invoice_sat,
            &invoice_description,
            &node_id,
            channel_expiry_weeks,
            external_options
        ).await.map(|entry| entry.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn get_cjit_entries(
    entry_ids: Option<Vec<String>>,
    filter: Option<CJitStateEnum>,
    refresh: bool,
) -> Result<Vec<ICJitEntry>, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        // If refresh is true and we have entry_ids, refresh those specific entries
        if refresh && entry_ids.is_some() {
            let entries = entry_ids.unwrap();
            // Since we don't have a bulk refresh method for CJIT entries,
            // we'll refresh them one by one
            let mut results = Vec::new();
            for entry_id in entries {
                if let Ok(entry) = db.refresh_cjit_entry(&entry_id).await {
                    results.push(entry);
                }
            }
            Ok(results.into_iter().map(|entry| entry.into()).collect())
        } else {
            // Otherwise get entries from the database
            db.get_cjit_entries(entry_ids.as_deref(), filter.map(|f| f.into())).await.map(|entries| {
                entries.into_iter().map(|entry| entry.into()).collect()
            })
        }
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

/// Refresh all active CJIT entries in the database with latest data from the LSP
#[uniffi::export]
pub async fn refresh_active_cjit_entries() -> Result<Vec<ICJitEntry>, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        db.refresh_active_cjit_entries().await.map(|entries| {
            entries.into_iter().map(|entry| entry.into()).collect()
        })
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn register_device(
    device_token: String,
    public_key: String,
    features: Vec<String>,
    node_id: String,
    iso_timestamp: String,
    signature: String,
    is_production: Option<bool>,
    custom_url: Option<String>
) -> Result<String, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.register_device(
            &device_token,
            &public_key,
            &features,
            &node_id,
            &iso_timestamp,
            &signature,
            is_production,
            custom_url.as_deref()
        ).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn test_notification(
    device_token: String,
    secret_message: String,
    notification_type: Option<String>,
    custom_url: Option<String>
) -> Result<String, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.test_notification(
            &device_token,
            &secret_message,
            notification_type.as_deref(),
            custom_url.as_deref()
        ).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn gift_pay(invoice: String) -> Result<IGift, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.gift_pay(&invoice).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn gift_order(client_node_id: String, code: String) -> Result<IGift, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.gift_order(&client_node_id, &code).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn get_gift(gift_id: String) -> Result<IGift, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.get_gift(&gift_id).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn get_payment(payment_id: String) -> Result<IBtBolt11Invoice, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.get_payment(&payment_id).await.map(|payment| payment.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn regtest_mine(count: Option<u32>) -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.regtest_mine(count).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn regtest_deposit(
    address: String,
    amount_sat: Option<u64>,
) -> Result<String, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.regtest_deposit(&address, amount_sat).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn regtest_pay(
    invoice: String,
    amount_sat: Option<u64>,
) -> Result<String, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.regtest_pay(&invoice, amount_sat).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn regtest_get_payment(payment_id: String) -> Result<IBtBolt11Invoice, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.regtest_get_payment(&payment_id).await.map(|invoice| invoice.into())
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn regtest_close_channel(
    funding_tx_id: String,
    vout: u32,
    force_close_after_s: Option<u64>,
) -> Result<String, BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        db.regtest_close_channel(&funding_tx_id, vout, force_close_after_s).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub fn activity_wipe_all() -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.wipe_all()
}

#[uniffi::export]
pub fn is_address_used(address: String) -> Result<bool, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.is_address_used(&address)
}

#[uniffi::export]
pub fn mark_activity_as_seen(activity_id: String, seen_at: u64) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.mark_activity_as_seen(&activity_id, seen_at)
}

#[uniffi::export]
pub fn upsert_transaction_details(details_list: Vec<TransactionDetails>) -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_transaction_details(&details_list)
}

#[uniffi::export]
pub fn get_transaction_details(tx_id: String) -> Result<Option<TransactionDetails>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_transaction_details(&tx_id)
}

#[uniffi::export]
pub fn get_all_transaction_details() -> Result<Vec<TransactionDetails>, ActivityError> {
    let guard = get_activity_db()?;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_transaction_details()
}

#[uniffi::export]
pub fn delete_transaction_details(tx_id: String) -> Result<bool, ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.delete_transaction_details(&tx_id)
}

#[uniffi::export]
pub fn wipe_all_transaction_details() -> Result<(), ActivityError> {
    let mut guard = get_activity_db()?;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.wipe_all_transaction_details()
}

#[uniffi::export]
pub async fn blocktank_remove_all_orders() -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        db.remove_all_orders().await
    }).await.unwrap()
}

#[uniffi::export]
pub async fn blocktank_remove_all_cjit_entries() -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        db.remove_all_cjit_entries().await
    }).await.unwrap()
}

#[uniffi::export]
pub async fn blocktank_wipe_all() -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        db.wipe_all().await
    }).await.unwrap()
}

#[uniffi::export]
pub async fn upsert_info(info: IBtInfo) -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let external_info: rust_blocktank_client::IBtInfo = info.into();
        db.upsert_info(&external_info).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn upsert_orders(orders: Vec<IBtOrder>) -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        let external_orders: Vec<rust_blocktank_client::IBtOrder> = orders.into_iter().map(|order| order.into()).collect();
        db.upsert_orders(&external_orders).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn upsert_cjit_entries(entries: Vec<ICJitEntry>) -> Result<(), BlocktankError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;

        let external_entries: Vec<rust_blocktank_client::ICJitEntry> = entries.into_iter().map(|e| e.into()).collect();
        db.upsert_cjit_entries(&external_entries).await
    }).await.unwrap_or_else(|e| Err(BlocktankError::ConnectionError {
        error_details: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn wipe_all_databases() -> Result<String, DbError> {
    let rt = ensure_runtime();

    // Wipe activity database - require it to be initialized
    {
        let cell = DB.get().ok_or(DbError::InitializationError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let mut guard = cell.lock().unwrap();
        let db = guard.activity_db.as_mut().ok_or(DbError::InitializationError {
            error_details: "Activity database not initialized. Call init_db first.".to_string()
        })?;
        db.wipe_all().map_err(|e| DbError::InitializationError {
            error_details: format!("Failed to wipe activity database: {}", e)
        })?;
    }

    // Wipe blocktank database - require it to be initialized
    rt.spawn(async move {
        let cell = ASYNC_DB.get().ok_or(DbError::InitializationError {
            error_details: "Database not initialized. Call init_db first.".to_string()
        })?;
        let guard = cell.lock().await;
        let db = guard.blocktank_db.as_ref().ok_or(DbError::InitializationError {
            error_details: "Blocktank database not initialized. Call init_db first.".to_string()
        })?;
        db.wipe_all().await.map_err(|e| DbError::InitializationError {
            error_details: format!("Failed to wipe blocktank database: {}", e)
        })?;
        Ok::<(), DbError>(())
    }).await.unwrap()?;

    Ok("All databases wiped successfully".to_string())
}

#[uniffi::export]
pub fn calculate_channel_liquidity_options(
    params: ChannelLiquidityParams,
) -> ChannelLiquidityOptions {
    crate::modules::blocktank::calculate_channel_liquidity_options(params)
}

#[uniffi::export]
pub fn get_default_lsp_balance(params: DefaultLspBalanceParams) -> u64 {
    crate::modules::blocktank::get_default_lsp_balance(params)
}

// ============================================================================
// Pubky Functions
// ============================================================================

#[uniffi::export]
pub fn resolve_pubky_url(uri: String) -> Result<String, PubkyError> {
    crate::modules::pubky::resolve_pubky_url(uri)
}

#[uniffi::export]
pub async fn fetch_pubky_file(uri: String) -> Result<Vec<u8>, PubkyError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        crate::modules::pubky::fetch_pubky_file(uri).await
    }).await.unwrap_or_else(|e| Err(PubkyError::ResolutionFailed {
        reason: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn start_pubky_auth(caps: String) -> Result<String, PubkyError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        crate::modules::pubky::start_pubky_auth(caps).await
    }).await.unwrap_or_else(|e| Err(PubkyError::AuthFailed {
        reason: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn cancel_pubky_auth() -> Result<(), PubkyError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        crate::modules::pubky::cancel_pubky_auth().await
    }).await.unwrap_or_else(|e| Err(PubkyError::AuthFailed {
        reason: format!("Runtime error: {}", e)
    }))
}

#[uniffi::export]
pub async fn complete_pubky_auth() -> Result<String, PubkyError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        crate::modules::pubky::complete_pubky_auth().await
    }).await.unwrap_or_else(|e| Err(PubkyError::AuthFailed {
        reason: format!("Runtime error: {}", e)
    }))
}

// ============================================================================
// Trezor Hardware Wallet Functions
// ============================================================================

fn get_trezor_manager() -> &'static TrezorManager {
    TREZOR_MANAGER.get_or_init(TrezorManager::new)
}

// ============================================================================
// Trezor / Bluetooth Functions
// ============================================================================

/// JNI function to initialize btleplug on Android.
/// This is called from Java via BluetoothInit.nativeInit().
///
/// The function name follows JNI naming convention:
/// Java_{package}_{class}_{method} where package dots become underscores
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_to_bitkit_services_BluetoothInit_nativeInit(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
) -> jni::sys::jboolean {
    use crate::modules::trezor::{is_ble_initialized, set_ble_initialized};

    // Already initialized
    if is_ble_initialized() {
        return jni::sys::JNI_TRUE;
    }

    // Initialize btleplug with the JNI environment
    match btleplug::platform::init(&env) {
        Ok(()) => {
            set_ble_initialized(true);
            jni::sys::JNI_TRUE
        }
        Err(e) => {
            // Log the error - this will be visible in logcat
            eprintln!("Failed to initialize btleplug: {:?}", e);
            jni::sys::JNI_FALSE
        }
    }
}

/// Initialize the Trezor manager with optional credential storage.
///
/// The credential_path is used to persist Bluetooth pairing credentials,
/// allowing reconnection without re-pairing.
///
/// NOTE: On Android, you must call the native initBle() function first!
#[uniffi::export]
pub async fn trezor_initialize(credential_path: Option<String>) -> Result<(), TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().initialize(credential_path).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Scan for available Trezor devices (USB + Bluetooth).
///
/// This performs an active Bluetooth scan and enumerates USB devices.
/// Returns a list of discovered devices.
#[uniffi::export]
pub async fn trezor_scan() -> Result<Vec<TrezorDeviceInfo>, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().scan().await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// List previously discovered devices without triggering a new scan.
#[uniffi::export]
pub async fn trezor_list_devices() -> Result<Vec<TrezorDeviceInfo>, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().list_devices().await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Connect to a Trezor device by its ID.
///
/// For Bluetooth devices, this will use stored credentials if available,
/// or trigger pairing if needed.
#[uniffi::export]
pub async fn trezor_connect(device_id: String) -> Result<TrezorFeatures, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().connect(&device_id).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Get a Bitcoin address from the connected Trezor device.
#[uniffi::export]
pub async fn trezor_get_address(params: TrezorGetAddressParams) -> Result<TrezorAddressResponse, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().get_address(params).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Get a public key (xpub) from the connected Trezor device.
#[uniffi::export]
pub async fn trezor_get_public_key(params: TrezorGetPublicKeyParams) -> Result<TrezorPublicKeyResponse, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().get_public_key(params).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Disconnect from the currently connected Trezor device.
#[uniffi::export]
pub async fn trezor_disconnect() -> Result<(), TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().disconnect().await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Check if the Trezor manager is initialized.
#[uniffi::export]
pub async fn trezor_is_initialized() -> bool {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().is_initialized().await
    }).await.unwrap_or(false)
}

/// Check if a Trezor device is currently connected.
#[uniffi::export]
pub async fn trezor_is_connected() -> bool {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().is_connected().await
    }).await.unwrap_or(false)
}

/// Get information about the currently connected Trezor device.
#[uniffi::export]
pub async fn trezor_get_connected_device() -> Option<TrezorDeviceInfo> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().get_connected_device().await
    }).await.unwrap_or(None)
}

/// Get the cached features of the currently connected Trezor device.
///
/// Returns the features that were obtained during `trezor_connect()`, without
/// triggering any device interaction. Returns None if no device is connected.
#[uniffi::export]
pub async fn trezor_get_features() -> Option<TrezorFeatures> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().get_features().await
    }).await.unwrap_or(None)
}

/// Sign a message with the connected Trezor device.
#[uniffi::export]
pub async fn trezor_sign_message(params: TrezorSignMessageParams) -> Result<TrezorSignedMessageResponse, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().sign_message(params).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Verify a message signature with the connected Trezor device.
#[uniffi::export]
pub async fn trezor_verify_message(params: TrezorVerifyMessageParams) -> Result<bool, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().verify_message(params).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Sign a Bitcoin transaction with the connected Trezor device.
#[uniffi::export]
pub async fn trezor_sign_tx(params: TrezorSignTxParams) -> Result<TrezorSignedTx, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().sign_tx(params).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Get the device's master root fingerprint as an 8-character hex string.
///
/// Returns the root fingerprint in the standard descriptor format (e.g., "73c5da0a").
/// Requires a connected device.
#[uniffi::export]
pub async fn trezor_get_device_fingerprint() -> Result<String, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().get_device_fingerprint().await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Sign a Bitcoin transaction from a PSBT (base64-encoded).
///
/// Parses the PSBT, extracts inputs/outputs/prev_txs, signs via the connected
/// Trezor device, and returns the signed transaction.
///
/// # Arguments
/// * `psbt_base64` - Base64-encoded PSBT data
/// * `network` - Bitcoin network type. Defaults to Bitcoin (mainnet) if None.
#[uniffi::export]
pub async fn trezor_sign_tx_from_psbt(psbt_base64: String, network: Option<TrezorCoinType>) -> Result<TrezorSignedTx, TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().sign_tx_from_psbt(psbt_base64, network).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

/// Clear stored Bluetooth pairing credentials for a specific Trezor device.
///
/// This removes any stored credentials, requiring re-pairing on the next connection.
/// Useful when a device has been reset or credentials have become stale.
#[uniffi::export]
pub async fn trezor_clear_credentials(device_id: String) -> Result<(), TrezorError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_trezor_manager().clear_credentials(&device_id).await
    }).await.unwrap_or_else(|e| Err(TrezorError::IoError { error_details: format!("Runtime error: {}", e) }))
}

// ============================================================================
// Account info FFI exports
// ============================================================================

/// Query account information for an extended public key via Electrum.
#[uniffi::export]
pub async fn trezor_get_account_info(
    extended_key: String,
    electrum_url: String,
    network: Option<TrezorCoinType>,
    gap_limit: Option<u32>,
) -> Result<AccountInfoResult, AccountInfoError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_account_info(&extended_key, &electrum_url, network, gap_limit).await
    }).await.unwrap_or_else(|e| Err(AccountInfoError::SyncError {
        error_details: format!("Runtime error: {}", e),
    }))
}

/// Query balance and UTXOs for a single Bitcoin address via Electrum.
#[uniffi::export]
pub async fn trezor_get_address_info(
    address: String,
    electrum_url: String,
    network: Option<TrezorCoinType>,
) -> Result<SingleAddressInfoResult, AccountInfoError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        get_address_info(&address, &electrum_url, network).await
    }).await.unwrap_or_else(|e| Err(AccountInfoError::SyncError {
        error_details: format!("Runtime error: {}", e),
    }))
}

/// Convert an account type to its corresponding script type.
#[uniffi::export]
pub fn trezor_account_type_to_script_type(account_type: AccountType) -> TrezorScriptType {
    account_type_to_script_type(account_type)
}

// ============================================================================
// Compose FFI exports
// ============================================================================

/// Compose a transaction offline for multiple fee levels.
///
/// No device interaction needed — pure coin selection and fee calculation.
#[uniffi::export]
pub fn trezor_precompose_transaction(params: TrezorPrecomposeParams) -> Vec<TrezorPrecomposedResult> {
    precompose_transaction(params)
}

/// Convert precomposed results into signing parameters for trezor_sign_tx.
///
/// The returned params have empty prev_txs — add them before signing.
#[uniffi::export]
pub fn trezor_precomposed_to_sign_params(
    inputs: Vec<TrezorPrecomposedInput>,
    outputs: Vec<TrezorPrecomposedOutput>,
    coin: Option<TrezorCoinType>,
) -> TrezorSignTxParams {
    precomposed_to_sign_params(inputs, outputs, coin)
}

/// Fetch previous transactions from Electrum for Trezor signing.
///
/// Takes transaction IDs (from TrezorSignTxParams inputs' prev_hash fields),
/// fetches the full transactions from Electrum, and returns them as
/// TrezorPrevTx structures ready to merge into TrezorSignTxParams.prev_txs.
///
/// Duplicate txids are automatically deduplicated.
#[uniffi::export]
pub async fn trezor_fetch_prev_txs(
    txids: Vec<String>,
    electrum_url: String,
) -> Result<Vec<TrezorPrevTx>, AccountInfoError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        fetch_prev_txs(txids, &electrum_url).await
    }).await.unwrap_or_else(|e| Err(AccountInfoError::SyncError {
        error_details: format!("Runtime error: {}", e),
    }))
}

/// Broadcast a signed raw transaction via Electrum.
///
/// Takes a hex-encoded serialized transaction and an Electrum server URL.
/// Returns the transaction ID on success.
#[uniffi::export]
pub async fn trezor_broadcast_raw_tx(
    serialized_tx: String,
    electrum_url: String,
) -> Result<String, AccountInfoError> {
    let rt = ensure_runtime();
    rt.spawn(async move {
        broadcast_raw_tx(serialized_tx, &electrum_url).await
    }).await.unwrap_or_else(|e| Err(AccountInfoError::SyncError {
        error_details: format!("Runtime error: {}", e),
    }))
}
