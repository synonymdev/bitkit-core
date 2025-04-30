use crate::modules::scanner::NetworkType;
use bitcoin_address_generator::{
    GetAddressResponse as ExternalGetAddressResponse,
    GetAddressesResponse as ExternalGetAddressesResponse,
    WordCount as ExternalWordCount
};
use bitcoin::Network as BitcoinNetwork;
use uniffi::{Enum, Record};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Serialize, Deserialize, Clone, Record)]  // Added Record trait
pub struct GetAddressResponse {
    /// The generated Bitcoin address as a string
    pub address: String,
    /// The derivation path used to generate the address
    pub path: String,
    /// The hexadecimal representation of the public key
    pub public_key: String,
}

// For GetAddressesResponse struct
#[derive(Debug, Serialize, Deserialize, Clone, Record)]  // Added Record trait
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
            addresses: response.addresses.into_iter().map(|addr| addr.into()).collect(),
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
            _ => Network::Bitcoin, // Default to Bitcoin mainnet
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