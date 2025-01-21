uniffi::setup_scaffolding!();

mod modules;

use once_cell::sync::OnceCell;
use tokio::sync::Mutex;
use rust_blocktank_client::{BtOrderState, IBtOrder};
use tokio::runtime::Runtime;
pub use modules::scanner::{
    Scanner,
    DecodingError
};
pub use modules::lnurl;
pub use modules::onchain;
pub use modules::activity;
use crate::activity::{ActivityError, ActivityDB, Activity, ActivityFilter, SortDirection, PaymentType, DbError};
use crate::modules::blocktank::{BlocktankDB, BlocktankError, IBtInfo};
use crate::onchain::{
    AddressError,
    ValidationResult
};
static RUNTIME: OnceCell<Runtime> = OnceCell::new();


pub struct DatabaseConnections {
    activity_db: Option<ActivityDB>,
    blocktank_db: Option<BlocktankDB>,
}

static DB: OnceCell<Mutex<DatabaseConnections>> = OnceCell::new();

fn init_runtime() {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Failed to create Tokio runtime")
    });
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
pub async fn init_db(base_path: String) -> Result<String, DbError> {
    init_runtime();
    DB.get_or_init(|| {
        Mutex::new(DatabaseConnections {
            activity_db: None,
            blocktank_db: None,
        })
    });

    // Create the database connections
    let activity_db = ActivityDB::new(&format!("{}/activity.db", base_path))?;
    let blocktank_db = BlocktankDB::new(&format!("{}/blocktank.db", base_path), None).await?;

    // Store the connections in our global state
    let cell = DB.get().unwrap();
    let mut guard = cell.lock().await;
    guard.activity_db = Some(activity_db);
    guard.blocktank_db = Some(blocktank_db);

    Ok("Databases initialized successfully".to_string())
}



#[uniffi::export]
pub async fn get_activities(
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
    let guard = cell.lock().await;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities(filter, tx_type, tags, search, min_date, max_date, limit, sort_direction)
}

#[uniffi::export]
pub async fn upsert_activity(activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.upsert_activity(&activity)
}

#[uniffi::export]
pub async fn insert_activity(activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    match activity {
        Activity::Onchain(onchain) => db.insert_onchain_activity(&onchain),
        Activity::Lightning(lightning) => db.insert_lightning_activity(&lightning),
    }
}

#[uniffi::export]
pub async fn update_activity(activity_id: String, activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    match activity {
        Activity::Onchain(onchain) => db.update_onchain_activity_by_id(&activity_id, &onchain),
        Activity::Lightning(lightning) => db.update_lightning_activity_by_id(&activity_id, &lightning),
    }
}

#[uniffi::export]
pub async fn get_activity_by_id(activity_id: String) -> Result<Option<Activity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().await;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activity_by_id(&activity_id)
}

#[uniffi::export]
pub async fn delete_activity_by_id(activity_id: String) -> Result<bool, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.delete_activity_by_id(&activity_id)
}

#[uniffi::export]
pub async fn add_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.add_tags(&activity_id, &tags)
}

#[uniffi::export]
pub async fn remove_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().await;
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.remove_tags(&activity_id, &tags)
}

#[uniffi::export]
pub async fn get_tags(activity_id: String) -> Result<Vec<String>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().await;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_tags(&activity_id)
}

#[uniffi::export]
pub async fn get_activities_by_tag(tag: String, limit: Option<u32>, sort_direction: Option<SortDirection>) -> Result<Vec<Activity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().await;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities_by_tag(&tag, limit, sort_direction)
}

#[uniffi::export]
pub async fn get_all_unique_tags() -> Result<Vec<String>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().await;
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        error_details: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_unique_tags()
}

/// Blocktank Module
#[uniffi::export]
pub async fn update_blocktank_url(new_url: String) -> Result<(), BlocktankError> {
    let cell = DB.get().ok_or(BlocktankError::ConnectionError {
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
    let cell = DB.get().ok_or(BlocktankError::ConnectionError {
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