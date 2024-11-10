use crate::modules::scanner::NetworkType;

#[derive(uniffi::Enum, Debug, PartialEq)]
pub enum AddressType {
    P2PKH,    // Legacy
    P2SH,     // SegWit
    P2WPKH,   // Native SegWit
    P2WSH,    // Native SegWit Script
    P2TR,     // Taproot
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