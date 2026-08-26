//! Trezor callback infrastructure for native transport and UI operations.
//!
//! These types and traits bridge the Rust trezor-connect-rs library with
//! native iOS/Android implementations via UniFFI callback interfaces.

use once_cell::sync::OnceCell;
use std::sync::Arc;

// ============================================================================
// Transport callback types
// ============================================================================

/// Structured transport error code returned by native callback operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrezorTransportErrorCode {
    /// Device is busy and the caller should back off before retrying.
    DeviceBusy,
}

/// Result from a transport read operation
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorTransportReadResult {
    /// Whether the read succeeded
    pub success: bool,
    /// Data read (empty on failure)
    pub data: Vec<u8>,
    /// Error message (empty on success)
    pub error: String,
    /// Structured error code (None on success or when the native error is generic)
    pub error_code: Option<TrezorTransportErrorCode>,
}

/// Result from a transport write or open operation
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorTransportWriteResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message (empty on success)
    pub error: String,
    /// Structured error code (None on success or when the native error is generic)
    pub error_code: Option<TrezorTransportErrorCode>,
}

/// Result from a high-level message call (for BLE/THP devices)
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrezorCallMessageResult {
    /// Whether the call succeeded
    pub success: bool,
    /// Response message type
    pub message_type: u16,
    /// Response protobuf data
    pub data: Vec<u8>,
    /// Error message (empty on success)
    pub error: String,
    /// Structured error code (None on success or when the native error is generic)
    pub error_code: Option<TrezorTransportErrorCode>,
}

/// Native device information returned from enumeration
#[derive(Debug, Clone, uniffi::Record)]
pub struct NativeDeviceInfo {
    /// Unique path/identifier for this device
    pub path: String,
    /// Transport type: "usb" or "bluetooth"
    pub transport_type: String,
    /// Optional device name (from BLE advertisement or USB descriptor)
    pub name: Option<String>,
    /// USB Vendor ID (for USB devices)
    pub vendor_id: Option<u16>,
    /// USB Product ID (for USB devices)
    pub product_id: Option<u16>,
}

// ============================================================================
// Callback traits
// ============================================================================

/// Callback interface for native Trezor transport operations
///
/// This trait must be implemented by the native iOS/Android code.
/// The implementation handles actual USB or Bluetooth communication.
///
/// # Android Implementation
/// Use Android USB Host API for USB devices:
/// - Enumerate devices with vendorId 0x1209 (0x534c for older), productId 0x53c1
/// - Request USB permission, claim interface, get endpoints
/// - Chunk size: 64 bytes for USB
///
/// Use Android BLE API for Bluetooth:
/// - Scan for Trezor BLE service UUID: 8c000001-a59b-4d58-a9ad-073df69fa1b1
/// - Connect and discover characteristics
/// - Read from: 8c000002-a59b-4d58-a9ad-073df69fa1b1
/// - Write to: 8c000003-a59b-4d58-a9ad-073df69fa1b1
/// - Chunk size: 244 bytes for BLE
///
/// # iOS Implementation
/// Use IOKit/CoreBluetooth with same service/characteristic UUIDs.
#[uniffi::export(with_foreign)]
pub trait TrezorTransportCallback: Send + Sync {
    /// Enumerate all connected Trezor devices
    fn enumerate_devices(&self) -> Vec<NativeDeviceInfo>;

    /// Open a connection to a device
    fn open_device(&self, path: String) -> TrezorTransportWriteResult;

    /// Close the connection to a device
    fn close_device(&self, path: String) -> TrezorTransportWriteResult;

    /// Read a chunk of data from the device
    fn read_chunk(&self, path: String) -> TrezorTransportReadResult;

    /// Write a chunk of data to the device
    fn write_chunk(&self, path: String, data: Vec<u8>) -> TrezorTransportWriteResult;

    /// Get the chunk size for a device (64 for USB, 244 for Bluetooth)
    fn get_chunk_size(&self, path: String) -> u32;

    /// High-level message call for BLE/THP devices.
    ///
    /// For BLE devices that use THP protocol (encrypted communication),
    /// the native layer should handle encryption/decryption via
    /// android-trezor-connect and return the raw protobuf response.
    ///
    /// Returns None if not supported (will fall back to Protocol V1 chunks).
    /// Returns Some(result) to use native THP handling.
    ///
    /// # Arguments
    /// * `path` - Device path
    /// * `message_type` - Protobuf message type (e.g., GetAddress = 29)
    /// * `data` - Serialized protobuf message data
    fn call_message(
        &self,
        path: String,
        message_type: u16,
        data: Vec<u8>,
    ) -> Option<TrezorCallMessageResult>;

    /// Get pairing code from user during BLE THP pairing.
    ///
    /// This is called when the Trezor device displays a 6-digit code
    /// that must be entered to complete Bluetooth pairing.
    ///
    /// The native layer should display a UI for the user to enter the code
    /// shown on the Trezor screen.
    ///
    /// Returns the 6-digit code as a string, or empty string to cancel.
    fn get_pairing_code(&self) -> String;

    /// Save THP pairing credentials for a device.
    ///
    /// Called after successful BLE pairing to store credentials for reconnection.
    /// The credential_json is a JSON string containing the serialized ThpCredentials.
    ///
    /// # Arguments
    /// * `device_id` - Device identifier (e.g., BLE address like "ble:AA:BB:CC:DD:EE:FF")
    /// * `credential_json` - JSON string with credential data
    ///
    /// Returns true if credentials were saved successfully.
    fn save_thp_credential(&self, device_id: String, credential_json: String) -> bool;

    /// Load THP pairing credentials for a device.
    ///
    /// Called before BLE handshake to check for stored credentials.
    /// If credentials are found, they will be used to skip the pairing dialog.
    ///
    /// # Arguments
    /// * `device_id` - Device identifier (e.g., BLE address like "ble:AA:BB:CC:DD:EE:FF")
    ///
    /// Returns the JSON string containing ThpCredentials, or None if not found.
    fn load_thp_credential(&self, device_id: String) -> Option<String>;

    /// Log a debug message from the Rust THP handshake layer.
    ///
    /// This forwards Rust-level errors and state information to the native
    /// debug UI (e.g., TrezorDebugLog on Android) so they are visible
    /// alongside the Kotlin-level logs.
    ///
    /// Both arguments are redacted by bitkit-core before this is called:
    /// credentials, keys, PSBTs, raw frame hex and anything else that looks
    /// like key material are replaced with `<redacted>`, and each string is
    /// length-capped. Consumers do not need their own scrubbing pass. See
    /// `log_sanitizer` for the exact policy.
    ///
    /// # Arguments
    /// * `tag` - Short tag identifying the subsystem (e.g., "HANDSHAKE", "THP")
    /// * `message` - Human-readable debug message
    fn log_debug(&self, tag: String, message: String);
}

// ============================================================================
// UI callback trait
// ============================================================================

/// Re-export the upstream `PassphraseResponse` and attach UniFFI scaffolding
/// to it via `#[uniffi::remote(Enum)]`. trezor-connect-rs intentionally does
/// not depend on uniffi, so we add the bindings metadata externally here.
/// The variant list below is parsed by the macro but not redefined as a type
/// — `PassphraseResponse` in scope resolves to the upstream enum.
///
/// NOTE: the variant list below must match `trezor_connect_rs::PassphraseResponse`
/// exactly (currently trezor-connect-rs 0.3.x). If a future bump reshapes the
/// upstream enum, update these variants in lockstep — the adapter tests in
/// `tests.rs` (`test_passphrase_adapter_*`) guard the variant-for-variant mapping.
pub use trezor_connect_rs::PassphraseResponse;

#[uniffi::remote(Enum)]
pub enum PassphraseResponse {
    /// User cancelled — aborts the pending operation.
    Cancel,
    /// Standard wallet — no passphrase, equivalent to `Some("")` on the device.
    Standard,
    /// Hidden wallet — derived from the passphrase entered on the host.
    Hidden { value: String },
    /// Enter the passphrase on the Trezor device itself instead of on the host.
    OnDevice,
}

/// Callback interface for handling PIN and passphrase requests from the Trezor device.
///
/// The native layer (iOS/Android) should implement this to show PIN/passphrase
/// input UI when the device requests it during operations like signing.
#[uniffi::export(with_foreign)]
pub trait TrezorUiCallback: Send + Sync {
    /// Called when the device requests a PIN.
    ///
    /// Show a PIN matrix UI and return the matrix-encoded PIN string.
    /// Return empty string to cancel.
    fn on_pin_request(&self) -> String;

    /// Called when the device requests a passphrase.
    ///
    /// If `on_device` is true, the device is asking for the passphrase to be
    /// entered on the Trezor itself — return `PassphraseResponse::OnDevice`.
    ///
    /// If `on_device` is false, show a passphrase input UI and return
    /// `Standard` (no passphrase), `Hidden { value }` (host-entered passphrase),
    /// `OnDevice` (defer entry to the Trezor), or `Cancel`.
    fn on_passphrase_request(&self, on_device: bool) -> PassphraseResponse;
}

// ============================================================================
// Global callback holders
// ============================================================================

/// Global transport callback holder
static TRANSPORT_CALLBACK: OnceCell<Arc<dyn TrezorTransportCallback>> = OnceCell::new();

/// Global UI callback holder
static UI_CALLBACK: OnceCell<Arc<dyn TrezorUiCallback>> = OnceCell::new();

/// Initialize the Trezor transport with a native callback implementation
///
/// This must be called before any Trezor scanning/connection operations.
/// The native layer (iOS/Android) must implement the TrezorTransportCallback interface.
#[uniffi::export]
pub fn trezor_set_transport_callback(callback: Arc<dyn TrezorTransportCallback>) {
    #[cfg(target_os = "android")]
    crate::init_android_logger();
    let _ = TRANSPORT_CALLBACK.set(callback);
}

/// Get the transport callback (internal use)
pub fn get_transport_callback() -> Option<&'static Arc<dyn TrezorTransportCallback>> {
    TRANSPORT_CALLBACK.get()
}

/// Set the UI callback for handling PIN and passphrase requests.
///
/// This should be called before connecting to a Trezor device if you want
/// the library to handle PIN/passphrase requests via your UI instead of
/// returning errors.
#[uniffi::export]
pub fn trezor_set_ui_callback(callback: Arc<dyn TrezorUiCallback>) {
    let _ = UI_CALLBACK.set(callback);
}

/// Get the UI callback (internal use)
pub fn get_ui_callback() -> Option<&'static Arc<dyn TrezorUiCallback>> {
    UI_CALLBACK.get()
}

/// Which wallet a connection should open.
///
/// Passed to `trezor_connect` and consumed at connect time — the passphrase is
/// a one-shot input, not retained anywhere afterwards. On THP devices (Safe
/// 5/7) it is bound to the session at `ThpCreateNewSession`; on legacy devices
/// the mid-operation `PassphraseRequest` is answered from the UI callback
/// instead (see [`TrezorUiCallback`]).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum WalletSelection {
    /// The standard wallet — no passphrase.
    Standard,
    /// A hidden wallet whose passphrase is entered on the host.
    Hidden { passphrase: String },
    /// A hidden wallet whose passphrase is entered on the Trezor itself.
    OnDevice,
}

impl Default for WalletSelection {
    fn default() -> Self {
        WalletSelection::Standard
    }
}

// Only the callback (mobile) transport binds the passphrase at connect time;
// the desktop path supplies it via the UI callback instead.
#[cfg(any(target_os = "android", target_os = "ios"))]
use zeroize::Zeroizing;

#[cfg(any(target_os = "android", target_os = "ios"))]
impl WalletSelection {
    /// Lower the selection to the `(passphrase, on_device)` pair the THP
    /// transport binds at session creation. `OnDevice` carries no host
    /// passphrase: the transport omits the field so the Trezor prompts on its
    /// own screen (firmware rejects an empty passphrase combined with
    /// `on_device = true`).
    ///
    /// The passphrase is moved into a `Zeroizing<String>` so this crate's copy
    /// is wiped from memory on drop instead of lingering in a freed allocation.
    pub(crate) fn into_session_passphrase(self) -> (Zeroizing<String>, bool) {
        match self {
            WalletSelection::Standard => (Zeroizing::new(String::new()), false),
            WalletSelection::Hidden { passphrase } => (Zeroizing::new(passphrase), false),
            WalletSelection::OnDevice => (Zeroizing::new(String::new()), true),
        }
    }
}

// ============================================================================
// BLE initialization
// ============================================================================

/// Track if BLE has been initialized on Android
#[cfg(target_os = "android")]
static BLE_INITIALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Check if BLE has been initialized.
///
/// On Android: Returns true if BluetoothInit.nativeInit() was called successfully.
/// On other platforms: Always returns true (BLE works natively).
#[uniffi::export]
pub fn trezor_is_ble_available() -> bool {
    #[cfg(target_os = "android")]
    {
        use std::sync::atomic::Ordering;
        BLE_INITIALIZED.load(Ordering::SeqCst)
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

/// Mark BLE as initialized (called from JNI on Android)
#[cfg(target_os = "android")]
pub fn set_ble_initialized(value: bool) {
    use std::sync::atomic::Ordering;
    BLE_INITIALIZED.store(value, Ordering::SeqCst);
}

/// Check if BLE is initialized (used by JNI function in lib.rs)
#[cfg(target_os = "android")]
pub fn is_ble_initialized() -> bool {
    use std::sync::atomic::Ordering;
    BLE_INITIALIZED.load(Ordering::SeqCst)
}
