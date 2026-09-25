use alloy_primitives::{address, Address};
use serde::{Deserialize, Serialize};

pub(super) const CHAIN_ID: u64 = 42161;
pub(super) const TOKEN: Address = address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9");

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
    pub amount: u64,
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
    /// Another operation consumed the payment nonce.
    Replaced,
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct UsdtTransfer {
    pub id: String,
    /// Source transaction hash, absent until execution is observed.
    pub tx_hash: Option<String>,
    pub user_operation_hash: Option<String>,
    pub recipient: String,
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
