use serde::{Deserialize, Serialize};

#[derive(Debug, uniffi::Enum)]
pub enum Activity {
    Onchain(OnchainActivity),
    Lightning(LightningActivity),
}

impl Activity {
    pub fn get_id(&self) -> &str {
        match self {
            Activity::Onchain(o) => &o.id,
            Activity::Lightning(l) => &l.id,
        }
    }

    pub fn get_activity_type(&self) -> ActivityType {
        match self {
            Activity::Onchain(_) => ActivityType::Onchain,
            Activity::Lightning(_) => ActivityType::Lightning,
        }
    }

    pub fn get_timestamp(&self) -> i64 {
        match self {
            Activity::Onchain(o) => o.timestamp,
            Activity::Lightning(l) => l.timestamp,
        }
    }

    pub fn get_created_at(&self) -> Option<i64> {
        match self {
            Activity::Onchain(o) => o.created_at,
            Activity::Lightning(l) => l.created_at,
        }
    }

    pub fn get_updated_at(&self) -> Option<i64> {
        match self {
            Activity::Onchain(o) => o.updated_at,
            Activity::Lightning(l) => l.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, uniffi::Enum)]
pub enum ActivityType {
    #[serde(rename = "onchain")]
    Onchain,
    #[serde(rename = "lightning")]
    Lightning,
}

#[derive(Debug, Serialize, Deserialize, uniffi::Enum)]
pub enum PaymentType {
    Sent,
    Received,
}

#[derive(Debug, Serialize, Deserialize, uniffi::Enum)]
pub enum PaymentState {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, uniffi::Record)]
pub struct OnchainActivity {
    pub id: String,
    pub activity_type: ActivityType,
    pub tx_type: PaymentType,
    pub tx_id: String,
    pub value: i64,
    pub fee: i64,
    pub fee_rate: i64,
    pub address: String,
    pub confirmed: bool,
    pub timestamp: i64,
    pub is_boosted: bool,
    pub is_transfer: bool,
    pub does_exist: bool,
    pub confirm_timestamp: Option<i64>,
    pub channel_id: Option<String>,
    pub transfer_tx_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, uniffi::Record)]
pub struct LightningActivity {
    pub id: String,
    pub activity_type: ActivityType,
    pub tx_type: PaymentType,
    pub status: PaymentState,
    pub value: i64,
    pub fee: Option<i64>,
    pub invoice: String,
    pub message: String,
    pub timestamp: i64,
    pub preimage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub enum SortDirection {
    Asc,
    Desc,
}