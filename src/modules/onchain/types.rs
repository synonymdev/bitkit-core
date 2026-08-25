use crate::modules::activity::{Activity, TransactionDetails};
use crate::modules::scanner::NetworkType;
use bitcoin::Network as BitcoinNetwork;
use bitcoin_address_generator::{
    GetAddressResponse as ExternalGetAddressResponse,
    GetAddressesResponse as ExternalGetAddressesResponse, WordCount as ExternalWordCount,
};
use serde::{Deserialize, Serialize};
use uniffi::{Enum, Record};

/// Default address gap limit: the number of consecutive unused addresses to scan
/// past the last used one before concluding a wallet branch is exhausted.
///
/// 20 is the BIP44 standard and also BDK's hardcoded Electrum stop-gap floor, so
/// this is the smallest value the watcher will effectively use. Exposed so callers
/// (and the mobile bindings) reference one source of truth instead of hardcoding 20.
pub const DEFAULT_GAP_LIMIT: u32 = 20;

#[derive(Debug, Clone, Copy, Enum)]
pub enum WordCount {
    /// 12-word mnemonic (128 bits of entropy)
    Words12 = 12,
    /// 15-word mnemonic (160 bits of entropy)
    Words15 = 15,
    /// 18-word mnemonic (192 bits of entropy)
    Words18 = 18,
    /// 21-word mnemonic (224 bits of entropy)
    Words21 = 21,
    /// 24-word mnemonic (256 bits of entropy)
    Words24 = 24,
}

// For GetAddressResponse struct
#[derive(Debug, Serialize, Deserialize, Clone, Record)] // Added Record trait
pub struct GetAddressResponse {
    /// The generated Bitcoin address as a string
    pub address: String,
    /// The derivation path used to generate the address
    pub path: String,
    /// The hexadecimal representation of the public key
    pub public_key: String,
}

// For GetAddressesResponse struct
#[derive(Debug, Serialize, Deserialize, Clone, Record)] // Added Record trait
pub struct GetAddressesResponse {
    /// Vector of generated Bitcoin addresses
    pub addresses: Vec<GetAddressResponse>,
}

impl From<ExternalWordCount> for WordCount {
    fn from(word_count: ExternalWordCount) -> Self {
        match word_count {
            ExternalWordCount::Words12 => WordCount::Words12,
            ExternalWordCount::Words15 => WordCount::Words15,
            ExternalWordCount::Words18 => WordCount::Words18,
            ExternalWordCount::Words21 => WordCount::Words21,
            ExternalWordCount::Words24 => WordCount::Words24,
        }
    }
}

impl From<WordCount> for ExternalWordCount {
    fn from(word_count: WordCount) -> Self {
        match word_count {
            WordCount::Words12 => ExternalWordCount::Words12,
            WordCount::Words15 => ExternalWordCount::Words15,
            WordCount::Words18 => ExternalWordCount::Words18,
            WordCount::Words21 => ExternalWordCount::Words21,
            WordCount::Words24 => ExternalWordCount::Words24,
        }
    }
}

impl From<ExternalGetAddressResponse> for GetAddressResponse {
    fn from(response: ExternalGetAddressResponse) -> Self {
        Self {
            address: response.address,
            path: response.path,
            public_key: response.public_key,
        }
    }
}

impl From<ExternalGetAddressesResponse> for GetAddressesResponse {
    fn from(response: ExternalGetAddressesResponse) -> Self {
        Self {
            addresses: response
                .addresses
                .into_iter()
                .map(|addr| addr.into())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Enum)]
pub enum Network {
    /// Mainnet Bitcoin.
    Bitcoin,
    /// Bitcoin's testnet network.
    Testnet,
    /// Bitcoin's testnet4 network.
    Testnet4,
    /// Bitcoin's signet network.
    Signet,
    /// Bitcoin's regtest network.
    Regtest,
}

impl From<BitcoinNetwork> for Network {
    fn from(network: BitcoinNetwork) -> Self {
        match network {
            BitcoinNetwork::Bitcoin => Network::Bitcoin,
            BitcoinNetwork::Testnet => Network::Testnet,
            BitcoinNetwork::Testnet4 => Network::Testnet4,
            BitcoinNetwork::Signet => Network::Signet,
            BitcoinNetwork::Regtest => Network::Regtest,
        }
    }
}

impl From<Network> for BitcoinNetwork {
    fn from(network: Network) -> Self {
        match network {
            Network::Bitcoin => BitcoinNetwork::Bitcoin,
            Network::Testnet => BitcoinNetwork::Testnet,
            Network::Testnet4 => BitcoinNetwork::Testnet4,
            Network::Signet => BitcoinNetwork::Signet,
            Network::Regtest => BitcoinNetwork::Regtest,
        }
    }
}

#[derive(uniffi::Enum, Debug, PartialEq)]
pub enum AddressType {
    P2PKH,  // Legacy
    P2SH,   // SegWit
    P2WPKH, // Native SegWit
    P2WSH,  // Native SegWit Script
    P2TR,   // Taproot
    Unknown,
}

impl AddressType {
    pub fn common_name(&self) -> &'static str {
        match self {
            AddressType::P2PKH => "Legacy",
            AddressType::P2SH => "SegWit",
            AddressType::P2WPKH => "Native SegWit",
            AddressType::P2WSH => "Native SegWit Script",
            AddressType::P2TR => "Taproot",
            AddressType::Unknown => "Unknown",
        }
    }
}

#[derive(uniffi::Record, Debug)]
pub struct ValidationResult {
    pub address: String,
    pub network: NetworkType,
    pub address_type: AddressType,
}

#[derive(uniffi::Record, Debug)]
pub struct SweepResult {
    /// The transaction ID of the sweep transaction
    pub txid: String,
    /// The total amount swept (in satoshis)
    pub amount_swept: u64,
    /// The fee paid (in satoshis)
    pub fee_paid: u64,
    /// The number of UTXOs swept
    pub utxos_swept: u32,
}

#[derive(uniffi::Record, Debug)]
pub struct SweepTransactionPreview {
    /// The PSBT (Partially Signed Bitcoin Transaction) in base64 format
    pub psbt: String,
    /// The total amount available to sweep (in satoshis)
    pub total_amount: u64,
    /// The estimated fee for the transaction (in satoshis)
    pub estimated_fee: u64,
    /// The estimated virtual size of the transaction (in vbytes)
    pub estimated_vsize: u64,
    /// The number of UTXOs that will be swept
    pub utxos_count: u32,
    /// The destination address
    pub destination_address: String,
    /// The amount that will be sent to destination after fees (in satoshis)
    pub amount_after_fees: u64,
}

#[derive(uniffi::Record, Debug)]
pub struct SweepableBalances {
    /// Balance in legacy (P2PKH) addresses (in satoshis)
    pub legacy_balance: u64,
    /// Balance in P2SH-SegWit (P2SH-P2WPKH) addresses (in satoshis)
    pub p2sh_balance: u64,
    /// Balance in Taproot (P2TR) addresses (in satoshis)
    pub taproot_balance: u64,
    /// Total balance across all wallet types (in satoshis)
    pub total_balance: u64,
    /// Number of UTXOs in legacy wallet
    pub legacy_utxos_count: u32,
    /// Number of UTXOs in P2SH-SegWit wallet
    pub p2sh_utxos_count: u32,
    /// Number of UTXOs in Taproot wallet
    pub taproot_utxos_count: u32,
    /// Total number of UTXOs across all wallet types
    pub total_utxos_count: u32,
}

// ============================================================================
// Legacy RN close recovery types
// ============================================================================

#[derive(uniffi::Record, Debug, Clone)]
pub struct LegacyRnCloseRecoveryScanResult {
    /// Total balance found in legacy RN P2WPKH close outputs (in satoshis).
    pub total_amount: u64,
    /// Number of P2WPKH outputs found.
    pub outputs_count: u32,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LegacyRnCloseRecoverySweepPreview {
    /// Fully signed raw sweep transaction hex. Broadcast only after user confirmation.
    pub tx_hex: String,
    /// Transaction id of the sweep transaction.
    pub txid: String,
    /// Total input amount in satoshis.
    pub total_amount: u64,
    /// Fee in satoshis.
    pub estimated_fee: u64,
    /// Transaction virtual size in vbytes.
    pub estimated_vsize: u64,
    /// Number of recovered outputs swept.
    pub outputs_count: u32,
    /// Destination address receiving the sweep.
    pub destination_address: String,
    /// Amount sent to destination after fees.
    pub amount_after_fees: u64,
}

// ============================================================================
// Account info types
// ============================================================================

/// Account type classification for extended public keys.
///
/// Determines the BIP standard, derivation path purpose, and script type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccountType {
    /// BIP44 legacy (P2PKH) — xpub/tpub prefix
    Legacy,
    /// BIP49 wrapped segwit (P2SH-P2WPKH) — ypub/upub prefix
    WrappedSegwit,
    /// BIP84 native segwit (P2WPKH) — zpub/vpub prefix
    NativeSegwit,
    /// BIP86 taproot (P2TR)
    Taproot,
}

/// A UTXO associated with an account or address.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountUtxo {
    /// Transaction ID (hex)
    pub txid: String,
    /// Output index
    pub vout: u32,
    /// Amount in satoshis
    pub amount: u64,
    /// Block height where the UTXO was confirmed (0 if unconfirmed)
    pub block_height: u32,
    /// Address holding this UTXO
    pub address: String,
    /// BIP32 derivation path (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
    /// Number of confirmations (0 if unconfirmed)
    pub confirmations: u32,
    /// Whether this is a coinbase output
    pub coinbase: bool,
    /// Whether this UTXO is owned by the account
    pub own: bool,
    /// Whether this UTXO must be included in the transaction
    pub required: Option<bool>,
}

/// Information about a single address in an account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AddressInfo {
    /// The Bitcoin address
    pub address: String,
    /// BIP32 derivation path
    pub path: String,
    /// Number of transfers (real count in `get_address_info`, 1/0 presence flag in `get_account_info`)
    pub transfers: u32,
}

/// Grouped address lists for an account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountAddresses {
    /// Used receive addresses (have at least one transaction)
    pub used: Vec<AddressInfo>,
    /// Unused receive addresses (no transactions yet)
    pub unused: Vec<AddressInfo>,
    /// Change addresses
    pub change: Vec<AddressInfo>,
}

/// Full account structure with addresses and UTXOs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ComposeAccount {
    /// Account derivation path (e.g., "m/84'/0'/0'")
    pub path: String,
    /// Categorized addresses
    pub addresses: AccountAddresses,
    /// Unspent transaction outputs
    pub utxo: Vec<AccountUtxo>,
}

/// Result from querying an extended public key via Electrum.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountInfoResult {
    /// The account structure with addresses and UTXOs
    pub account: ComposeAccount,
    /// Total confirmed balance in satoshis
    pub balance: u64,
    /// Number of UTXOs
    pub utxo_count: u32,
    /// The detected or specified account type
    pub account_type: AccountType,
    /// The current blockchain tip height
    pub block_height: u32,
}

/// Result from querying a single Bitcoin address.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SingleAddressInfoResult {
    /// The queried address
    pub address: String,
    /// Total confirmed balance in satoshis
    pub balance: u64,
    /// UTXOs for this address
    pub utxos: Vec<AccountUtxo>,
    /// Number of transactions involving this address
    pub transfers: u32,
    /// Current blockchain tip height
    pub block_height: u32,
}

// ============================================================================
// Shared wallet parameters
// ============================================================================

/// Common parameters for creating and syncing a watch-only BDK wallet.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WalletParams {
    /// Extended public key (xpub/ypub/zpub/tpub/upub/vpub)
    pub extended_key: String,
    /// Electrum server URL for wallet sync
    pub electrum_url: String,
    /// Root fingerprint hex (e.g. "73c5da0a"). Required for hardware wallet signing.
    pub fingerprint: Option<String>,
    /// Bitcoin network (auto-detected from key prefix if not specified)
    pub network: Option<Network>,
    /// Override account type for ambiguous key prefixes (xpub/tpub)
    pub account_type: Option<AccountType>,
}

// ============================================================================
// Transaction compose types (signer-agnostic)
// ============================================================================

/// Coin selection strategy for transaction composition.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum CoinSelection {
    /// Branch-and-bound (default). Minimizes change by searching for exact matches.
    BranchAndBound,
    /// Selects largest UTXOs first. Useful for UTXO consolidation.
    LargestFirst,
    /// Selects oldest UTXOs first. Maximizes coin-age spending.
    OldestFirst,
}

/// Output specification for transaction composition.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ComposeOutput {
    /// Payment to a specific address with a fixed amount (satoshis)
    Payment { address: String, amount_sats: u64 },
    /// Send all remaining funds (after fees) to an address
    SendMax { address: String },
    /// OP_RETURN data output (hex-encoded payload)
    OpReturn { data_hex: String },
}

/// Parameters for composing a signer-agnostic transaction.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ComposeParams {
    /// Wallet configuration (key, server, network)
    pub wallet: WalletParams,
    /// Desired transaction outputs
    pub outputs: Vec<ComposeOutput>,
    /// Fee rates to evaluate (sat/vB), one PSBT per rate
    pub fee_rates: Vec<f32>,
    /// UTXO selection strategy (defaults to BranchAndBound)
    pub coin_selection: Option<CoinSelection>,
}

/// Result of composing a transaction at a single fee rate.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ComposeResult {
    /// Successfully built a signable PSBT
    Success {
        /// Base64-encoded PSBT ready for signing
        psbt: String,
        /// Total fee in satoshis
        fee: u64,
        /// Target fee rate in sat/vB (actual may differ slightly due to rounding)
        fee_rate: f32,
        /// Total value spent (payments + fee, excluding change).
        /// Uses BDK's `sent - received` semantics, which may undercount for
        /// self-transfers where the destination is also owned by the wallet.
        total_spent: u64,
    },
    /// Composition failed (e.g. insufficient funds)
    Error { error: String },
}

/// A finalized transaction ready to broadcast.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CompletedTransaction {
    /// Consensus-encoded transaction hex.
    pub serialized_tx: String,
    /// Canonical transaction id, excluding witness data.
    pub txid: String,
}

// ============================================================================
// Transaction history types
// ============================================================================

/// Transaction direction from the wallet's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TxDirection {
    /// Wallet sent funds to an external address
    Sent,
    /// Wallet received funds from an external source
    Received,
    /// Wallet sent funds to itself (e.g. consolidation, change-only)
    SelfTransfer,
}

/// Classify a transaction by direction and compute the display amount.
///
/// Pure function for easy unit testing. Uses i128 intermediates to avoid
/// silent truncation when casting large u64 values to i64.
///
/// Returns `(direction, display_amount, net_value)`.
pub(crate) fn classify_tx(sent: u64, received: u64, fee: Option<u64>) -> (TxDirection, u64, i64) {
    let net = (received as i128 - sent as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64;

    let direction = if sent > 0 && received > 0 {
        match fee {
            Some(f) if sent.saturating_sub(received) <= f => TxDirection::SelfTransfer,
            _ => TxDirection::Sent,
        }
    } else if sent > 0 {
        TxDirection::Sent
    } else {
        TxDirection::Received
    };

    let amount = match direction {
        TxDirection::Received => received,
        TxDirection::Sent => sent
            .saturating_sub(received)
            .saturating_sub(fee.unwrap_or(0)),
        TxDirection::SelfTransfer => fee.unwrap_or(0),
    };

    (direction, amount, net)
}

/// A single transaction in the wallet's history.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HistoryTransaction {
    /// Transaction ID (hex)
    pub txid: String,
    /// Amount received by the wallet (sats)
    pub received: u64,
    /// Amount sent by the wallet (sats) — includes change sent back to self
    pub sent: u64,
    /// Net value from wallet's perspective: received - sent (positive = inflow, negative = outflow)
    pub net: i64,
    /// Transaction fee in sats (None if not available, e.g. for received-only txs)
    pub fee: Option<u64>,
    /// Fee rate in sats per virtual byte (None if fee or tx size is unavailable).
    pub fee_rate: Option<f64>,
    /// Display amount in sats:
    /// - Received: the received value
    /// - Sent: amount that left the wallet (sent - received - fee)
    /// - SelfTransfer: the fee paid
    pub amount: u64,
    /// Transaction direction
    pub direction: TxDirection,
    /// Block height (None if unconfirmed/mempool)
    pub block_height: Option<u32>,
    /// Block timestamp as unix epoch seconds (None if unconfirmed)
    pub timestamp: Option<u64>,
    /// Number of confirmations (0 if unconfirmed)
    pub confirmations: u32,
}

/// Balance breakdown from BDK.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WalletBalance {
    /// Confirmed and spendable balance (sats)
    pub confirmed: u64,
    /// Immature coinbase outputs (sats)
    pub immature: u64,
    /// Unconfirmed UTXOs from trusted sources (own change) (sats)
    pub trusted_pending: u64,
    /// Unconfirmed UTXOs from external sources (sats)
    pub untrusted_pending: u64,
    /// Total spendable: confirmed + trusted_pending (sats)
    pub spendable: u64,
    /// Grand total: all categories (sats)
    pub total: u64,
}

impl From<bdk::Balance> for WalletBalance {
    fn from(b: bdk::Balance) -> Self {
        Self {
            confirmed: b.confirmed,
            immature: b.immature,
            trusted_pending: b.trusted_pending,
            untrusted_pending: b.untrusted_pending,
            spendable: b.confirmed + b.trusted_pending,
            total: b.confirmed + b.immature + b.trusted_pending + b.untrusted_pending,
        }
    }
}

/// Events emitted by the onchain xpub watcher.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum WatcherEvent {
    /// Transaction activity changed — contains full updated state.
    ///
    /// `activities` and `transaction_details` are persistence-ready: they carry
    /// the watcher's `wallet_id`, real decoded addresses, fees from the watched
    /// wallet's perspective, and DB-valid timestamps, so the app can store them
    /// directly through the normal Core activity APIs (e.g. `upsert_activity` /
    /// `upsert_transaction_details`). The two vecs are parallel by `tx_id`.
    TransactionsChanged {
        activities: Vec<Activity>,
        transaction_details: Vec<TransactionDetails>,
        balance: WalletBalance,
        tx_count: u32,
        block_height: u32,
        account_type: AccountType,
    },
    /// An error occurred in the watcher loop.
    Error { message: String },
    /// Connection to the Electrum server was lost.
    Disconnected { message: String },
    /// Connection to the Electrum server was restored.
    Reconnected,
}

/// Result from querying transaction history for an xpub.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TransactionHistoryResult {
    /// All transactions, sorted: unconfirmed first, then by timestamp descending
    pub transactions: Vec<HistoryTransaction>,
    /// Balance breakdown
    pub balance: WalletBalance,
    /// Total number of transactions
    pub tx_count: u32,
    /// Current blockchain tip height
    pub block_height: u32,
    /// The detected or specified account type
    pub account_type: AccountType,
}

// ============================================================================
// Transaction detail types (single-tx endpoint)
// ============================================================================

/// A transaction input with full details.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TxDetailInput {
    /// Previous output transaction ID (hex)
    pub txid: String,
    /// Previous output index
    pub vout: u32,
    /// Sequence number
    pub sequence: u32,
    /// Script signature (hex-encoded)
    pub script_sig: String,
    /// Witness stack (each element hex-encoded)
    pub witness: Vec<String>,
}

/// A transaction output with full details.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TxDetailOutput {
    /// Output value in sats
    pub value: u64,
    /// Script public key (hex-encoded)
    pub script_pubkey: String,
    /// Decoded address (None if script is not decodable to an address)
    pub address: Option<String>,
    /// Whether this output belongs to the queried wallet
    pub is_mine: bool,
}

/// Full details for a single transaction, including raw inputs/outputs and size metrics.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TransactionDetail {
    /// Transaction ID (hex)
    pub txid: String,
    /// Amount received by the wallet (sats)
    pub received: u64,
    /// Amount sent by the wallet (sats) — includes change sent back to self
    pub sent: u64,
    /// Net value from wallet's perspective: received - sent (positive = inflow, negative = outflow)
    pub net: i64,
    /// Display amount in sats (same semantics as HistoryTransaction.amount)
    pub amount: u64,
    /// Transaction fee in sats (None if not available)
    pub fee: Option<u64>,
    /// Transaction direction
    pub direction: TxDirection,
    /// Block height (None if unconfirmed/mempool)
    pub block_height: Option<u32>,
    /// Block timestamp as unix epoch seconds (None if unconfirmed)
    pub timestamp: Option<u64>,
    /// Number of confirmations (0 if unconfirmed)
    pub confirmations: u32,
    /// Transaction inputs
    pub inputs: Vec<TxDetailInput>,
    /// Transaction outputs
    pub outputs: Vec<TxDetailOutput>,
    /// Serialized transaction size in bytes
    pub size: u32,
    /// Virtual size in vbytes (ceil(weight / 4))
    pub vsize: u32,
    /// Transaction weight in weight units
    pub weight: u32,
    /// Fee rate in sat/vB (fee / vsize), None if fee is unavailable or vsize is zero
    pub fee_rate: Option<f64>,
}
