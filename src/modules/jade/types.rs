//! FFI-compatible types for the Jade module.
//!
//! The records here are the shapes the bindings see. Wire shapes stay private:
//! Jade's `get_version_info` reply uses SCREAMING_SNAKE keys and a string state,
//! so deriving `Deserialize` straight onto the FFI record would silently yield
//! nothing but `None`.

use serde::Deserialize;

use crate::onchain::AccountType;

/// The oldest firmware this module targets.
///
/// Single-signature `get_receive_address` and `sign_psbt` were added during the
/// 0.1.x series. The device is the authority here: an older unit answers
/// `UNKNOWN_METHOD`, which maps to `JadeError::UnsupportedFirmware`, so this
/// constant is advisory and only improves the message.
pub(crate) const MIN_JADE_FIRMWARE: &str = "0.1.48";

/// Taproot address support landed in 1.0.34.
pub(crate) const MIN_JADE_FIRMWARE_TAPROOT: &str = "1.0.34";

/// Compare two dotted version strings.
///
/// Returns false when either side cannot be parsed, so an unrecognised version
/// string never blocks an operation the device might well support. The device
/// remains the authority: it answers `BAD_PARAMETERS` for a variant it does not
/// know, and this check only turns that into a clearer message.
pub(crate) fn version_at_least(installed: &str, required: &str) -> bool {
    fn parts(version: &str) -> Option<(u32, u32, u32)> {
        let trimmed = version
            .trim()
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()?;
        let mut fields = trimmed.split('.').map(str::parse::<u32>);
        let major = fields.next()?.ok()?;
        let minor = fields.next().transpose().ok()?.unwrap_or(0);
        let patch = fields.next().transpose().ok()?.unwrap_or(0);
        Some((major, minor, patch))
    }

    match (parts(installed), parts(required)) {
        (Some(installed), Some(required)) => installed >= required,
        _ => false,
    }
}

/// The Bitcoin networks Jade recognises.
///
/// Jade has no signet, so there are exactly three. Its regtest is named
/// `localtest` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JadeNetwork {
    Mainnet,
    Testnet,
    Regtest,
}

impl JadeNetwork {
    pub(crate) fn wire_name(self) -> &'static str {
        match self {
            JadeNetwork::Mainnet => "mainnet",
            JadeNetwork::Testnet => "testnet",
            JadeNetwork::Regtest => "localtest",
        }
    }

    /// The BIP44 coin type this network derives under.
    pub(crate) fn coin_type(self) -> u32 {
        match self {
            JadeNetwork::Mainnet => 0,
            JadeNetwork::Testnet | JadeNetwork::Regtest => 1,
        }
    }
}

impl From<JadeNetwork> for bitcoin::Network {
    fn from(network: JadeNetwork) -> Self {
        match network {
            JadeNetwork::Mainnet => bitcoin::Network::Bitcoin,
            JadeNetwork::Testnet => bitcoin::Network::Testnet,
            JadeNetwork::Regtest => bitcoin::Network::Regtest,
        }
    }
}

/// How the host reaches a particular device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JadeTransportKind {
    Bluetooth,
    Serial,
}

impl JadeTransportKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            JadeTransportKind::Bluetooth => "ble",
            JadeTransportKind::Serial => "serial",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "ble" => Some(JadeTransportKind::Bluetooth),
            "serial" => Some(JadeTransportKind::Serial),
            _ => None,
        }
    }
}

/// The single-signature descriptor variants Jade accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JadeAddressVariant {
    Pkh,
    Wpkh,
    ShWpkh,
    Tr,
}

impl JadeAddressVariant {
    pub(crate) fn wire_name(self) -> &'static str {
        match self {
            JadeAddressVariant::Pkh => "pkh(k)",
            JadeAddressVariant::Wpkh => "wpkh(k)",
            JadeAddressVariant::ShWpkh => "sh(wpkh(k))",
            JadeAddressVariant::Tr => "tr(k)",
        }
    }

    /// The BIP44 purpose this variant is derived under.
    pub(crate) fn purpose(self) -> u32 {
        match self {
            JadeAddressVariant::Pkh => 44,
            JadeAddressVariant::ShWpkh => 49,
            JadeAddressVariant::Wpkh => 84,
            JadeAddressVariant::Tr => 86,
        }
    }
}

impl From<AccountType> for JadeAddressVariant {
    fn from(account_type: AccountType) -> Self {
        match account_type {
            AccountType::Legacy => JadeAddressVariant::Pkh,
            AccountType::WrappedSegwit => JadeAddressVariant::ShWpkh,
            AccountType::NativeSegwit => JadeAddressVariant::Wpkh,
            AccountType::Taproot => JadeAddressVariant::Tr,
        }
    }
}

/// The device's wallet state, as reported by `get_version_info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JadeState {
    /// No wallet. Setup has to be completed on the device itself.
    Uninit,
    /// A wallet exists but has not been persisted with a PIN.
    Unsaved,
    /// A wallet exists and is PIN locked. Call `jade_unlock`.
    Locked,
    /// Unlocked and usable.
    Ready,
    /// A temporary wallet session is active.
    Temp,
    /// Firmware reported a state this version does not know about.
    Unknown,
}

impl JadeState {
    fn from_wire(value: &str) -> Self {
        match value {
            "UNINIT" => JadeState::Uninit,
            "UNSAVED" => JadeState::Unsaved,
            "LOCKED" => JadeState::Locked,
            "READY" => JadeState::Ready,
            "TEMP" => JadeState::Temp,
            _ => JadeState::Unknown,
        }
    }
}

/// The result of `ping`.
///
/// Modelled as an enum rather than the raw `u8` the device sends. It documents
/// the three states, and it keeps this module clear of unsigned 8 and 16 bit
/// FFI returns, which needed a binding-generator fix to work on Android ARM32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JadePingStatus {
    Idle,
    Busy,
    AwaitingUserInput,
}

impl JadePingStatus {
    pub(crate) fn from_wire(value: u64) -> Self {
        match value {
            0 => JadePingStatus::Idle,
            1 => JadePingStatus::Busy,
            _ => JadePingStatus::AwaitingUserInput,
        }
    }
}

/// A device discovered by a scan.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JadeDeviceInfo {
    /// Stable identifier passed to `jade_connect`.
    ///
    /// Formed as `{transport}:{path}` so an Android USB host path and a Rust
    /// enumerated serial path cannot collide and send a connect to the wrong
    /// transport.
    pub id: String,
    pub transport: JadeTransportKind,
    /// Advertised or descriptor name, for example "Jade C0FFEE".
    pub name: Option<String>,
    /// Transport specific address: a BLE identifier or a serial device path.
    pub path: String,
    pub serial_number: Option<String>,
}

impl JadeDeviceInfo {
    pub(crate) fn build_id(transport: JadeTransportKind, path: &str) -> String {
        format!("{}:{}", transport.as_str(), path)
    }

    /// Split an id back into its transport and path.
    pub(crate) fn parse_id(id: &str) -> Option<(JadeTransportKind, &str)> {
        let (kind, path) = id.split_once(':')?;
        Some((JadeTransportKind::from_str(kind)?, path))
    }
}

/// Device firmware and state summary.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JadeVersionInfo {
    pub jade_version: String,
    pub jade_state: JadeState,
    /// "ALL", "MAIN" or "TEST": which networks this unit is locked to.
    pub jade_networks: Option<String>,
    pub jade_has_pin: Option<bool>,
    pub board_type: Option<String>,
    pub jade_config: Option<String>,
    pub jade_features: Option<String>,
    pub idf_version: Option<String>,
    pub chip_features: Option<String>,
    pub efuse_mac: Option<String>,
    /// Battery bucket, 0 to 5. Widened from the wire's small integer so this
    /// crate exposes no unsigned 8 bit types over FFI.
    pub battery_status: Option<u32>,
    pub jade_ota_max_chunk: Option<u32>,
}

/// The wire shape of `get_version_info`, kept separate from the FFI record.
#[derive(Debug, Deserialize)]
pub(crate) struct WireVersionInfo {
    #[serde(rename = "JADE_VERSION")]
    pub jade_version: Option<String>,
    #[serde(rename = "JADE_STATE")]
    pub jade_state: Option<String>,
    #[serde(rename = "JADE_NETWORKS")]
    pub jade_networks: Option<String>,
    #[serde(rename = "JADE_HAS_PIN")]
    pub jade_has_pin: Option<bool>,
    #[serde(rename = "BOARD_TYPE")]
    pub board_type: Option<String>,
    #[serde(rename = "JADE_CONFIG")]
    pub jade_config: Option<String>,
    #[serde(rename = "JADE_FEATURES")]
    pub jade_features: Option<String>,
    #[serde(rename = "IDF_VERSION")]
    pub idf_version: Option<String>,
    #[serde(rename = "CHIP_FEATURES")]
    pub chip_features: Option<String>,
    #[serde(rename = "EFUSEMAC")]
    pub efuse_mac: Option<String>,
    #[serde(rename = "BATTERY_STATUS")]
    pub battery_status: Option<u32>,
    #[serde(rename = "JADE_OTA_MAX_CHUNK")]
    pub jade_ota_max_chunk: Option<u32>,
}

impl From<WireVersionInfo> for JadeVersionInfo {
    fn from(wire: WireVersionInfo) -> Self {
        JadeVersionInfo {
            jade_version: wire.jade_version.unwrap_or_default(),
            jade_state: wire
                .jade_state
                .as_deref()
                .map(JadeState::from_wire)
                .unwrap_or(JadeState::Unknown),
            jade_networks: wire.jade_networks,
            jade_has_pin: wire.jade_has_pin,
            board_type: wire.board_type,
            jade_config: wire.jade_config,
            jade_features: wire.jade_features,
            idf_version: wire.idf_version,
            chip_features: wire.chip_features,
            efuse_mac: wire.efuse_mac,
            battery_status: wire.battery_status,
            jade_ota_max_chunk: wire.jade_ota_max_chunk,
        }
    }
}

/// An extended public key, echoed back with the request it answers.
///
/// The path and fingerprint travel with the key so the caller can confirm the
/// device answered the question that was asked.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JadeXpubResponse {
    pub xpub: String,
    pub derivation_path: String,
    /// Master fingerprint, eight lowercase hex characters.
    pub master_fingerprint: String,
}

/// One account within an export.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JadeAccount {
    pub account_type: AccountType,
    pub xpub: String,
    pub derivation_path: String,
}

/// A multi-account export, shaped like `PassportAccountExport` so applications
/// have one import path for both signers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JadeAccountExport {
    pub master_fingerprint: String,
    pub account_index: u32,
    pub accounts: Vec<JadeAccount>,
}

/// A signed message, with the address that verifies it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JadeSignedMessage {
    /// Base64 encoded recoverable signature.
    pub signature: String,
    /// Address derived from the signing path, for verification.
    pub address: String,
    pub derivation_path: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeGetXpubParams {
    pub network: JadeNetwork,
    pub derivation_path: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeVerifyAddressParams {
    pub network: JadeNetwork,
    pub variant: JadeAddressVariant,
    pub derivation_path: String,
    /// The address the application is about to display. The device is asked to
    /// show its own derivation, and the two are compared.
    pub expected_address: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeSignMessageParams {
    pub derivation_path: String,
    pub message: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeSignPsbtParams {
    pub network: JadeNetwork,
    /// Base64 encoded PSBT. The signed PSBT comes back base64 encoded too, so
    /// it feeds straight into `finalize_psbt`.
    pub psbt: String,
}
