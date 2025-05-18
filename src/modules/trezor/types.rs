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

/// Script type for inputs and outputs
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
pub enum ScriptType {
    #[serde(rename = "SPENDADDRESS")]
    SpendAddress,
    #[serde(rename = "SPENDMULTISIG")]
    SpendMultisig,
    #[serde(rename = "SPENDWITNESS")]
    SpendWitness,
    #[serde(rename = "SPENDP2SHWITNESS")]
    SpendP2SHWitness,
    #[serde(rename = "SPENDTAPROOT")]
    SpendTaproot,
    #[serde(rename = "EXTERNAL")]
    External,
    #[serde(rename = "PAYTOADDRESS")]
    PayToAddress,
    #[serde(rename = "PAYTOSCRIPTHASH")]
    PayToScriptHash,
    #[serde(rename = "PAYTOMULTISIG")]
    PayToMultisig,
    #[serde(rename = "PAYTOWITNESS")]
    PayToWitness,
    #[serde(rename = "PAYTOP2SHWITNESS")]
    PayToP2SHWitness,
    #[serde(rename = "PAYTOTAPROOT")]
    PayToTaproot,
    #[serde(rename = "PAYTOOPRETURN")]
    PayToOpReturn,
}

/// Transaction input type
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct TxInputType {
    /// Previous transaction hash
    pub prev_hash: String,
    /// Previous transaction output index
    pub prev_index: u32,
    /// Amount in satoshis
    pub amount: u64,
    /// Transaction sequence
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u32>,
    /// BIP32 derivation path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_n: Option<Vec<u32>>,
    /// Script type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_type: Option<ScriptType>,
    /// Multisig information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multisig: Option<MultisigRedeemScriptType>,
    /// Script public key (for external inputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_pubkey: Option<String>,
    /// Script signature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_sig: Option<String>,
    /// Witness data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness: Option<String>,
    /// Ownership proof
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership_proof: Option<String>,
    /// Commitment data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commitment_data: Option<String>,
    /// Original hash for RBF
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_hash: Option<String>,
    /// Original index for RBF
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_index: Option<u32>,
    /// Coinjoin flags
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coinjoin_flags: Option<u32>,
}

/// Transaction output type
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct TxOutputType {
    /// Output address (for address outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// BIP32 derivation path (for change outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_n: Option<Vec<u32>>,
    /// Amount in satoshis
    pub amount: u64,
    /// Script type
    pub script_type: ScriptType,
    /// Multisig information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multisig: Option<MultisigRedeemScriptType>,
    /// OP_RETURN data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_return_data: Option<String>,
    /// Original hash for RBF
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_hash: Option<String>,
    /// Original index for RBF
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_index: Option<u32>,
    /// Payment request index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_req_index: Option<u32>,
}

/// Reference transaction for transaction signing
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct RefTransaction {
    /// Transaction hash
    pub hash: String,
    /// Transaction version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Transaction inputs
    pub inputs: Vec<RefTxInput>,
    /// Transaction outputs (binary format)
    pub bin_outputs: Vec<RefTxOutput>,
    /// Lock time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_time: Option<u32>,
    /// Expiry (for Zcash/Decred)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u32>,
    /// Version group ID (for Zcash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_group_id: Option<u32>,
    /// Overwintered flag (for Zcash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwintered: Option<bool>,
    /// Timestamp (for Capricoin)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u32>,
    /// Branch ID (for Zcash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_id: Option<u32>,
    /// Extra data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_data: Option<String>,
}

/// Reference transaction input
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct RefTxInput {
    /// Previous transaction hash
    pub prev_hash: String,
    /// Previous transaction output index
    pub prev_index: u32,
    /// Script signature
    pub script_sig: String,
    /// Sequence number
    pub sequence: u32,
}

/// Reference transaction output (binary format)
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct RefTxOutput {
    /// Amount in satoshis
    pub amount: u64,
    /// Script public key (binary hex)
    pub script_pubkey: String,
}

/// Amount unit for display
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
pub enum AmountUnit {
    #[serde(rename = "BITCOIN")]
    Bitcoin,
    #[serde(rename = "MILLIBITCOIN")]
    MilliBitcoin,
    #[serde(rename = "MICROBITCOIN")]
    MicroBitcoin,
    #[serde(rename = "SATOSHI")]
    Satoshi,
}

/// Payment request memo types
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct PaymentRequestMemo {
    /// Text memo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_memo: Option<TextMemo>,
    /// Refund memo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refund_memo: Option<RefundMemo>,
    /// Coin purchase memo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin_purchase_memo: Option<CoinPurchaseMemo>,
}

/// Text memo
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct TextMemo {
    /// Text content
    pub text: String,
}

/// Refund memo
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct RefundMemo {
    /// Refund address
    pub address: String,
    /// MAC
    pub mac: String,
}

/// Coin purchase memo
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct CoinPurchaseMemo {
    /// Coin type
    pub coin_type: u32,
    /// Amount
    pub amount: u64,
    /// Address
    pub address: String,
    /// MAC
    pub mac: String,
}

/// Payment request
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct TxAckPaymentRequest {
    /// Nonce
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Recipient name
    pub recipient_name: String,
    /// Memos
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memos: Option<Vec<PaymentRequestMemo>>,
    /// Amount
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    /// Signature
    pub signature: String,
}

/// Output type for compose transaction
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Enum)]
#[serde(tag = "type")]
pub enum ComposeOutput {
    /// Regular output with amount and address
    #[serde(rename = "regular")]
    Regular {
        /// Amount in satoshis
        amount: String,
        /// Recipient address
        address: String,
    },
    /// Send max output
    #[serde(rename = "send-max")]
    SendMax {
        /// Recipient address
        address: String,
    },
    /// OP_RETURN output
    #[serde(rename = "opreturn")]
    OpReturn {
        /// Hexadecimal string with arbitrary data
        #[serde(rename = "dataHex")]
        data_hex: String,
    },
    /// Payment without address (precompose only)
    #[serde(rename = "payment-noaddress")]
    PaymentNoAddress {
        /// Amount in satoshis
        amount: String,
    },
    /// Send max without address (precompose only)
    #[serde(rename = "send-max-noaddress")]
    SendMaxNoAddress,
}

/// UTXO information for account
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct AccountUtxo {
    /// Transaction ID
    pub txid: String,
    /// Output index
    pub vout: u32,
    /// Amount in satoshis
    pub amount: String,
    /// Block height
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_height: Option<u32>,
    /// Address
    pub address: String,
    /// Derivation path
    pub path: String,
    /// Number of confirmations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u32>,
}

/// Address information
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct AddressInfo {
    /// Address string
    pub address: String,
    /// Derivation path
    pub path: String,
    /// Number of transfers
    pub transfers: u32,
}

/// Account addresses
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct AccountAddresses {
    /// Used addresses
    pub used: Vec<AddressInfo>,
    /// Unused addresses
    pub unused: Vec<AddressInfo>,
    /// Change addresses
    pub change: Vec<AddressInfo>,
}

/// Account information for compose transaction
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct ComposeAccount {
    /// Derivation path
    pub path: String,
    /// Account addresses
    pub addresses: AccountAddresses,
    /// UTXOs
    pub utxo: Vec<AccountUtxo>,
}

/// Fee level for compose transaction
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct FeeLevel {
    /// Fee per unit (satoshi/byte or satoshi/vbyte)
    #[serde(rename = "feePerUnit")]
    pub fee_per_unit: String,
    /// Base fee in satoshi (optional, used in RBF and DOGE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_fee: Option<u32>,
    /// Floor base fee (optional, used in DOGE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor_base_fee: Option<bool>,
}

/// Parameters for composeTransaction method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ComposeTransactionParams {
    /// Array of output objects
    pub outputs: Vec<ComposeOutput>,
    /// Coin name/type (required)
    pub coin: String,
    /// Push transaction to blockchain (payment mode only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<bool>,
    /// Transaction sequence (for RBF or locktime)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u32>,
    /// Account info (precompose mode only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<ComposeAccount>,
    /// Fee levels (precompose mode only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_levels: Option<Vec<FeeLevel>>,
    /// Skip input/output permutation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_permutation: Option<bool>,
    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
}

/// Precomposed transaction input
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct PrecomposedInput {
    /// BIP32 derivation path
    pub address_n: Vec<u32>,
    /// Amount in satoshis
    pub amount: String,
    /// Previous transaction hash
    pub prev_hash: String,
    /// Previous output index
    pub prev_index: u32,
    /// Script type
    pub script_type: ScriptType,
}

/// Precomposed transaction output
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct PrecomposedOutput {
    /// BIP32 derivation path (for change outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_n: Option<Vec<u32>>,
    /// Amount in satoshis
    pub amount: String,
    /// Address (for regular outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Script type
    pub script_type: ScriptType,
}

/// Precomposed transaction
#[derive(Serialize, Deserialize, Debug, Clone, uniffi::Record)]
pub struct PrecomposedTransaction {
    /// Transaction type (usually "final" or "error")
    #[serde(rename = "type")]
    pub tx_type: String,
    /// Total amount spent (including fee)
    #[serde(rename = "totalSpent", skip_serializing_if = "Option::is_none")]
    pub total_spent: Option<String>,
    /// Transaction fee
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<String>,
    /// Fee per byte
    #[serde(rename = "feePerByte", skip_serializing_if = "Option::is_none")]
    pub fee_per_byte: Option<String>,
    /// Transaction size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u32>,
    /// Transaction inputs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<PrecomposedInput>>,
    /// Transaction outputs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<PrecomposedOutput>>,
    /// Output permutation indices
    #[serde(rename = "outputsPermutation", skip_serializing_if = "Option::is_none")]
    pub outputs_permutation: Option<Vec<u32>>,
}

/// Compose transaction response
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Enum)]
#[serde(untagged)]
pub enum ComposeTransactionResponse {
    /// Signed transaction (payment mode)
    SignedTransaction(SignedTransactionResponse),
    /// Precomposed transactions (precompose mode)
    PrecomposedTransactions(Vec<PrecomposedTransaction>),
}

/// Parameters for verifyMessage method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VerifyMessageParams {
    /// Signer address
    pub address: String,
    /// Signature in base64 format
    pub signature: String,
    /// Signed message
    pub message: String,
    /// Coin name/type (required)
    pub coin: String,
    /// Convert message from hex
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex: Option<bool>,
    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
}

/// Parameters for signMessage method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignMessageParams {
    /// BIP-32 path as string or array of numbers
    pub path: String,
    /// Coin name/type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    /// Message to sign
    pub message: String,
    /// Convert message from hex
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex: Option<bool>,
    /// No script type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_script_type: Option<bool>,
    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
}

/// Parameters for signTransaction method
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignTransactionParams {
    /// Coin name/type
    pub coin: String,
    /// Transaction inputs
    pub inputs: Vec<TxInputType>,
    /// Transaction outputs
    pub outputs: Vec<TxOutputType>,
    /// Reference transactions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refTxs: Option<Vec<RefTransaction>>,
    /// Payment requests (SLIP-24)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paymentRequests: Option<Vec<TxAckPaymentRequest>>,
    /// Lock time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locktime: Option<u32>,
    /// Transaction version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Expiry (for Zcash/Decred)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u32>,
    /// Version group ID (for Zcash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versionGroupId: Option<u32>,
    /// Overwintered flag (for Zcash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwintered: Option<bool>,
    /// Timestamp (for Capricoin)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u32>,
    /// Branch ID (for Zcash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branchId: Option<u32>,
    /// Broadcast transaction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<bool>,
    /// Amount unit for display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amountUnit: Option<AmountUnit>,
    /// Unlock path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlockPath: Option<UnlockPath>,
    /// Serialize full transaction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serialize: Option<bool>,
    /// Display address in chunks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunkify: Option<bool>,
    /// Additional common parameters
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub common: Option<CommonParams>,
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

    /// Response from composeTransaction method
    ComposeTransaction(ComposeTransactionResponse),

    /// Response from verifyMessage method
    VerifyMessage(VerifyMessageResponse),

    /// Response from signMessage method
    MessageSignature(MessageSignatureResponse),

    /// Response from signTransaction method
    SignedTransaction(SignedTransactionResponse),
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

/// Verify message response
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct VerifyMessageResponse {
    /// Verification result message
    pub message: String,
}

/// Message signature response
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct MessageSignatureResponse {
    /// Signer address
    pub address: String,
    /// Signature in base64 format
    pub signature: String,
}

/// Signed transaction response
#[derive(Debug, Clone, Deserialize, Serialize, uniffi::Record)]
pub struct SignedTransactionResponse {
    /// Array of signer signatures
    pub signatures: Vec<String>,
    /// Serialized transaction
    pub serializedTx: String,
    /// Broadcasted transaction ID (if push was true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,
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