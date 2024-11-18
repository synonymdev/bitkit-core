uniffi::setup_scaffolding!();

mod modules;

// Re-export commonly used types and functions
pub use modules::scanner::{Scanner, DecodingError};
pub use modules::lnurl;
pub use modules::onchain;
use crate::onchain::{AddressError, ValidationResult};

#[uniffi::export]
pub async fn decode(invoice: String) -> Result<Scanner, DecodingError> {
    Scanner::decode(invoice).await
}

#[uniffi::export]
pub async fn get_lnurl_invoice(address: String, amount_satoshis: u64) -> Result<String, lnurl::LnurlError> {
    lnurl::get_lnurl_invoice(&address, amount_satoshis).await
}

#[uniffi::export]
pub fn validate_bitcoin_address(address: String) -> Result<ValidationResult, AddressError> {
    onchain::BitcoinAddressValidator::validate_address(&address)
}