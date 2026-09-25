use alloy_primitives::{address, Address};
use serde::{Deserialize, Serialize};

pub(super) const CHAIN_ID: u64 = 42161;
pub(super) const TOKEN: Address = address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9");
pub(super) const OFT: Address = address!("14E4A1B13bf7F943c8ff7C51fb60FA964A298D92");
pub(super) const BRIDGE_HELPER: Address = address!("a90f03c856D01F698E7071B393387cd75a8a319A");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum UsdtDestination {
    Stable,
    Ethereum,
    Arbitrum,
    Polygon,
    Plasma,
}

impl UsdtDestination {
    pub(super) fn token(self) -> Address {
        match self {
            Self::Arbitrum => TOKEN,
            Self::Ethereum => address!("dAC17F958D2ee523a2206206994597C13D831ec7"),
            Self::Polygon => address!("c2132D05D31c914a87C6611C10748AEb04B58e8F"),
            Self::Plasma => address!("B8CE59FC3717ada4C02eaDF9682A9e934F625ebb"),
            Self::Stable => address!("779Ded0c9e1022225f8E0630b35a9b54bE713736"),
        }
    }

    pub(super) fn from_endpoint(eid: u32) -> Option<Self> {
        [Self::Ethereum, Self::Polygon, Self::Plasma, Self::Stable]
            .into_iter()
            .find(|d| d.endpoint() == Some(eid))
    }

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
    /// An explicit network in the payment URI; bare addresses have no restriction.
    pub chain_id: Option<u64>,
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
    /// Signed payment awaiting a conclusive source-chain outcome.
    Pending,
    /// Payment received on its destination chain.
    Confirmed,
    /// Source payment failed or was proven not to have executed.
    Failed,
    /// Source payment executed; destination delivery is pending.
    Bridging,
    /// Delivery is blocked or its message could not be recovered; it may still complete.
    BridgeNeedsAttention,
    /// Delivery was permanently stopped. This does not imply a refund of source funds or fees.
    BridgeFailed,
    /// Another operation consumed the payment nonce.
    Replaced,
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct UsdtTransfer {
    pub id: String,
    /// Source transaction hash, absent until execution is observed.
    pub tx_hash: Option<String>,
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
}

impl UsdtTransfer {
    pub(super) fn mark_unexecuted(&mut self, status: UsdtTransferStatus) {
        self.status = status;
        self.received_amount = 0;
        self.fee = Some(0);
    }
}
