mod errors;
mod implementation;
mod types;

pub use errors::{AccountInfoError, AddressError, BroadcastError, SweepError};
pub use implementation::{
    broadcast_raw_tx, build_descriptors, derive_base_path, detect_account_type,
    detect_network_from_key, get_account_info, get_address_info, normalize_extended_key,
    BitcoinAddressValidator,
};
pub use types::{
    AccountAddresses, AccountInfoResult, AccountType, AccountUtxo, AddressInfo,
    AddressType, ComposeAccount, GetAddressResponse, GetAddressesResponse, Network,
    SingleAddressInfoResult, SweepResult, SweepTransactionPreview, SweepableBalances,
    ValidationResult, WordCount,
};

#[cfg(test)]
mod tests;
