uniffi::setup_scaffolding!();

mod modules;

use once_cell::sync::OnceCell;
use std::sync::Mutex;

pub use modules::scanner::{
    Scanner,
    DecodingError
};
pub use modules::lnurl;
pub use modules::onchain;
pub use modules::activity;
use crate::activity::{
    ActivityError,
    ActivityDB,
    OnchainActivity,
    LightningActivity,
    Activity
};
use crate::onchain::{
    AddressError,
    ValidationResult
};


pub struct DatabaseConnections {
    activity_db: Option<ActivityDB>,
    // blocktank_db: Option<BlocktankDB>,
}

#[derive(thiserror::Error, Debug, uniffi::Enum)]
pub enum DbError {
    #[error("Activity DB Error: {0}")]
    ActivityError(#[from] ActivityError),
    // #[error("Blocktank DB Error: {0}")]
    // BlocktankError(#[from] BlocktankError),
    #[error("Database initialization failed: {message}")]
    InitializationError { message: String },
}

static DB: OnceCell<Mutex<DatabaseConnections>> = OnceCell::new();

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
    DB.get_or_init(|| {
        Mutex::new(DatabaseConnections {
            activity_db: None,
            // blocktank_db: None,
        })
    });

    // Create the database connections
    let activity_db = ActivityDB::new(&format!("{}/activity.db", base_path))?;
    // let blocktank_db = BlocktankDB::new(&format!("{}/blocktank.db", base_path))?;

    // Store the connections in our global state
    if let Some(cell) = DB.get() {
        let mut guard = cell.lock().unwrap();
        guard.activity_db = Some(activity_db);
        // guard.blocktank_db = Some(blocktank_db);
        Ok("Databases initialized successfully".to_string())
    } else {
        Err(DbError::InitializationError {
            message: "Failed to initialize global DB state".to_string()
        })
    }
}

#[uniffi::export]
pub fn get_all_onchain_activities(limit: Option<u32>) -> Result<Vec<OnchainActivity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_onchain_activities(limit)
}

#[uniffi::export]
pub fn get_all_lightning_activities(limit: Option<u32>) -> Result<Vec<LightningActivity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_lightning_activities(limit)
}

#[uniffi::export]
pub fn insert_activity(activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    match activity {
        Activity::Onchain(onchain) => db.insert_onchain_activity(&onchain),
        Activity::Lightning(lightning) => db.insert_lightning_activity(&lightning),
    }
}

#[uniffi::export]
pub fn update_activity(activity_id: String, activity: Activity) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    match activity {
        Activity::Onchain(onchain) => db.update_onchain_activity_by_id(&activity_id, &onchain),
        Activity::Lightning(lightning) => db.update_lightning_activity_by_id(&activity_id, &lightning),
    }
}

#[uniffi::export]
pub fn get_all_activities(limit: Option<u32>) -> Result<Vec<Activity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_all_activities(limit)
}

#[uniffi::export]
pub fn get_activity_by_id(activity_id: String) -> Result<Option<Activity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activity_by_id(&activity_id)
}

#[uniffi::export]
pub fn delete_activity_by_id(activity_id: String) -> Result<bool, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.delete_activity_by_id(&activity_id)
}

#[uniffi::export]
pub fn add_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.add_tags(&activity_id, &tags)
}

#[uniffi::export]
pub fn remove_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let mut guard = cell.lock().unwrap();
    let db = guard.activity_db.as_mut().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.remove_tags(&activity_id, &tags)
}

#[uniffi::export]
pub fn get_tags(activity_id: String) -> Result<Vec<String>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_tags(&activity_id)
}

#[uniffi::export]
pub fn get_activities_by_tag(tag: String, limit: Option<u32>) -> Result<Vec<Activity>, ActivityError> {
    let cell = DB.get().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    let guard = cell.lock().unwrap();
    let db = guard.activity_db.as_ref().ok_or(ActivityError::ConnectionError {
        message: "Database not initialized. Call init_db first.".to_string()
    })?;
    db.get_activities_by_tag(&tag, limit)
}