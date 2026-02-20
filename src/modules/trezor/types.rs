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
}

impl From<TrezorScriptType> for trezor_connect_rs::ScriptType {
    fn from(t: TrezorScriptType) -> Self {
        match t {
            TrezorScriptType::SpendAddress => trezor_connect_rs::ScriptType::SpendAddress,
            TrezorScriptType::SpendP2shWitness => trezor_connect_rs::ScriptType::SpendP2SHWitness,
            TrezorScriptType::SpendWitness => trezor_connect_rs::ScriptType::SpendWitness,
            TrezorScriptType::SpendTaproot => trezor_connect_rs::ScriptType::SpendTaproot,
            TrezorScriptType::SpendMultisig => trezor_connect_rs::ScriptType::SpendMultisig,
        }
    }
}

/// Parameters for getting an address from the device.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorGetAddressParams {
    /// BIP32 path (e.g., "m/84'/0'/0'/0/0")
    pub path: String,
    /// Coin name (default: "Bitcoin")
    pub coin: Option<String>,
    /// Whether to display the address on the device for confirmation
    pub show_on_trezor: bool,
    /// Script type (auto-detected from path if not specified)
    pub script_type: Option<TrezorScriptType>,
}

impl From<TrezorGetAddressParams> for trezor_connect_rs::GetAddressParams {
    fn from(p: TrezorGetAddressParams) -> Self {
        Self {
            path: p.path,
            coin: p.coin,
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
    /// Coin name (default: "Bitcoin")
    pub coin: Option<String>,
    /// Whether to display on device for confirmation
    pub show_on_trezor: bool,
}

impl From<TrezorGetPublicKeyParams> for trezor_connect_rs::GetPublicKeyParams {
    fn from(p: TrezorGetPublicKeyParams) -> Self {
        Self {
            path: p.path,
            coin: p.coin,
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
    /// Coin name (default: "Bitcoin")
    pub coin: Option<String>,
}

impl From<TrezorSignMessageParams> for trezor_connect_rs::SignMessageParams {
    fn from(p: TrezorSignMessageParams) -> Self {
        Self {
            path: p.path,
            message: p.message,
            coin: p.coin,
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
    /// Coin name (default: "Bitcoin")
    pub coin: Option<String>,
}

impl From<TrezorVerifyMessageParams> for trezor_connect_rs::VerifyMessageParams {
    fn from(p: TrezorVerifyMessageParams) -> Self {
        Self {
            address: p.address,
            signature: p.signature,
            message: p.message,
            coin: p.coin,
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
    /// Coin name (default: "Bitcoin")
    pub coin: Option<String>,
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
        }
    }
}

impl From<TrezorSignTxParams> for trezor_connect_rs::SignTxParams {
    fn from(params: TrezorSignTxParams) -> Self {
        Self {
            inputs: params.inputs.into_iter().map(|i| i.into()).collect(),
            outputs: params.outputs.into_iter().map(|o| o.into()).collect(),
            coin: params.coin,
            lock_time: params.lock_time,
            version: params.version,
            prev_txs: params.prev_txs.into_iter().map(|t| t.into()).collect(),
        }
    }
}

impl From<trezor_connect_rs::SignedTxResponse> for TrezorSignedTx {
    fn from(response: trezor_connect_rs::SignedTxResponse) -> Self {
        Self {
            signatures: response.signatures,
            serialized_tx: response.serialized_tx,
        }
    }
}
