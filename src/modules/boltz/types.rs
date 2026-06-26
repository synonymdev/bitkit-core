use boltz_client::network::{BitcoinChain, Chain, Network as BoltzClientNetwork};
use serde::{Deserialize, Serialize};

/// Bitcoin network selection for Boltz swaps. Maps to the networks Boltz
/// operates on (mainnet, testnet, regtest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, Serialize, Deserialize)]
pub enum BoltzNetwork {
    Mainnet,
    Testnet,
    Regtest,
}

impl BoltzNetwork {
    pub(crate) fn as_client_network(self) -> BoltzClientNetwork {
        match self {
            BoltzNetwork::Mainnet => BoltzClientNetwork::Mainnet,
            BoltzNetwork::Testnet => BoltzClientNetwork::Testnet,
            BoltzNetwork::Regtest => BoltzClientNetwork::Regtest,
        }
    }

    pub(crate) fn as_bitcoin_chain(self) -> BitcoinChain {
        match self {
            BoltzNetwork::Mainnet => BitcoinChain::Bitcoin,
            BoltzNetwork::Testnet => BitcoinChain::BitcoinTestnet,
            BoltzNetwork::Regtest => BitcoinChain::BitcoinRegtest,
        }
    }

    pub(crate) fn as_chain(self) -> Chain {
        Chain::Bitcoin(self.as_bitcoin_chain())
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            BoltzNetwork::Mainnet => "mainnet",
            BoltzNetwork::Testnet => "testnet",
            BoltzNetwork::Regtest => "regtest",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<BoltzNetwork> {
        match s {
            "mainnet" => Some(BoltzNetwork::Mainnet),
            "testnet" => Some(BoltzNetwork::Testnet),
            "regtest" => Some(BoltzNetwork::Regtest),
            _ => None,
        }
    }
}

/// The direction of a Boltz swap.
///
/// - `Submarine`: onchain Bitcoin -> Lightning (the user locks onchain funds,
///   Boltz pays a Lightning invoice).
/// - `Reverse`: Lightning -> onchain Bitcoin (the user pays a Boltz hold
///   invoice, Boltz locks onchain funds the user then claims).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, Serialize, Deserialize)]
pub enum BoltzSwapType {
    Submarine,
    Reverse,
}

impl BoltzSwapType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            BoltzSwapType::Submarine => "submarine",
            BoltzSwapType::Reverse => "reverse",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<BoltzSwapType> {
        match s {
            "submarine" => Some(BoltzSwapType::Submarine),
            "reverse" => Some(BoltzSwapType::Reverse),
            _ => None,
        }
    }
}

/// Typed view of the Boltz swap lifecycle. `Unknown` carries the raw status so
/// new server-side states don't break the bindings.
///
/// See <https://api.docs.boltz.exchange/lifecycle.html>.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum, Serialize, Deserialize)]
pub enum BoltzSwapStatus {
    /// `swap.created` — initial state.
    SwapCreated,
    /// `invoice.set` — invoice attached to a submarine swap.
    InvoiceSet,
    /// `transaction.mempool` — a lockup transaction is in the mempool.
    TransactionMempool,
    /// `transaction.confirmed` — a lockup transaction confirmed.
    TransactionConfirmed,
    /// `invoice.pending` — Boltz is paying the submarine swap invoice.
    InvoicePending,
    /// `invoice.paid` — submarine swap invoice paid by Boltz.
    InvoicePaid,
    /// `invoice.settled` — reverse swap invoice settled (preimage revealed).
    InvoiceSettled,
    /// `invoice.failedToPay` — submarine swap invoice could not be paid; refund.
    InvoiceFailedToPay,
    /// `invoice.expired` — reverse swap invoice expired before payment.
    InvoiceExpired,
    /// `transaction.claim.pending` — Boltz ready for a cooperative claim.
    TransactionClaimPending,
    /// `transaction.claimed` — onchain funds claimed.
    TransactionClaimed,
    /// `transaction.refunded` — onchain funds refunded.
    TransactionRefunded,
    /// `transaction.lockupFailed` — wrong amount locked; can refund.
    TransactionLockupFailed,
    /// `transaction.failed` — Boltz failed to lock the agreed funds.
    TransactionFailed,
    /// `swap.expired` — swap expired without completing.
    SwapExpired,
    /// Any status not yet modelled. `raw` holds the verbatim Boltz status.
    Unknown { raw: String },
}

impl BoltzSwapStatus {
    /// Map a raw Boltz status string to a typed status.
    pub fn from_raw(raw: &str) -> BoltzSwapStatus {
        match raw {
            "swap.created" => BoltzSwapStatus::SwapCreated,
            "invoice.set" => BoltzSwapStatus::InvoiceSet,
            "transaction.mempool" => BoltzSwapStatus::TransactionMempool,
            "transaction.confirmed" => BoltzSwapStatus::TransactionConfirmed,
            "invoice.pending" => BoltzSwapStatus::InvoicePending,
            "invoice.paid" => BoltzSwapStatus::InvoicePaid,
            "invoice.settled" => BoltzSwapStatus::InvoiceSettled,
            "invoice.failedToPay" => BoltzSwapStatus::InvoiceFailedToPay,
            "invoice.expired" => BoltzSwapStatus::InvoiceExpired,
            "transaction.claim.pending" => BoltzSwapStatus::TransactionClaimPending,
            "transaction.claimed" => BoltzSwapStatus::TransactionClaimed,
            "transaction.refunded" => BoltzSwapStatus::TransactionRefunded,
            "transaction.lockupFailed" => BoltzSwapStatus::TransactionLockupFailed,
            "transaction.failed" => BoltzSwapStatus::TransactionFailed,
            "swap.expired" => BoltzSwapStatus::SwapExpired,
            other => BoltzSwapStatus::Unknown {
                raw: other.to_string(),
            },
        }
    }

    /// The raw Boltz status string this typed status was derived from.
    pub fn as_raw(&self) -> String {
        match self {
            BoltzSwapStatus::SwapCreated => "swap.created".to_string(),
            BoltzSwapStatus::InvoiceSet => "invoice.set".to_string(),
            BoltzSwapStatus::TransactionMempool => "transaction.mempool".to_string(),
            BoltzSwapStatus::TransactionConfirmed => "transaction.confirmed".to_string(),
            BoltzSwapStatus::InvoicePending => "invoice.pending".to_string(),
            BoltzSwapStatus::InvoicePaid => "invoice.paid".to_string(),
            BoltzSwapStatus::InvoiceSettled => "invoice.settled".to_string(),
            BoltzSwapStatus::InvoiceFailedToPay => "invoice.failedToPay".to_string(),
            BoltzSwapStatus::InvoiceExpired => "invoice.expired".to_string(),
            BoltzSwapStatus::TransactionClaimPending => "transaction.claim.pending".to_string(),
            BoltzSwapStatus::TransactionClaimed => "transaction.claimed".to_string(),
            BoltzSwapStatus::TransactionRefunded => "transaction.refunded".to_string(),
            BoltzSwapStatus::TransactionLockupFailed => "transaction.lockupFailed".to_string(),
            BoltzSwapStatus::TransactionFailed => "transaction.failed".to_string(),
            BoltzSwapStatus::SwapExpired => "swap.expired".to_string(),
            BoltzSwapStatus::Unknown { raw } => raw.clone(),
        }
    }

    /// Whether the swap has reached a terminal state and no further action is
    /// possible or required.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            BoltzSwapStatus::TransactionClaimed
                | BoltzSwapStatus::TransactionRefunded
                | BoltzSwapStatus::InvoiceSettled
                | BoltzSwapStatus::TransactionFailed
                | BoltzSwapStatus::InvoiceExpired
                | BoltzSwapStatus::SwapExpired
        )
    }
}

/// Fees and limits for a swap pair, used to size a swap and present costs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BoltzPairInfo {
    /// Pair hash identifying the current terms (passed back to Boltz if needed).
    pub hash: String,
    /// Exchange rate of the pair.
    pub rate: f64,
    /// Minimum swap amount in satoshis.
    pub minimal_sat: u64,
    /// Maximum swap amount in satoshis.
    pub maximal_sat: u64,
    /// Boltz service fee as a percentage of the swap amount.
    pub fee_percentage: f64,
    /// Estimated absolute miner fees in satoshis.
    pub miner_fees_sat: u64,
}

/// Result of creating a submarine swap (onchain -> Lightning).
///
/// The caller funds `address` with `expected_amount_sat` from its onchain
/// wallet; Boltz then pays the Lightning invoice supplied at creation.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SubmarineSwapResponse {
    pub id: String,
    /// Onchain lockup address to send funds to.
    pub address: String,
    /// BIP21 URI for the lockup payment.
    pub bip21: String,
    /// Exact amount in satoshis the caller must send to `address`.
    pub expected_amount_sat: u64,
    /// Whether Boltz will accept a zero-conf lockup.
    pub accept_zero_conf: bool,
    /// Onchain timeout height after which a refund is possible.
    pub timeout_block_height: u64,
}

/// Result of creating a reverse swap (Lightning -> onchain).
///
/// The caller pays `invoice` from its Lightning node; once Boltz locks funds at
/// `lockup_address`, the module claims them to the provided onchain address.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ReverseSwapResponse {
    pub id: String,
    /// Hold invoice the caller must pay via Lightning.
    pub invoice: String,
    /// Address Boltz locks the onchain funds to.
    pub lockup_address: String,
    /// Amount in satoshis that will be received onchain (after Boltz fees).
    pub onchain_amount_sat: u64,
    /// Onchain timeout height for the swap.
    pub timeout_block_height: u64,
}

/// A persisted swap and its current state, returned by the listing/query APIs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BoltzSwap {
    pub id: String,
    pub swap_type: BoltzSwapType,
    pub status: BoltzSwapStatus,
    pub network: BoltzNetwork,
    /// Deterministic BIP85 index used to derive this swap's key and preimage
    /// from the wallet seed. The recovery handle: given the seed and this index
    /// (or by scanning indices), the swap's secrets can be reconstructed.
    pub swap_index: u64,
    /// For submarine swaps: the amount to lock onchain. For reverse swaps: the
    /// Lightning invoice amount.
    pub amount_sat: u64,
    /// For reverse swaps: the onchain amount that will be received.
    pub onchain_amount_sat: Option<u64>,
    /// Lightning invoice associated with the swap (the hold invoice for reverse
    /// swaps, the invoice Boltz pays for submarine swaps).
    pub invoice: Option<String>,
    /// Onchain lockup address.
    pub lockup_address: Option<String>,
    /// The address funds are claimed to (reverse) or refunded to (submarine).
    pub onchain_address: Option<String>,
    pub timeout_block_height: u64,
    pub created_at: u64,
    /// Txid of the claim transaction once broadcast (reverse swaps).
    pub claim_tx_id: Option<String>,
    /// Txid of the refund transaction once broadcast (submarine swaps).
    pub refund_tx_id: Option<String>,
}

/// Events emitted to a registered [`crate::modules::boltz::BoltzEventListener`]
/// as swaps progress through their lifecycle.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum BoltzSwapEvent {
    /// The swap transitioned to a new status.
    StatusUpdate {
        swap_id: String,
        status: BoltzSwapStatus,
    },
    /// A reverse swap was claimed onchain. `txid` is the claim transaction.
    Claimed { swap_id: String, txid: String },
    /// A submarine swap was refunded onchain. `txid` is the refund transaction.
    Refunded { swap_id: String, txid: String },
    /// An error occurred while processing the swap (e.g. an auto-claim failed).
    Error { swap_id: String, message: String },
}
