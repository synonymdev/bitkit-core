mod errors;
mod implementation;
mod types;

pub use errors::{AddressError, SweepError};
pub use implementation::BitcoinAddressValidator;
pub use types::{
    AddressType, GetAddressResponse, GetAddressesResponse, Network, SweepResult,
    SweepTransactionPreview, SweepableBalances, ValidationResult, WordCount,
};

#[cfg(test)]
mod tests;
