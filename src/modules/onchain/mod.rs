mod implementation;
mod types;
mod errors;

pub use implementation::BitcoinAddressValidator;
pub use types::{AddressType, ValidationResult};
pub use errors::AddressError;

#[cfg(test)]
mod tests;