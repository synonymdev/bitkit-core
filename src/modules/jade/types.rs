//! UniFFI scaffolding for the `jade-client-rs` types.
//!
//! Every type here is defined in that crate, not this one. `#[uniffi::remote]`
//! attaches the same scaffolding `#[derive(uniffi::…)]` would, without a
//! mirrored set of structs and hand-written `From` conversions in both
//! directions. The trezor module predates this and pays that cost; this module
//! does not.
//!
//! The declarations below must match the upstream definitions variant for
//! variant and field for field. The compiler catches a mismatch, and the tests
//! in `tests.rs` exercise the round trip.

pub use jade_client_rs::{
    JadeAccount, JadeAccountExport, JadeAddressVariant, JadeDeviceInfo, JadeError, JadeNetwork,
    JadePingStatus, JadeSignedMessage, JadeState, JadeTransportErrorCode, JadeTransportKind,
    JadeVersionInfo, JadeXpubResponse,
};

use crate::onchain::AccountType;

#[uniffi::remote(Enum)]
pub enum JadeNetwork {
    Mainnet,
    Testnet,
    Regtest,
}

#[uniffi::remote(Enum)]
pub enum JadeTransportKind {
    Bluetooth,
    Serial,
}

#[uniffi::remote(Enum)]
pub enum JadeAddressVariant {
    Pkh,
    Wpkh,
    ShWpkh,
    Tr,
}

#[uniffi::remote(Enum)]
pub enum JadeState {
    Uninit,
    Unsaved,
    Locked,
    Ready,
    Temp,
    Unknown,
}

#[uniffi::remote(Enum)]
pub enum JadePingStatus {
    Idle,
    Busy,
    AwaitingUserInput,
}

#[uniffi::remote(Enum)]
pub enum JadeTransportErrorCode {
    DeviceBusy,
    NotConnected,
    Disconnected,
    Timeout,
    PermissionDenied,
}

#[uniffi::remote(Record)]
pub struct JadeDeviceInfo {
    pub path: String,
    pub transport: JadeTransportKind,
    pub name: Option<String>,
    pub serial_number: Option<String>,
}

#[uniffi::remote(Record)]
pub struct JadeVersionInfo {
    pub jade_version: String,
    pub jade_state: JadeState,
    pub jade_networks: Option<String>,
    pub jade_has_pin: Option<bool>,
    pub board_type: Option<String>,
    pub jade_config: Option<String>,
    pub jade_features: Option<String>,
    pub idf_version: Option<String>,
    pub chip_features: Option<String>,
    pub efuse_mac: Option<String>,
    pub battery_status: Option<u32>,
    pub jade_ota_max_chunk: Option<u32>,
}

#[uniffi::remote(Record)]
pub struct JadeXpubResponse {
    pub xpub: String,
    pub derivation_path: String,
    pub master_fingerprint: String,
}

#[uniffi::remote(Record)]
pub struct JadeAccount {
    pub variant: JadeAddressVariant,
    pub xpub: String,
    pub derivation_path: String,
}

#[uniffi::remote(Record)]
pub struct JadeAccountExport {
    pub master_fingerprint: String,
    pub account_index: u32,
    pub accounts: Vec<JadeAccount>,
}

#[uniffi::remote(Record)]
pub struct JadeSignedMessage {
    pub signature: String,
    pub address: String,
    pub derivation_path: String,
}

#[uniffi::remote(Error)]
pub enum JadeError {
    TransportError { error_details: String },
    DeviceNotFound,
    DeviceDisconnected,
    DeviceBusy,
    NotConnected,
    NotInitialized,
    ConnectionError { error_details: String },
    ProtocolError { error_details: String },
    Timeout,
    UserCancelled,
    DeviceLocked,
    DeviceUninitialized,
    InvalidPin,
    NetworkMismatch { error_details: String },
    UnsupportedFirmware { installed: String, required: String },
    InvalidPath { error_details: String },
    InvalidPsbt { error_details: String },
    PsbtTooLarge { size: u64, max: u64 },
    FingerprintMismatch { device: String, psbt: String },
    NothingSigned,
    AddressMismatch { expected: String, returned: String },
    PinServerError { error_details: String },
    DeviceError { error_details: String },
    IoError { error_details: String },
}

/// Map the signer-neutral account type onto Jade's descriptor variant.
pub(crate) fn account_type_to_variant(account_type: AccountType) -> JadeAddressVariant {
    match account_type {
        AccountType::Legacy => JadeAddressVariant::Pkh,
        AccountType::WrappedSegwit => JadeAddressVariant::ShWpkh,
        AccountType::NativeSegwit => JadeAddressVariant::Wpkh,
        AccountType::Taproot => JadeAddressVariant::Tr,
    }
}
