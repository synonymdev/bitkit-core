mod implementation;
mod types;
mod errors;

pub use implementation::BitcoinAddressValidator;
pub use types::{AddressType, ValidationResult, WordCount, GetAddressResponse, GetAddressesResponse, Network};
pub use errors::AddressError;

#[cfg(test)]
mod tests;