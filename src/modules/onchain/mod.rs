mod compose;
mod errors;
mod extended_pubkey;
mod implementation;
mod listener;
mod types;

pub use compose::compose_transaction;
pub use errors::{AccountInfoError, AddressError, BroadcastError, OnchainError, SweepError};
pub use extended_pubkey::serialized_extended_pubkey;
pub use implementation::{
    broadcast_raw_tx, build_descriptors, derive_base_path, detect_account_type,
    detect_network_from_key, get_account_info, get_address_info, get_transaction_detail,
    get_transaction_history, normalize_extended_key, BitcoinAddressValidator,
};
pub use listener::{start_watcher, stop_all_watchers, stop_watcher, EventListener, WatcherParams};
pub use types::{
    AccountAddresses, AccountInfoResult, AccountType, AccountUtxo, AddressInfo, AddressType,
    CoinSelection, ComposeAccount, ComposeOutput, ComposeParams, ComposeResult, GetAddressResponse,
    GetAddressesResponse, HistoryTransaction, LegacyRnCloseRecoveryScanResult,
    LegacyRnCloseRecoverySweepPreview, Network, SingleAddressInfoResult, SweepResult,
    SweepTransactionPreview, SweepableBalances, TransactionDetail, TransactionHistoryResult,
    TxDetailInput, TxDetailOutput, TxDirection, ValidationResult, WalletBalance, WalletParams,
    WatcherEvent, WordCount, DEFAULT_GAP_LIMIT,
};

#[cfg(test)]
mod tests;
