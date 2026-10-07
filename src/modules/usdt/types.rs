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
    Base,
    Bsc,
    Solana,
    Tron,
}

impl UsdtDestination {
    pub(super) fn token(self) -> Option<Address> {
        Some(match self {
            Self::Arbitrum => TOKEN,
            Self::Ethereum => address!("dAC17F958D2ee523a2206206994597C13D831ec7"),
            Self::Polygon => address!("c2132D05D31c914a87C6611C10748AEb04B58e8F"),
            Self::Plasma => address!("B8CE59FC3717ada4C02eaDF9682A9e934F625ebb"),
            Self::Stable => address!("779Ded0c9e1022225f8E0630b35a9b54bE713736"),
            Self::Base => address!("fde4c96c8593536e31f229ea8f37b2ada2699bb2"),
            Self::Bsc => address!("55d398326f99059ff775485246999027b3197955"),
            Self::Solana | Self::Tron => return None,
        })
    }

    pub(super) fn network(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Ethereum => "ethereum",
            Self::Arbitrum => "arbitrum",
            Self::Polygon => "polygon",
            Self::Plasma => "plasma",
            Self::Base => "base",
            Self::Bsc => "bsc",
            Self::Solana => "solana",
            Self::Tron => "tron",
        }
    }

    pub(super) fn from_network(network: &str) -> Option<Self> {
        [
            Self::Ethereum,
            Self::Polygon,
            Self::Plasma,
            Self::Base,
            Self::Bsc,
            Self::Solana,
            Self::Tron,
        ]
        .into_iter()
        .find(|destination| destination.network() == network)
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
            Self::Arbitrum | Self::Base | Self::Bsc | Self::Solana | Self::Tron => None,
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
    /// Absent for a direct Arbitrum payment. The provider is fixed when this quote is approved.
    pub bridge_provider: Option<UsdtBridgeProvider>,
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
    /// Source-chain receipt proves USDT was returned to this wallet.
    BridgeRefunded,
    /// Another operation consumed the payment nonce.
    Replaced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum UsdtBridgeProvider {
    Usdt0,
    Orchestra,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct UsdtOrchestraTransfer {
    pub quote_id: String,
    pub funding_address: String,
    pub destination_tx: Option<String>,
    pub refund_tx: Option<String>,
    pub refund_amount: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct UsdtTransfer {
    pub id: String,
    /// Source transaction hash, absent until execution is observed.
    pub tx_hash: Option<String>,
    pub user_operation_hash: Option<String>,
    pub bridge_guid: Option<String>,
    pub orchestra: Option<UsdtOrchestraTransfer>,
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
