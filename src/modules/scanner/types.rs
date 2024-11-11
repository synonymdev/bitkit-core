use std::collections::HashMap;
use std::fmt;
use bitcoin::Network;
use serde::Serialize;

#[derive(uniffi::Enum, Debug, Clone, PartialEq, Serialize)]
pub enum NetworkType {
    Bitcoin,
    Testnet,
    Regtest,
    Signet,
}

// Implement Display for NetworkType
impl fmt::Display for NetworkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkType::Bitcoin => write!(f, "bitcoin"),
            NetworkType::Testnet => write!(f, "testnet"),
            NetworkType::Regtest => write!(f, "regtest"),
            NetworkType::Signet => write!(f, "signet"),
        }
    }
}
impl From<Network> for NetworkType {
    fn from(network: Network) -> Self {
        match network {
            Network::Bitcoin => NetworkType::Bitcoin,
            Network::Testnet => NetworkType::Testnet,
            Network::Regtest => NetworkType::Regtest,
            Network::Signet => NetworkType::Signet,
            _ => NetworkType::Bitcoin,
        }
    }
}

impl From<NetworkType> for Network {
    fn from(network_type: NetworkType) -> Self {
        match network_type {
            NetworkType::Bitcoin => Network::Bitcoin,
            NetworkType::Testnet => Network::Testnet,
            NetworkType::Regtest => Network::Regtest,
            NetworkType::Signet => Network::Signet,
        }
    }
}

#[derive(uniffi::Enum, Serialize, Debug, Clone)]
pub enum Unit {
    Bitcoin,
    Satoshi,
    MilliSatoshi,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LnurlChannelData {
    pub uri: String,
    pub callback: String,
    pub k1: String,
    pub tag: String,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LnurlAuthData {
    pub uri: String,
    pub tag: String,
    pub k1: String,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LnurlWithdrawData {
    pub uri: String,
    pub callback: String,
    pub k1: String,
    pub default_description: String,
    pub min_withdrawable: Option<u64>,
    pub max_withdrawable: u64,
    pub tag: String,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LnurlPayData {
    pub uri: String,
    pub callback: String,
    pub min_sendable: u64,
    pub max_sendable: u64,
    pub metadata_str: String,
    pub comment_allowed: Option<u32>,
    pub allows_nostr: bool,
    pub nostr_pubkey: Option<Vec<u8>>,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LnurlAddressData {
    pub uri: String,
    pub domain: String,
    pub username: String,
}

#[derive(uniffi::Record, Serialize, Debug, Clone)]
pub struct PubkyAuth {
    pub data: String,
}

#[derive(uniffi::Record, Serialize, Debug, Clone)]
pub struct OnChainInvoice {
    pub address: String,
    pub amount_satoshis: u64,
    pub label: Option<String>,
    pub message: Option<String>,
    pub params: Option<HashMap<String, String>>,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct LightningInvoice {
    pub payment_hash: Vec<u8>,
    pub amount_satoshis: u64,
    pub timestamp_seconds: u64,
    pub expiry_seconds: u64,
    pub is_expired: bool,
    pub description: Option<String>,
    pub network_type: NetworkType,
    pub payee_node_id: Option<Vec<u8>>,
}

#[derive(uniffi::Enum, Debug, Clone)]
pub enum Scanner {
    OnChain { invoice: OnChainInvoice },
    Lightning { invoice: LightningInvoice },
    PubkyAuth { data: String },
    LnurlChannel { data: LnurlChannelData },
    LnurlAuth { data: LnurlAuthData },
    LnurlWithdraw { data: LnurlWithdrawData },
    LnurlAddress { data: LnurlAddressData },
    LnurlPay { data: LnurlPayData },
    NodeId { url: String, network: NetworkType },
    TreasureHunt { chest_id: String },
    OrangeTicket { ticket_id: String },
}