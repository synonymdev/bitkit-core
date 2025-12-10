uniffi::setup_scaffolding!();

mod modules;

use once_cell::sync::OnceCell;
pub use modules::scanner::{
    Scanner,
    DecodingError
};
pub use modules::lnurl;
pub use modules::onchain;
pub use modules::activity;
use crate::activity::{ActivityError, ActivityDB, OnchainActivity, LightningActivity, Activity, ActivityFilter, SortDirection, PaymentType, DbError, ClosedChannelDetails, ActivityTags, PreActivityMetadata, TransactionDetails, TxInput, TxOutput};
use crate::modules::blocktank::{BlocktankDB, BlocktankError, IBtInfo, IBtOrder, CreateOrderOptions, BtOrderState2, IBt0ConfMinTxFeeWindow, IBtEstimateFeeResponse, IBtEstimateFeeResponse2, CreateCjitOptions, ICJitEntry, CJitStateEnum, IBtBolt11Invoice, IGift, ChannelLiquidityOptions, ChannelLiquidityParams, DefaultLspBalanceParams};
use crate::onchain::{AddressError, ValidationResult, GetAddressResponse, Network, GetAddressesResponse};
pub use crate::onchain::WordCount;

use std::sync::Mutex as StdMutex;
use tokio::runtime::Runtime;
use tokio::sync::Mutex as TokioMutex;
use crate::modules::trezor;
use crate::modules::trezor::{AccountInfoDetails, AmountUnit, CommonParams, ComposeAccount, ComposeOutput, ComposeTransactionParams, DeepLinkResult, DefaultAccountType, FeeLevel, GetAccountInfoParams, GetAddressParams, MultisigRedeemScriptType, RefTransaction, SignMessageParams, SignTransactionParams, TokenFilter, TrezorConnectError, TrezorEnvironment, TrezorResponsePayload, TxAckPaymentRequest, TxInputType, TxOutputType, UnlockPath, VerifyMessageParams, XrpMarker};
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
pub fn trezor_get_features(
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };
    match trezor_client.get_features(request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_get_address(
    path: String,
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>,
    address: Option<String>,
    showOnTrezor: Option<bool>,
    chunkify: Option<bool>,
    useEventListener: Option<bool>,
    coin: Option<String>,
    crossChain: Option<bool>,
    multisig: Option<MultisigRedeemScriptType>,
    scriptType: Option<String>,
    unlockPath: Option<UnlockPath>,
    common: Option<CommonParams>,
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };

    let coin = Some(coin.unwrap_or_else(|| "btc".to_string()));
    let params = GetAddressParams {
        path,
        address,
        showOnTrezor,
        chunkify,
        useEventListener,
        coin,
        crossChain,
        multisig,
        scriptType,
        unlockPath,
        common,
    };

    match trezor_client.get_address(params, request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_get_account_info(
    coin: String,
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>,
    path: Option<String>,
    descriptor: Option<String>,
    details: Option<AccountInfoDetails>,
    tokens: Option<TokenFilter>,
    page: Option<u32>,
    pageSize: Option<u32>,
    from: Option<u32>,
    to: Option<u32>,
    gap: Option<u32>,
    contractFilter: Option<String>,
    marker: Option<XrpMarker>,
    defaultAccountType: Option<DefaultAccountType>,
    suppressBackupWarning: Option<bool>,
    common: Option<CommonParams>,
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };

    let params = GetAccountInfoParams {
        path,
        descriptor,
        coin,
        details,
        tokens,
        page,
        pageSize,
        from,
        to,
        gap,
        contractFilter,
        marker,
        defaultAccountType,
        suppressBackupWarning,
        common,
    };

    match trezor_client.get_account_info(params, request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_handle_deep_link(
    callback_url: String,
) -> Result<TrezorResponsePayload, TrezorConnectError> {
    match trezor::handle_deep_link(callback_url) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_verify_message(
    address: String,
    signature: String,
    message: String,
    coin: String,
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>,
    hex: Option<bool>,
    common: Option<CommonParams>,
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };

    let params = VerifyMessageParams {
        address,
        signature,
        message,
        coin,
        hex,
        common,
    };

    match trezor_client.verify_message(params, request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_sign_message(
    path: String,
    message: String,
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>,
    coin: Option<String>,
    hex: Option<bool>,
    no_script_type: Option<bool>,
    common: Option<CommonParams>,
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };

    let params = SignMessageParams {
        path,
        coin,
        message,
        hex,
        no_script_type,
        common,
    };

    match trezor_client.sign_message(params, request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_sign_transaction(
    coin: String,
    inputs: Vec<TxInputType>,
    outputs: Vec<TxOutputType>,
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>,
    ref_txs: Option<Vec<RefTransaction>>,
    payment_requests: Option<Vec<TxAckPaymentRequest>>,
    locktime: Option<u32>,
    version: Option<u32>,
    expiry: Option<u32>,
    version_group_id: Option<u32>,
    overwintered: Option<bool>,
    timestamp: Option<u32>,
    branch_id: Option<u32>,
    push: Option<bool>,
    amount_unit: Option<AmountUnit>,
    unlock_path: Option<UnlockPath>,
    serialize: Option<bool>,
    chunkify: Option<bool>,
    common: Option<CommonParams>,
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };

    let params = SignTransactionParams {
        coin,
        inputs,
        outputs,
        refTxs: ref_txs,
        paymentRequests: payment_requests,
        locktime,
        version,
        expiry,
        versionGroupId: version_group_id,
        overwintered,
        timestamp,
        branchId: branch_id,
        push,
        amountUnit: amount_unit,
        unlockPath: unlock_path,
        serialize,
        chunkify,
        common,
    };

    match trezor_client.sign_transaction(params, request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
}

#[uniffi::export]
pub fn trezor_compose_transaction(
    outputs: Vec<ComposeOutput>,
    coin: String,
    callback_url: String,
    request_id: Option<String>,
    trezor_environment: Option<TrezorEnvironment>,
    push: Option<bool>,
    sequence: Option<u32>,
    account: Option<ComposeAccount>,
    fee_levels: Option<Vec<FeeLevel>>,
    skip_permutation: Option<bool>,
    common: Option<CommonParams>,
) -> Result<DeepLinkResult, TrezorConnectError> {
    let trezor_environment = trezor_environment.unwrap_or(TrezorEnvironment::Production);
    let trezor_client = match trezor::TrezorConnectClient::new(trezor_environment, callback_url) {
        Ok(client) => client,
        Err(e) => return Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    };

    let params = ComposeTransactionParams {
        outputs,
        coin,
        push,
        sequence,
        account,
        fee_levels,
        skip_permutation,
        common,
    };

    match trezor_client.compose_transaction(params, request_id) {
        Ok(result) => Ok(result),
        Err(e) => Err(TrezorConnectError::ClientError { error_details: e.to_string() }),
    }
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
