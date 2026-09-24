use alloy_primitives::{address, Address};
use serde::{Deserialize, Serialize};

pub(super) const CHAIN_ID: u64 = 42161;
pub(super) const TOKEN: Address = address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9");
pub(super) const OFT: Address = address!("14E4A1B13bf7F943c8ff7C51fb60FA964A298D92");
pub(super) const BRIDGE_HELPER: Address = address!("a90f03c856D01F698E7071B393387cd75a8a319A");
pub(super) const EXPLORER: &str = "https://arbiscan.io";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum UsdtDestination {
    Stable,
    Ethereum,
    Arbitrum,
    Polygon,
    Plasma,
}

impl UsdtDestination {
    pub(super) fn endpoint(self) -> Option<u32> {
        match self {
            Self::Stable => Some(30396),
            Self::Ethereum => Some(30101),
            Self::Arbitrum => None,
            Self::Polygon => Some(30109),
            Self::Plasma => Some(30383),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct UsdtPaymentRequest {
    pub recipient: String,
    pub amount: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct UsdtQuote {
    pub id: String,
    pub recipient: String,
    pub destination: UsdtDestination,
    pub amount: u64,
    pub received_amount: u64,
    pub maximum_fee: u64,
    pub expires_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum UsdtTransferStatus {
    Pending,
    Confirmed,
    Failed,
    Bridging,
    BridgeNeedsAttention,
    Replaced,
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct UsdtTransfer {
    pub id: String,
    pub tx_hash: String,
    pub user_operation_hash: Option<String>,
    pub bridge_guid: Option<String>,
    pub recipient: String,
    pub destination: UsdtDestination,
    pub amount: u64,
    pub received_amount: u64,
    pub fee: Option<u64>,
    pub is_incoming: bool,
    pub status: UsdtTransferStatus,
    pub timestamp: u64,
    pub explorer_url: String,
}
