use serde::{Deserialize, Serialize};
use url::Url;
use crate::modules::trezor::TrezorConnectError;

/// Result type for deep link generation, including the URL and the ID used
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeepLinkResult {
    /// The generated deep link URL
    pub url: String,
    /// The request ID used (either provided or auto-generated)
    pub request_id: String,
}

/// Environment options for Trezor deep linking
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum TrezorEnvironment {
    /// Production environment (currently unavailable according to docs)
    Production,
    /// Development environment
    Development,
    /// Local environment
    Local,
}

/// Empty struct that serializes to {}
#[derive(Serialize)]
pub(crate) struct Empty;

/// Main client for Trezor Connect deep linking
#[derive(Debug, Clone)]
pub struct TrezorConnectClient {
    /// Environment to use for deep links
    pub(crate) environment: TrezorEnvironment,
    /// Base callback URL for Trezor to return results to (without the ID)
    pub(crate) callback_base: Url,
}


/// Common parameters for all Trezor Connect methods
#[derive(Serialize, Deserialize, Debug, Default, Clone, uniffi::Record)]
pub struct CommonParams {
    /// Specific device instance to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceParams>,
    /// Set to true if method should use empty passphrase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub useEmptyPassphrase: Option<bool>,
    /// Allow seedless device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowSeedlessDevice: Option<bool>,
    /// Skip final reload
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipFinalReload: Option<bool>,
}

/// Parameters for specifying a particular device
#[derive(Serialize, Deserialize, Debug, Default, Clone, uniffi::Record)]
pub struct DeviceParams {
    /// Device instance path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Device instance ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<u32>,
}

/// HD Node Type
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct HDNodeType {
    /// Depth
    pub depth: u32,
    /// Fingerprint
    pub fingerprint: u32,
    /// Child number
    pub child_num: u32,
    /// Chain code
    pub chain_code: String,
    /// Public key
    pub public_key: String,
    /// Private key (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// BIP32 derivation path (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_n: Option<Vec<u32>>,
}

/// HD Node Path Type
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct HDNodePathType {
    /// Node data (can be String or HDNodeType)
    #[serde(flatten)]
    pub node: HDNodeTypeOrString,
    /// BIP32 derivation path
    pub address_n: Vec<u32>,
}

/// Union type for HD Node (either a String or HDNodeType)
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
#[serde(untagged)]
pub enum HDNodeTypeOrString {
    /// HD Node as a string
    String(String),
    /// HD Node as an object
    Node(HDNodeType),
}

/// Multisig Redeem Script Type
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct MultisigRedeemScriptType {
    /// Public keys
    pub pubkeys: Vec<HDNodePathType>,
    /// Signatures
    pub signatures: Vec<String>,
    /// M-of-N threshold
    pub m: u32,
    /// Nodes (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<HDNodeType>>,
    /// Pubkeys order (optional): 0 for PRESERVED, 1 for LEXICOGRAPHIC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubkeys_order: Option<u8>,
}

/// Unlock Path parameters
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct UnlockPath {
    /// BIP32 derivation path
    pub address_n: Vec<u32>,
    /// MAC (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

/// Parameters for getAddress method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAddressParams {
    /// BIP-32 path as string or array of numbers
    #[serde(rename = "path")]
    pub path: String,

    /// Address for validation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,

    /// Show on display (optional, default is true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub showOnTrezor: Option<bool>,

    /// Display address in chunks of 4 characters (optional, default is false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunkify: Option<bool>,

    /// Use event listener (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub useEventListener: Option<bool>,

    /// Coin name/type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,

    /// Allow cross-chain address generation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossChain: Option<bool>,

    /// Multisig information (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multisig: Option<MultisigRedeemScriptType>,

    /// Script type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scriptType: Option<String>,

    /// Unlock path information (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlockPath: Option<UnlockPath>,

    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
}

/// Parameters for getPublicKey method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetPublicKeyParams {
    /// BIP-32 path as string
    pub path: String,

    /// Display the result on the Trezor device (optional, default is false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub showOnTrezor: Option<bool>,

    /// Suppress backup warning (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressBackupWarning: Option<bool>,

    /// Display the result in chunks for better readability (optional, default is false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunkify: Option<bool>,

    /// Coin name/type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,

    /// Allow cross-chain key generation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossChain: Option<bool>,

    /// Script type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scriptType: Option<String>,

    /// Ignore SLIP-0132 XPUB magic (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignoreXpubMagic: Option<bool>,

    /// ECDSA curve name to use (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ecdsaCurveName: Option<String>,

    /// Unlock path information (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlockPath: Option<UnlockPath>,

    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
}

/// Level of details to be returned by getAccountInfo
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
pub enum AccountInfoDetails {
    /// Return only account balances (default)
    #[serde(rename = "basic")]
    Basic,
    /// Return with derived addresses or ERC20 tokens
    #[serde(rename = "tokens")]
    Tokens,
    /// Same as tokens with balances
    #[serde(rename = "tokenBalances")]
    TokenBalances,
    /// TokenBalances + complete account transaction history
    #[serde(rename = "txs")]
    Txs,
}

/// Token filter options for getAccountInfo
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
pub enum TokenFilter {
    /// Return only addresses with nonzero balance (default)
    #[serde(rename = "nonzero")]
    Nonzero,
    /// Return addresses with at least one transaction
    #[serde(rename = "used")]
    Used,
    /// Return all derived addresses
    #[serde(rename = "derived")]
    Derived,
}

/// Bitcoin account types for default display
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
pub enum DefaultAccountType {
    /// Normal account
    #[serde(rename = "normal")]
    Normal,
    /// SegWit account
    #[serde(rename = "segwit")]
    Segwit,
    /// Legacy account
    #[serde(rename = "legacy")]
    Legacy,
}

/// Marker object for XRP accounts
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct XrpMarker {
    /// Ledger number
    pub ledger: u64,
    /// Sequence number
    pub seq: u64,
}

/// Parameters for getAccountInfo method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAccountInfoParams {
    // Path-based parameters
    /// BIP-32 path as string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    // Descriptor-based parameters
    /// Public key or address of account
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<String>,

    // Required for all modes
    /// Coin name/type
    pub coin: String,

    // Optional parameters
    /// Level of details returned by request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<AccountInfoDetails>,

    /// Specifies which tokens (xpub addresses) are returned
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenFilter>,

    /// Transaction history page index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,

    /// Transaction history page size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pageSize: Option<u32>,

    /// Transaction history from block filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u32>,

    /// Transaction history to block filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<u32>,

    /// Address derivation gap size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<u32>,

    /// Ethereum-like accounts only: get ERC20 token info and balance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contractFilter: Option<String>,

    /// XRP accounts only, transaction history page marker
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<XrpMarker>,

    /// Bitcoin-like accounts only: specify which account group is displayed as default in popup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaultAccountType: Option<DefaultAccountType>,

    /// Suppress backup warning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressBackupWarning: Option<bool>,

    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
}


/// Type alias for TrezorConnectResult
pub type TrezorConnectResult<T> = Result<T, TrezorConnectError>;

/// Enum representing the different types of Trezor responses
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Enum)]
#[serde(untagged)]
pub enum TrezorResponsePayload {
    /// Response from getFeatures method
    Features(FeatureResponse),

    /// Response from getAddress method
    Address(AddressResponse),

    /// Response from getPublicKey method
    PublicKey(PublicKeyResponse),

    /// Response from getAccountInfo method
    AccountInfo(AccountInfoResponse),
}

/// Feature response containing device capabilities and information
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct FeatureResponse {
    pub vendor: String,
    pub major_version: u32,
    pub minor_version: u32,
    pub patch_version: u32,
    pub device_id: String,
    // Add other fields as needed from the getFeatures response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    // Add other optional fields as needed
}

/// Address response containing the derived address information
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct AddressResponse {
    pub address: String,
    pub path: Vec<u32>,
    pub serializedPath: String,
    // Add other fields as needed
}

/// Public key response containing the derived public key information
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct PublicKeyResponse {
    pub path: Vec<u32>,
    pub serializedPath: String,
    pub xpub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xpubSegwit: Option<String>,
    pub chainCode: String,
    pub childNum: u32,
    pub publicKey: String,
    pub fingerprint: u32,
    pub depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<String>,
}

/// Account info response
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct AccountInfoResponse {
    pub id: u32,
    pub path: String,
    pub descriptor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacyXpub: Option<String>,
    pub balance: String,
    pub availableBalance: String,
    // Add other fields as needed
}

/// Common response wrapper for all Trezor responses
#[derive(Debug, Clone, Deserialize)]
pub struct TrezorResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}