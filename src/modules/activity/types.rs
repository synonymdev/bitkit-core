use crate::activity::ActivityError;
use crate::modules::blocktank::BlocktankError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, uniffi::Enum)]
pub enum Activity {
    Onchain(OnchainActivity),
    Lightning(LightningActivity),
}

#[derive(Debug, uniffi::Enum)]
pub enum ActivityFilter {
    All,
    Lightning,
    Onchain,
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

    pub fn get_timestamp(&self) -> u64 {
        match self {
            Activity::Onchain(o) => o.timestamp,
            Activity::Lightning(l) => l.timestamp,
        }
    }

    pub fn get_created_at(&self) -> Option<u64> {
        match self {
            Activity::Onchain(o) => o.created_at,
            Activity::Lightning(l) => l.created_at,
        }
    }

    pub fn get_updated_at(&self) -> Option<u64> {
        match self {
            Activity::Onchain(o) => o.updated_at,
            Activity::Lightning(l) => l.updated_at,
        }
    }

    pub fn get_seen_at(&self) -> Option<u64> {
        match self {
            Activity::Onchain(o) => o.seen_at,
            Activity::Lightning(l) => l.seen_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, uniffi::Enum)]
pub enum ActivityType {
    #[serde(rename = "onchain")]
    Onchain,
    #[serde(rename = "lightning")]
    Lightning,
}

#[derive(Debug, Serialize, Deserialize, uniffi::Enum, Clone, PartialEq, Eq)]
pub enum PaymentType {
    Sent,
    Received,
}

#[derive(Debug, Serialize, Deserialize, uniffi::Enum, Clone, PartialEq, Eq)]
pub enum PaymentState {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone, uniffi::Record)]
pub struct OnchainActivity {
    pub id: String,
    pub tx_type: PaymentType,
    pub tx_id: String,
    pub value: u64,
    pub fee: u64,
    pub fee_rate: u64,
    pub address: String,
    pub confirmed: bool,
    pub timestamp: u64,
    pub is_boosted: bool,
    pub boost_tx_ids: Vec<String>,
    pub is_transfer: bool,
    pub does_exist: bool,
    pub confirm_timestamp: Option<u64>,
    pub channel_id: Option<String>,
    pub transfer_tx_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, uniffi::Record)]
pub struct LightningActivity {
    pub id: String,
    pub tx_type: PaymentType,
    pub status: PaymentState,
    pub value: u64,
    pub fee: Option<u64>,
    pub invoice: String,
    pub message: String,
    pub timestamp: u64,
    pub preimage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, uniffi::Record)]
pub struct ClosedChannelDetails {
    pub channel_id: String,
    pub counterparty_node_id: String,
    pub funding_txo_txid: String,
    pub funding_txo_index: u32,
    pub channel_value_sats: u64,
    pub closed_at: u64,
    pub outbound_capacity_msat: u64,
    pub inbound_capacity_msat: u64,
    pub counterparty_unspendable_punishment_reserve: u64,
    pub unspendable_punishment_reserve: u64,
    pub forwarding_fee_proportional_millionths: u32,
    pub forwarding_fee_base_msat: u32,
    pub channel_name: String,
    pub channel_closure_reason: String,
}

#[derive(Debug, Clone, uniffi::Record, Serialize, Deserialize)]
pub struct ActivityTags {
    pub activity_id: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record, Serialize, Deserialize)]
pub struct PreActivityMetadata {
    pub payment_id: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub is_receive: bool,
    pub fee_rate: u64,
    pub is_transfer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    pub created_at: u64,
}

/// Details about a transaction input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct TxInput {
    /// The transaction ID of the previous output being spent.
    pub txid: String,
    /// The output index of the previous output being spent.
    pub vout: u32,
    /// The script signature (hex-encoded).
    pub scriptsig: String,
    /// The witness stack (hex-encoded strings).
    pub witness: Vec<String>,
    /// The sequence number.
    pub sequence: u32,
}

/// Details about a transaction output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct TxOutput {
    /// The script public key (hex-encoded).
    pub scriptpubkey: String,
    /// The script public key type (e.g., "p2pkh", "p2sh", "p2wpkh", "p2wsh", "p2tr").
    pub scriptpubkey_type: Option<String>,
    /// The address corresponding to this script (if decodable).
    pub scriptpubkey_address: Option<String>,
    /// The value in satoshis.
    pub value: i64,
    /// The output index in the transaction.
    pub n: u32,
}

/// Details about an onchain transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct TransactionDetails {
    /// The transaction ID.
    pub tx_id: String,
    /// The net amount in this transaction (in satoshis).
    ///
    /// This is calculated as: (received - sent). For incoming payments,
    /// this will be positive. For outgoing payments, this will be negative.
    ///
    /// Note: This amount does NOT include transaction fees.
    pub amount_sats: i64,
    /// The transaction inputs with full details.
    pub inputs: Vec<TxInput>,
    /// The transaction outputs with full details.
    pub outputs: Vec<TxOutput>,
}

impl Default for SortDirection {
    fn default() -> Self {
        SortDirection::Desc
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum DbError {
    #[error("DB Activity Error: {error_details}")]
    DbActivityError { error_details: ActivityError },

    #[error("DB Blocktank Error: {error_details}")]
    DbBlocktankError { error_details: BlocktankError },

    #[error("Initialization Error: {error_details}")]
    InitializationError { error_details: String },
}

impl From<ActivityError> for DbError {
    fn from(error: ActivityError) -> Self {
        DbError::DbActivityError {
            error_details: error,
        }
    }
}

impl From<BlocktankError> for DbError {
    fn from(error: BlocktankError) -> Self {
        DbError::DbBlocktankError {
            error_details: error,
        }
    }
}
