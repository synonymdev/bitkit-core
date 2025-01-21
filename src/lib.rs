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
use crate::modules::blocktank::{BlocktankDB, BlocktankError, IBtInfo};
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
    ensure_runtime();
    Scanner::decode(invoice).await
}

#[uniffi::export]
pub async fn get_lnurl_invoice(address: String, amount_satoshis: u64) -> Result<String, lnurl::LnurlError> {
    ensure_runtime();
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

/// Blocktank Module
#[uniffi::export]
pub async fn update_blocktank_url(new_url: String) -> Result<(), BlocktankError> {
    ensure_runtime();
    let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.blocktank_db.as_mut().ok_or(BlocktankError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.update_blocktank_url(&new_url).await
}

#[uniffi::export]
pub async fn get_info(refresh: Option<bool>) -> Result<Option<IBtInfo>, BlocktankError> {
    ensure_runtime();
    let cell = ASYNC_DB.get().ok_or(BlocktankError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().await;
    let db = guard.blocktank_db.as_ref().ok_or(BlocktankError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;

    if refresh.unwrap_or(false) {
        match db.fetch_and_store_info().await {
            Ok(info) => Ok(Some(info.into())),
            Err(_) => Ok(None),
        }
    } else {
        let info = db.get_info().await?;
        Ok(info.map(|info| info.into()))
    }
}