uniffi::setup_scaffolding!();

mod modules;

use once_cell::sync::OnceCell;
use std::sync::Mutex;
use thiserror::Error;
pub use modules::scanner::{
    Scanner,
    DecodingError
};
pub use modules::lnurl;
pub use modules::onchain;
pub use modules::activity;
use crate::activity::{ActivityError, ActivityDB, OnchainActivity, LightningActivity, Activity, ActivityFilter, SortDirection, PaymentType, DbError};
use crate::modules::blocktank::{BlocktankDB, BlocktankError, IBtInfo, IBtOrder, CreateOrderOptions, BtOrderState2, IBt0ConfMinTxFeeWindow, IBtEstimateFeeResponse, IBtEstimateFeeResponse2, CreateCjitOptions, ICJitEntry, CJitStateEnum, IBtBolt11Invoice};
use crate::onchain::{
    AddressError,
    ValidationResult
};

use std::sync::Mutex as StdMutex;
use tokio::runtime::Runtime;
use tokio::sync::Mutex as TokioMutex;

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

#[uniffi::export]
pub async fn decode(invoice: String) -> Result<Scanner, DecodingError> {
    Scanner::decode(invoice).await
}

#[uniffi::export]
pub async fn get_lnurl_invoice(address: String, amount_satoshis: u64) -> Result<String, lnurl::LnurlError> {
    lnurl::get_lnurl_invoice(&address, amount_satoshis).await
}

#[uniffi::export]
pub fn validate_bitcoin_address(address: String) -> Result<ValidationResult, AddressError> {
    onchain::BitcoinAddressValidator::validate_address(&address)
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
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities(filter, tx_type, tags, search, min_date, max_date, limit, sort_direction)
}

#[uniffi::export]
pub fn upsert_activity(activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_activity(&activity)
}

#[uniffi::export]
pub fn insert_activity(activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
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
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
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
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activity_by_id(&activity_id)
}

#[uniffi::export]
pub fn delete_activity_by_id(activity_id: String) -> Result<bool, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.delete_activity_by_id(&activity_id)
}

#[uniffi::export]
pub fn add_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.add_tags(&activity_id, &tags)
}

#[uniffi::export]
pub fn remove_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.remove_tags(&activity_id, &tags)
}

#[uniffi::export]
pub fn get_tags(activity_id: String) -> Result<Vec<String>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_tags(&activity_id)
}

#[uniffi::export]
pub fn get_activities_by_tag(tag: String, limit: Option<u32>, sort_direction: Option<SortDirection>) -> Result<Vec<Activity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities_by_tag(&tag, limit, sort_direction)
}

#[uniffi::export]
pub fn get_all_unique_tags() -> Result<Vec<String>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_unique_tags()
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
            &signature
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

        let notification_type = notification_type.unwrap_or("orderPaymentConfirmed".to_string());
        db.test_notification(&device_token, &secret_message, &notification_type).await
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