//! FFI-compatible types for the Trezor module.


/// Transport type for Trezor devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrezorTransportType {
    /// USB connection
    Usb,
    /// Bluetooth connection
    Bluetooth,
}

impl From<trezor_connect_rs::TransportType> for TrezorTransportType {
    fn from(t: trezor_connect_rs::TransportType) -> Self {
        match t {
            trezor_connect_rs::TransportType::Usb => TrezorTransportType::Usb,
            trezor_connect_rs::TransportType::Bluetooth => TrezorTransportType::Bluetooth,
        }
    }
}

/// Device information exposed to FFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorDeviceInfo {
    /// Unique identifier for the device
    pub id: String,
    /// Transport type (USB or Bluetooth)
    pub transport_type: TrezorTransportType,
    /// Device name (from BLE advertisement or USB descriptor)
    pub name: Option<String>,
    /// Transport-specific path (used internally for connection)
    pub path: String,
    /// Device label (set by user during device setup)
    pub label: Option<String>,
    /// Device model (e.g., "T2", "Safe 5", "Safe 7")
    pub model: Option<String>,
    /// Whether the device is in bootloader mode
    pub is_bootloader: bool,
}

impl From<trezor_connect_rs::DeviceInfo> for TrezorDeviceInfo {
    fn from(info: trezor_connect_rs::DeviceInfo) -> Self {
        Self {
            id: info.id,
            transport_type: info.transport_type.into(),
            name: info.name,
            path: info.path,
            label: info.label,
            model: info.model,
            is_bootloader: info.is_bootloader,
        }
    }
}

/// Device features after initialization.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorFeatures {
    /// Vendor string
    pub vendor: Option<String>,
    /// Device model
    pub model: Option<String>,
    /// Device label (set by user during device setup)
    pub label: Option<String>,
    /// Device ID (unique per device)
    pub device_id: Option<String>,
    /// Major firmware version
    pub major_version: Option<u32>,
    /// Minor firmware version
    pub minor_version: Option<u32>,
    /// Patch firmware version
    pub patch_version: Option<u32>,
    /// Whether PIN protection is enabled
    pub pin_protection: Option<bool>,
    /// Whether passphrase protection is enabled
    pub passphrase_protection: Option<bool>,
    /// Whether the device is initialized with a seed
    pub initialized: Option<bool>,
    /// Whether the device needs backup
    pub needs_backup: Option<bool>,
}

impl From<trezor_connect_rs::device::Features> for TrezorFeatures {
    fn from(f: trezor_connect_rs::device::Features) -> Self {
        Self {
            vendor: f.vendor,
            model: f.model,
            label: f.label,
            device_id: f.device_id,
            major_version: f.major_version,
            minor_version: f.minor_version,
            patch_version: f.patch_version,
            pin_protection: f.pin_protection,
            passphrase_protection: f.passphrase_protection,
            initialized: f.initialized,
            needs_backup: f.needs_backup,
        }
    }
}

/// Script types for address derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrezorScriptType {
    /// P2PKH (legacy)
    SpendAddress,
    /// P2SH-P2WPKH (nested SegWit)
    SpendP2shWitness,
    /// P2WPKH (native SegWit)
    SpendWitness,
    /// P2TR (Taproot)
    SpendTaproot,
    /// P2SH multisig
    SpendMultisig,
    /// External/watch-only input (not signed by device)
    External,
}

impl From<TrezorScriptType> for trezor_connect_rs::ScriptType {
    fn from(t: TrezorScriptType) -> Self {
        match t {
            TrezorScriptType::SpendAddress => trezor_connect_rs::ScriptType::SpendAddress,
            TrezorScriptType::SpendP2shWitness => trezor_connect_rs::ScriptType::SpendP2SHWitness,
            TrezorScriptType::SpendWitness => trezor_connect_rs::ScriptType::SpendWitness,
            TrezorScriptType::SpendTaproot => trezor_connect_rs::ScriptType::SpendTaproot,
            TrezorScriptType::SpendMultisig => trezor_connect_rs::ScriptType::SpendMultisig,
            TrezorScriptType::External => trezor_connect_rs::ScriptType::External,
        }
    }
}

impl From<trezor_connect_rs::ScriptType> for TrezorScriptType {
    fn from(t: trezor_connect_rs::ScriptType) -> Self {
        match t {
            trezor_connect_rs::ScriptType::SpendAddress => TrezorScriptType::SpendAddress,
            trezor_connect_rs::ScriptType::SpendP2SHWitness => TrezorScriptType::SpendP2shWitness,
            trezor_connect_rs::ScriptType::SpendWitness => TrezorScriptType::SpendWitness,
            trezor_connect_rs::ScriptType::SpendTaproot => TrezorScriptType::SpendTaproot,
            trezor_connect_rs::ScriptType::SpendMultisig => TrezorScriptType::SpendMultisig,
            trezor_connect_rs::ScriptType::External => TrezorScriptType::External,
        }
    }
}

/// Bitcoin network / coin type for Trezor operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrezorCoinType {
    /// Bitcoin mainnet
    Bitcoin,
    /// Bitcoin testnet
    Testnet,
    /// Bitcoin signet (treated as testnet by the device)
    Signet,
    /// Bitcoin regtest
    Regtest,
}

impl From<TrezorCoinType> for trezor_connect_rs::Network {
    fn from(c: TrezorCoinType) -> Self {
        match c {
            TrezorCoinType::Bitcoin => trezor_connect_rs::Network::Bitcoin,
            TrezorCoinType::Testnet | TrezorCoinType::Signet => trezor_connect_rs::Network::Testnet,
            TrezorCoinType::Regtest => trezor_connect_rs::Network::Regtest,
        }
    }
}

/// Parameters for getting an address from the device.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorGetAddressParams {
    /// BIP32 path (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
    /// Coin network (default: Bitcoin)
    pub coin: Option<TrezorCoinType>,
    /// Whether to display the address on the device for confirmation
    pub show_on_trezor: bool,
    /// Script type (auto-detected from path if not specified)
    pub script_type: Option<TrezorScriptType>,
}

impl From<TrezorGetAddressParams> for trezor_connect_rs::GetAddressParams {
    fn from(p: TrezorGetAddressParams) -> Self {
        Self {
            path: p.path,
            coin: p.coin.map(|c| c.into()),
            show_on_trezor: p.show_on_trezor,
            script_type: p.script_type.map(|s| s.into()),
            multisig: None,
        }
    }
}

/// Address response from device.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorAddressResponse {
    /// The Bitcoin address
    pub address: String,
    /// The serialized path (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
}

impl From<trezor_connect_rs::AddressResponse> for TrezorAddressResponse {
    fn from(r: trezor_connect_rs::AddressResponse) -> Self {
        Self {
            address: r.address,
            path: r.serialized_path,
        }
    }
}

/// Parameters for getting a public key from the device.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorGetPublicKeyParams {
    /// BIP32 path (e.g., "m/84'/0'/0'")
    pub path: String,
    /// Coin network (default: Bitcoin)
    pub coin: Option<TrezorCoinType>,
    /// Whether to display on device for confirmation
    pub show_on_trezor: bool,
}

impl From<TrezorGetPublicKeyParams> for trezor_connect_rs::GetPublicKeyParams {
    fn from(p: TrezorGetPublicKeyParams) -> Self {
        Self {
            path: p.path,
            coin: p.coin.map(|c| c.into()),
            show_on_trezor: p.show_on_trezor,
            script_type: None,
        }
    }
}

/// Public key response from device.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorPublicKeyResponse {
    /// Extended public key (xpub)
    pub xpub: String,
    /// The serialized path (e.g., "m/84'/0'/0'")
    pub path: String,
    /// Compressed public key (hex encoded)
    pub public_key: String,
    /// Chain code (hex encoded)
    pub chain_code: String,
    /// Parent key fingerprint
    pub fingerprint: u32,
    /// Derivation depth
    pub depth: u32,
    /// Master root fingerprint (from the device's master seed)
    pub root_fingerprint: Option<u32>,
}

impl From<trezor_connect_rs::PublicKeyResponse> for TrezorPublicKeyResponse {
    fn from(r: trezor_connect_rs::PublicKeyResponse) -> Self {
        Self {
            xpub: r.xpub,
            path: r.serialized_path,
            public_key: r.public_key,
            chain_code: r.chain_code,
            fingerprint: r.fingerprint,
            depth: r.depth,
            root_fingerprint: r.root_fingerprint,
        }
    }
}

/// Parameters for signing a message.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorSignMessageParams {
    /// BIP32 path for the signing key (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
    /// Message to sign
    pub message: String,
    /// Coin network (default: Bitcoin)
    pub coin: Option<TrezorCoinType>,
}

impl From<TrezorSignMessageParams> for trezor_connect_rs::SignMessageParams {
    fn from(p: TrezorSignMessageParams) -> Self {
        Self {
            path: p.path,
            message: p.message,
            coin: p.coin.map(|c| c.into()),
            no_script_type: false,
        }
    }
}

/// Response from signing a message.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorSignedMessageResponse {
    /// Bitcoin address that signed the message
    pub address: String,
    /// Signature (base64 encoded)
    pub signature: String,
}

impl From<trezor_connect_rs::SignedMessageResponse> for TrezorSignedMessageResponse {
    fn from(r: trezor_connect_rs::SignedMessageResponse) -> Self {
        Self {
            address: r.address,
            signature: r.signature,
        }
    }
}

/// Parameters for verifying a message signature.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorVerifyMessageParams {
    /// Bitcoin address that signed the message
    pub address: String,
    /// Signature (base64 encoded)
    pub signature: String,
    /// Original message
    pub message: String,
    /// Coin network (default: Bitcoin)
    pub coin: Option<TrezorCoinType>,
}

impl From<TrezorVerifyMessageParams> for trezor_connect_rs::VerifyMessageParams {
    fn from(p: TrezorVerifyMessageParams) -> Self {
        Self {
            address: p.address,
            signature: p.signature,
            message: p.message,
            coin: p.coin.map(|c| c.into()),
        }
    }
}

/// Transaction input for signing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorTxInput {
    /// Previous transaction hash (hex, 32 bytes)
    pub prev_hash: String,
    /// Previous output index
    pub prev_index: u32,
    /// BIP32 derivation path (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
    /// Amount in satoshis
    pub amount: u64,
    /// Script type
    pub script_type: TrezorScriptType,
    /// Sequence number (default: 0xFFFFFFFD for RBF)
    pub sequence: Option<u32>,
    /// Original transaction hash for RBF replacement (hex encoded)
    pub orig_hash: Option<String>,
    /// Original input index for RBF replacement
    pub orig_index: Option<u32>,
}

/// Transaction output for signing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorTxOutput {
    /// Destination address (for external outputs)
    pub address: Option<String>,
    /// BIP32 path (for change outputs)
    pub path: Option<String>,
    /// Amount in satoshis
    pub amount: u64,
    /// Script type (for change outputs)
    pub script_type: Option<TrezorScriptType>,
    /// OP_RETURN data (hex encoded, for data outputs)
    pub op_return_data: Option<String>,
    /// Original transaction hash for RBF replacement (hex encoded)
    pub orig_hash: Option<String>,
    /// Original output index for RBF replacement
    pub orig_index: Option<u32>,
}

/// Previous transaction data (for non-SegWit input verification).
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorPrevTx {
    /// Transaction hash (hex encoded)
    pub hash: String,
    /// Transaction version
    pub version: u32,
    /// Lock time
    pub lock_time: u32,
    /// Transaction inputs
    pub inputs: Vec<TrezorPrevTxInput>,
    /// Transaction outputs
    pub outputs: Vec<TrezorPrevTxOutput>,
}

/// Input of a previous transaction.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorPrevTxInput {
    /// Previous transaction hash (hex encoded)
    pub prev_hash: String,
    /// Previous output index
    pub prev_index: u32,
    /// Script signature (hex encoded)
    pub script_sig: String,
    /// Sequence number
    pub sequence: u32,
}

/// Output of a previous transaction.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorPrevTxOutput {
    /// Amount in satoshis
    pub amount: u64,
    /// Script pubkey (hex encoded)
    pub script_pubkey: String,
}

/// Parameters for signing a transaction.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorSignTxParams {
    /// Transaction inputs
    pub inputs: Vec<TrezorTxInput>,
    /// Transaction outputs
    pub outputs: Vec<TrezorTxOutput>,
    /// Coin network (default: Bitcoin)
    pub coin: Option<TrezorCoinType>,
    /// Lock time (default: 0)
    pub lock_time: Option<u32>,
    /// Version (default: 2)
    pub version: Option<u32>,
    /// Previous transactions (for non-SegWit input verification)
    pub prev_txs: Vec<TrezorPrevTx>,
}

/// Signed transaction result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorSignedTx {
    /// Signatures for each input (hex encoded)
    pub signatures: Vec<String>,
    /// Serialized transaction (hex)
    pub serialized_tx: String,
    /// Broadcast transaction ID (populated when push=true)
    pub txid: Option<String>,
}

impl From<TrezorTxInput> for trezor_connect_rs::SignTxInput {
    fn from(input: TrezorTxInput) -> Self {
        Self {
            prev_hash: input.prev_hash,
            prev_index: input.prev_index,
            path: input.path,
            amount: input.amount,
            script_type: input.script_type.into(),
            sequence: input.sequence,
            orig_hash: input.orig_hash,
            orig_index: input.orig_index,
            multisig: None,
            script_pubkey: None,
            script_sig: None,
            witness: None,
            ownership_proof: None,
            commitment_data: None,
        }
    }
}

impl From<TrezorTxOutput> for trezor_connect_rs::SignTxOutput {
    fn from(output: TrezorTxOutput) -> Self {
        Self {
            address: output.address,
            path: output.path,
            amount: output.amount,
            script_type: output.script_type.map(|s| s.into()),
            op_return_data: output.op_return_data,
            orig_hash: output.orig_hash,
            orig_index: output.orig_index,
            multisig: None,
            payment_req_index: None,
        }
    }
}

impl From<TrezorPrevTxInput> for trezor_connect_rs::SignTxPrevTxInput {
    fn from(input: TrezorPrevTxInput) -> Self {
        Self {
            prev_hash: input.prev_hash,
            prev_index: input.prev_index,
            script_sig: input.script_sig,
            sequence: input.sequence,
        }
    }
}

impl From<TrezorPrevTxOutput> for trezor_connect_rs::SignTxPrevTxOutput {
    fn from(output: TrezorPrevTxOutput) -> Self {
        Self {
            amount: output.amount,
            script_pubkey: output.script_pubkey,
        }
    }
}

impl From<TrezorPrevTx> for trezor_connect_rs::SignTxPrevTx {
    fn from(tx: TrezorPrevTx) -> Self {
        Self {
            hash: tx.hash,
            version: tx.version,
            lock_time: tx.lock_time,
            inputs: tx.inputs.into_iter().map(|i| i.into()).collect(),
            outputs: tx.outputs.into_iter().map(|o| o.into()).collect(),
            extra_data: None,
        }
    }
}

impl From<TrezorSignTxParams> for trezor_connect_rs::SignTxParams {
    fn from(params: TrezorSignTxParams) -> Self {
        Self {
            inputs: params.inputs.into_iter().map(|i| i.into()).collect(),
            outputs: params.outputs.into_iter().map(|o| o.into()).collect(),
            coin: params.coin.map(|c| c.into()),
            lock_time: params.lock_time,
            version: params.version,
            prev_txs: params.prev_txs.into_iter().map(|t| t.into()).collect(),
            push: None,
            amount_unit: None,
            serialize: None,
            chunkify: None,
            unlock_path: None,
            payment_requests: vec![],
        }
    }
}

impl From<trezor_connect_rs::SignedTxResponse> for TrezorSignedTx {
    fn from(response: trezor_connect_rs::SignedTxResponse) -> Self {
        Self {
            signatures: response.signatures,
            serialized_tx: response.serialized_tx,
            txid: response.txid,
        }
    }
}

// ============================================================================
// Account info types for Trezor compose
// ============================================================================

/// Account type classification for extended public keys.
///
/// Determines the BIP standard, derivation path purpose, and script type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccountType {
    /// BIP44 legacy (P2PKH) — xpub/tpub prefix
    Legacy,
    /// BIP49 wrapped segwit (P2SH-P2WPKH) — ypub/upub prefix
    WrappedSegwit,
    /// BIP84 native segwit (P2WPKH) — zpub/vpub prefix
    NativeSegwit,
    /// BIP86 taproot (P2TR)
    Taproot,
}

/// A UTXO in the format expected by Trezor compose.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountUtxo {
    /// Transaction ID (hex)
    pub txid: String,
    /// Output index
    pub vout: u32,
    /// Amount in satoshis
    pub amount: u64,
    /// Block height where the UTXO was confirmed (0 if unconfirmed)
    pub block_height: u32,
    /// Address holding this UTXO
    pub address: String,
    /// BIP32 derivation path (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
    /// Number of confirmations (0 if unconfirmed)
    pub confirmations: u32,
    /// Whether this is a coinbase output
    pub coinbase: bool,
    /// Whether this UTXO is owned by the account
    pub own: bool,
    /// Whether this UTXO must be included in the transaction
    pub required: Option<bool>,
}

/// Information about a single address in an account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AddressInfo {
    /// The Bitcoin address
    pub address: String,
    /// BIP32 derivation path
    pub path: String,
    /// Number of transactions involving this address
    pub transfers: u32,
}

/// Grouped address lists for an account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountAddresses {
    /// Used receive addresses (have at least one transaction)
    pub used: Vec<AddressInfo>,
    /// Unused receive addresses (no transactions yet)
    pub unused: Vec<AddressInfo>,
    /// Change addresses
    pub change: Vec<AddressInfo>,
}

/// Full account structure for Trezor compose.
///
/// This is the `account` object expected by `composeTransaction` in
/// precompose mode.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ComposeAccount {
    /// Account derivation path (e.g., "m/84'/0'/0'")
    pub path: String,
    /// Categorized addresses
    pub addresses: AccountAddresses,
    /// Unspent transaction outputs
    pub utxo: Vec<AccountUtxo>,
}

/// Result from querying an extended public key — ready for Trezor compose.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountInfoResult {
    /// The compose-compatible account structure
    pub account: ComposeAccount,
    /// Total confirmed balance in satoshis
    pub balance: u64,
    /// Number of UTXOs
    pub utxo_count: u32,
    /// The detected or specified account type
    pub account_type: AccountType,
    /// The current blockchain tip height
    pub block_height: u32,
}

// ============================================================================
// From impls: bitkit-core account types → trezor-connect-rs compose types
// ============================================================================

impl From<AddressInfo> for trezor_connect_rs::api::compose::AccountAddress {
    fn from(a: AddressInfo) -> Self {
        Self {
            address: a.address,
            path: a.path,
            transfers: a.transfers,
        }
    }
}

impl From<AccountAddresses> for trezor_connect_rs::api::compose::AccountAddresses {
    fn from(a: AccountAddresses) -> Self {
        Self {
            used: a.used.into_iter().map(|x| x.into()).collect(),
            unused: a.unused.into_iter().map(|x| x.into()).collect(),
            change: a.change.into_iter().map(|x| x.into()).collect(),
        }
    }
}

impl From<AccountUtxo> for trezor_connect_rs::api::compose::ComposeUtxo {
    fn from(u: AccountUtxo) -> Self {
        Self {
            txid: u.txid,
            vout: u.vout,
            amount: u.amount,
            address: u.address,
            path: u.path,
            confirmations: u.confirmations,
            coinbase: u.coinbase,
            own: u.own,
            required: u.required,
        }
    }
}

impl From<ComposeAccount> for trezor_connect_rs::api::compose::ComposeAccount {
    fn from(a: ComposeAccount) -> Self {
        Self {
            path: a.path,
            addresses: a.addresses.into(),
            utxo: a.utxo.into_iter().map(|u| u.into()).collect(),
        }
    }
}

/// Result from querying a single Bitcoin address.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SingleAddressInfoResult {
    /// The queried address
    pub address: String,
    /// Total confirmed balance in satoshis
    pub balance: u64,
    /// UTXOs for this address
    pub utxos: Vec<AccountUtxo>,
    /// Number of transactions involving this address
    pub transfers: u32,
    /// Current blockchain tip height
    pub block_height: u32,
}

// ============================================================================
// Compose FFI types
// ============================================================================

/// Fee level for transaction composition.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorFeeLevel {
    /// Fee rate in sat/vB
    pub fee_per_unit: String,
    /// Base fee in satoshis (optional, added to calculated fee)
    pub base_fee: Option<u64>,
    /// Whether to use floor for base fee calculation
    pub floor_base_fee: Option<bool>,
}

impl From<TrezorFeeLevel> for trezor_connect_rs::api::compose::FeeLevel {
    fn from(f: TrezorFeeLevel) -> Self {
        Self {
            fee_per_unit: f.fee_per_unit,
            base_fee: f.base_fee,
            floor_base_fee: f.floor_base_fee,
        }
    }
}

/// Sorting strategy for transaction inputs and outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrezorSortingStrategy {
    /// BIP-69: deterministic lexicographic sorting
    Bip69,
    /// Random shuffle (better privacy)
    Random,
    /// Keep original order
    None,
}

impl From<TrezorSortingStrategy> for trezor_connect_rs::compose::sorting::SortingStrategy {
    fn from(s: TrezorSortingStrategy) -> Self {
        match s {
            TrezorSortingStrategy::Bip69 => trezor_connect_rs::compose::sorting::SortingStrategy::Bip69,
            TrezorSortingStrategy::Random => trezor_connect_rs::compose::sorting::SortingStrategy::Random,
            TrezorSortingStrategy::None => trezor_connect_rs::compose::sorting::SortingStrategy::None,
        }
    }
}

impl From<trezor_connect_rs::compose::sorting::SortingStrategy> for TrezorSortingStrategy {
    fn from(s: trezor_connect_rs::compose::sorting::SortingStrategy) -> Self {
        match s {
            trezor_connect_rs::compose::sorting::SortingStrategy::Bip69 => TrezorSortingStrategy::Bip69,
            trezor_connect_rs::compose::sorting::SortingStrategy::Random => TrezorSortingStrategy::Random,
            trezor_connect_rs::compose::sorting::SortingStrategy::None => TrezorSortingStrategy::None,
        }
    }
}

/// Output specification for precompose.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum TrezorPrecomposeOutput {
    /// Payment to a specific address
    Payment { address: String, amount: String },
    /// Payment without address (estimation only)
    PaymentNoAddress { amount: String },
    /// Send all remaining funds to an address
    SendMax { address: String },
    /// Send all remaining funds (no address)
    SendMaxNoAddress,
    /// OP_RETURN data output
    OpReturn { data_hex: String },
}

impl From<TrezorPrecomposeOutput> for trezor_connect_rs::api::compose::PrecomposeOutput {
    fn from(o: TrezorPrecomposeOutput) -> Self {
        match o {
            TrezorPrecomposeOutput::Payment { address, amount } => {
                trezor_connect_rs::api::compose::PrecomposeOutput::Payment { address, amount }
            }
            TrezorPrecomposeOutput::PaymentNoAddress { amount } => {
                trezor_connect_rs::api::compose::PrecomposeOutput::PaymentNoAddress { amount }
            }
            TrezorPrecomposeOutput::SendMax { address } => {
                trezor_connect_rs::api::compose::PrecomposeOutput::SendMax { address }
            }
            TrezorPrecomposeOutput::SendMaxNoAddress => {
                trezor_connect_rs::api::compose::PrecomposeOutput::SendMaxNoAddress
            }
            TrezorPrecomposeOutput::OpReturn { data_hex } => {
                trezor_connect_rs::api::compose::PrecomposeOutput::OpReturn { data_hex }
            }
        }
    }
}

/// Parameters for precompose transaction.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorPrecomposeParams {
    /// Desired outputs
    pub outputs: Vec<TrezorPrecomposeOutput>,
    /// Coin name (e.g., "Bitcoin", "Regtest")
    pub coin: String,
    /// Account with UTXOs and addresses
    pub account: ComposeAccount,
    /// Fee levels to evaluate
    pub fee_levels: Vec<TrezorFeeLevel>,
    /// Default sequence number
    pub sequence: Option<u32>,
    /// Sorting strategy for inputs/outputs
    pub sorting_strategy: Option<TrezorSortingStrategy>,
}

impl From<TrezorPrecomposeParams> for trezor_connect_rs::api::compose::PrecomposeParams {
    fn from(p: TrezorPrecomposeParams) -> Self {
        Self {
            outputs: p.outputs.into_iter().map(|o| o.into()).collect(),
            coin: p.coin,
            account: p.account.into(),
            fee_levels: p.fee_levels.into_iter().map(|f| f.into()).collect(),
            sequence: p.sequence,
            sorting_strategy: p.sorting_strategy.map(|s| s.into()),
        }
    }
}

/// Input in a precomposed result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorPrecomposedInput {
    /// Transaction ID (hex)
    pub txid: String,
    /// Output index
    pub vout: u32,
    /// Amount in satoshis (as string)
    pub amount: String,
    /// Address
    pub address: String,
    /// BIP32 derivation path
    pub path: String,
    /// Script type
    pub script_type: TrezorScriptType,
}

impl From<trezor_connect_rs::api::compose::PrecomposedInput> for TrezorPrecomposedInput {
    fn from(i: trezor_connect_rs::api::compose::PrecomposedInput) -> Self {
        Self {
            txid: i.txid,
            vout: i.vout,
            amount: i.amount,
            address: i.address,
            path: i.path,
            script_type: i.script_type.into(),
        }
    }
}

/// Output in a precomposed result.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum TrezorPrecomposedOutput {
    /// Payment to an address
    Payment { address: String, amount: String },
    /// Change output
    Change { address: String, path: String, amount: String, script_type: TrezorScriptType },
    /// OP_RETURN data output
    OpReturn { data_hex: String },
}

impl From<trezor_connect_rs::api::compose::PrecomposedOutput> for TrezorPrecomposedOutput {
    fn from(o: trezor_connect_rs::api::compose::PrecomposedOutput) -> Self {
        match o {
            trezor_connect_rs::api::compose::PrecomposedOutput::Payment { address, amount } => {
                TrezorPrecomposedOutput::Payment { address, amount }
            }
            trezor_connect_rs::api::compose::PrecomposedOutput::Change { address, path, amount, script_type } => {
                TrezorPrecomposedOutput::Change {
                    address,
                    path,
                    amount,
                    script_type: script_type.into(),
                }
            }
            trezor_connect_rs::api::compose::PrecomposedOutput::OpReturn { data_hex } => {
                TrezorPrecomposedOutput::OpReturn { data_hex }
            }
        }
    }
}

/// Precomposed transaction result (one per fee level).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum TrezorPrecomposedResult {
    /// Successfully composed a sendable transaction
    Final {
        total_spent: String,
        fee: String,
        fee_per_byte: String,
        bytes: u32,
        inputs: Vec<TrezorPrecomposedInput>,
        outputs: Vec<TrezorPrecomposedOutput>,
        outputs_permutation: Vec<u32>,
    },
    /// Non-final result (e.g., send-max estimation)
    NonFinal {
        max: Option<String>,
        total_spent: String,
        fee: String,
        fee_per_byte: String,
        bytes: u32,
    },
    /// Composition failed
    Error {
        error: String,
    },
}

impl From<trezor_connect_rs::api::compose::PrecomposedResult> for TrezorPrecomposedResult {
    fn from(r: trezor_connect_rs::api::compose::PrecomposedResult) -> Self {
        match r {
            trezor_connect_rs::api::compose::PrecomposedResult::Final {
                total_spent, fee, fee_per_byte, bytes, inputs, outputs, outputs_permutation,
            } => TrezorPrecomposedResult::Final {
                total_spent,
                fee,
                fee_per_byte,
                bytes: bytes as u32,
                inputs: inputs.into_iter().map(|i| i.into()).collect(),
                outputs: outputs.into_iter().map(|o| o.into()).collect(),
                outputs_permutation: outputs_permutation.into_iter().map(|i| i as u32).collect(),
            },
            trezor_connect_rs::api::compose::PrecomposedResult::NonFinal {
                max, total_spent, fee, fee_per_byte, bytes,
            } => TrezorPrecomposedResult::NonFinal {
                max,
                total_spent,
                fee,
                fee_per_byte,
                bytes: bytes as u32,
            },
            trezor_connect_rs::api::compose::PrecomposedResult::Error { error } => {
                TrezorPrecomposedResult::Error { error }
            }
        }
    }
}