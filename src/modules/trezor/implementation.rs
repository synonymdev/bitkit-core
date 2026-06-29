//! Trezor manager implementation.
//!
//! On desktop platforms, this uses the trezor-connect-rs library directly.
//! On Android/iOS, this uses a callback-based transport implemented natively.

use base64::{engine::general_purpose, Engine as _};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::modules::trezor::{
    TrezorAddressResponse, TrezorCoinType, TrezorDeviceInfo, TrezorError, TrezorFeatures,
    TrezorGetAddressParams, TrezorGetPublicKeyParams, TrezorPublicKeyResponse,
    TrezorSignMessageParams, TrezorSignTxParams, TrezorSignedMessageResponse, TrezorSignedTx,
    TrezorTransportType, TrezorVerifyMessageParams, WalletSelection,
};

#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::modules::trezor::encode_callback_transport_error;

// Desktop: use full trezor-connect-rs
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use trezor_connect_rs::{
    ConnectedDevice, DeviceInfo, GetAddressParams, GetPublicKeyParams, SignMessageParams,
    SignTxParams, Trezor, VerifyMessageParams,
};

// Mobile: use callback transport from trezor-connect-rs
#[cfg(any(target_os = "android", target_os = "ios"))]
use trezor_connect_rs::{
    CallbackDeviceInfo, CallbackMessageResult, CallbackReadResult, CallbackResult,
    CallbackTransport, ConnectedDevice, GetAddressParams, GetPublicKeyParams, SignMessageParams,
    SignTxParams, Transport, TransportCallback, VerifyMessageParams,
};

#[cfg(any(target_os = "android", target_os = "ios"))]
use trezor_connect_rs::device_info::{DeviceInfo, TransportType};

/// Validate a BIP32 derivation path.
///
/// Valid paths must:
/// - Start with "m/"
/// - Contain only numeric indices (with optional ' or h for hardened)
/// - Have a known purpose for Bitcoin (44, 49, 84, 86)
///
/// # Examples
/// - `m/84'/0'/0'/0/0` - valid native SegWit path
/// - `m/44'/0'/0'/0/0` - valid legacy path
/// - `m/49'/0'/0'/0/0` - valid nested SegWit path
/// - `m/86'/0'/0'/0/0` - valid Taproot path
pub(crate) fn validate_derivation_path(path: &str) -> Result<(), TrezorError> {
    // Must start with "m/"
    if !path.starts_with("m/") {
        return Err(TrezorError::InvalidPath {
            error_details: format!("Path must start with 'm/': {}", path),
        });
    }

    let path_part = &path[2..]; // Skip "m/"
    if path_part.is_empty() {
        return Err(TrezorError::InvalidPath {
            error_details: "Path cannot be empty after 'm/'".to_string(),
        });
    }

    let components: Vec<&str> = path_part.split('/').collect();

    // Validate each component
    for (i, component) in components.iter().enumerate() {
        if component.is_empty() {
            return Err(TrezorError::InvalidPath {
                error_details: format!("Empty path component at index {}", i),
            });
        }

        // Check if it's hardened (ends with ' or h)
        let num_str = if component.ends_with('\'') || component.ends_with('h') {
            &component[..component.len() - 1]
        } else {
            *component
        };

        // Must be a valid number
        if num_str.parse::<u32>().is_err() {
            return Err(TrezorError::InvalidPath {
                error_details: format!(
                    "Invalid path component '{}' at index {}: must be a number",
                    component, i
                ),
            });
        }
    }

    // Note: We intentionally don't reject unknown purposes (like 48 for multisig)
    // to maintain flexibility for advanced use cases. The device itself will
    // validate if the path is appropriate for the requested operation.

    Ok(())
}

/// Validate sign transaction parameters before sending to the device.
///
/// Checks input paths, script types, output structure, and change output paths.
/// Used by both `sign_tx` and `sign_tx_from_psbt` to ensure consistent validation.
fn validate_sign_tx_params(params: &SignTxParams) -> Result<(), TrezorError> {
    // Validate all input paths
    for (i, input) in params.inputs.iter().enumerate() {
        validate_derivation_path(&input.path).map_err(|e| match e {
            TrezorError::InvalidPath { error_details } => TrezorError::InvalidPath {
                error_details: format!("Input {}: {}", i, error_details),
            },
            other => other,
        })?;
    }

    // Reject unsupported input script types
    for (i, input) in params.inputs.iter().enumerate() {
        match input.script_type {
            trezor_connect_rs::ScriptType::SpendMultisig => {
                return Err(TrezorError::DeviceError {
                    error_details: format!(
                        "Input {}: Multisig inputs are not currently supported.",
                        i
                    ),
                })
            }
            trezor_connect_rs::ScriptType::External => {
                return Err(TrezorError::DeviceError {
                    error_details: format!(
                        "Input {}: External inputs are not currently supported.",
                        i
                    ),
                })
            }
            _ => {}
        }
    }

    // Validate at least 1 input and 1 output
    if params.inputs.is_empty() {
        return Err(TrezorError::DeviceError {
            error_details: "Transaction must have at least one input.".to_string(),
        });
    }
    if params.outputs.is_empty() {
        return Err(TrezorError::DeviceError {
            error_details: "Transaction must have at least one output.".to_string(),
        });
    }

    // Validate each output is exactly one mode: External, Change, or OpReturn
    for (i, output) in params.outputs.iter().enumerate() {
        let has_address = output.address.is_some();
        let has_path = output.path.is_some();
        let has_op_return = output.op_return_data.is_some();

        match (has_address, has_path, has_op_return) {
            (true, false, false) => {
                // External output - valid
            }
            (false, true, false) => {
                // Change output - must have script_type
                if output.script_type.is_none() {
                    return Err(TrezorError::DeviceError {
                        error_details: format!(
                            "Output {}: change output must specify script_type.",
                            i
                        ),
                    });
                }
            }
            (false, false, true) => {
                // OP_RETURN output - amount must be 0
                if output.amount != 0 {
                    return Err(TrezorError::DeviceError {
                        error_details: format!(
                            "Output {}: OP_RETURN output must have amount 0.",
                            i
                        ),
                    });
                }
            }
            _ => {
                return Err(TrezorError::DeviceError {
                    error_details: format!(
                        "Output {}: must be exactly one of: external (address only), change (path only), or OP_RETURN (op_return_data only).",
                        i
                    ),
                });
            }
        }
    }

    // Validate change output paths
    for (i, output) in params.outputs.iter().enumerate() {
        if let Some(ref path) = output.path {
            validate_derivation_path(path).map_err(|e| match e {
                TrezorError::InvalidPath { error_details } => TrezorError::InvalidPath {
                    error_details: format!("Output {} (change): {}", i, error_details),
                },
                other => other,
            })?;
        }
    }

    Ok(())
}

/// Cached native device info for callback-based transport
#[derive(Clone, Debug)]
pub(crate) struct CachedNativeDevice {
    pub(crate) path: String,
    pub(crate) transport_type: TrezorTransportType,
    pub(crate) name: Option<String>,
}

/// Adapter that implements trezor_connect_rs::TransportCallback using the UniFFI callback.
/// This bridges the trezor-connect-rs library with the native Kotlin/Swift callback.
#[cfg(any(target_os = "android", target_os = "ios"))]
struct CallbackAdapter {
    callback: Arc<dyn crate::TrezorTransportCallback>,
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl TransportCallback for CallbackAdapter {
    fn enumerate_devices(&self) -> Vec<CallbackDeviceInfo> {
        let devices = self.callback.enumerate_devices();
        devices
            .into_iter()
            .map(|d| CallbackDeviceInfo {
                path: d.path,
                transport_type: d.transport_type,
                name: d.name,
                vendor_id: d.vendor_id,
                product_id: d.product_id,
            })
            .collect()
    }

    fn open_device(&self, path: &str) -> CallbackResult {
        let result = self.callback.open_device(path.to_string());
        CallbackResult {
            success: result.success,
            error: encode_callback_transport_error(result.error, result.error_code),
        }
    }

    fn close_device(&self, path: &str) -> CallbackResult {
        let result = self.callback.close_device(path.to_string());
        CallbackResult {
            success: result.success,
            error: encode_callback_transport_error(result.error, result.error_code),
        }
    }

    fn read_chunk(&self, path: &str) -> CallbackReadResult {
        let result = self.callback.read_chunk(path.to_string());
        CallbackReadResult {
            success: result.success,
            data: result.data,
            error: encode_callback_transport_error(result.error, result.error_code),
        }
    }

    fn write_chunk(&self, path: &str, data: &[u8]) -> CallbackResult {
        let result = self.callback.write_chunk(path.to_string(), data.to_vec());
        CallbackResult {
            success: result.success,
            error: encode_callback_transport_error(result.error, result.error_code),
        }
    }

    fn get_chunk_size(&self, path: &str) -> u32 {
        self.callback.get_chunk_size(path.to_string())
    }

    fn call_message(
        &self,
        path: &str,
        message_type: u16,
        data: &[u8],
    ) -> Option<CallbackMessageResult> {
        // Call the native layer's call_message implementation
        let result = self
            .callback
            .call_message(path.to_string(), message_type, data.to_vec());

        // Convert from TrezorCallMessageResult to CallbackMessageResult
        result.map(|r| CallbackMessageResult {
            success: r.success,
            message_type: r.message_type,
            data: r.data,
            error: encode_callback_transport_error(r.error, r.error_code),
        })
    }

    fn get_pairing_code(&self) -> String {
        // Call the native layer to get the pairing code from the user
        self.callback.get_pairing_code()
    }

    fn save_thp_credential(&self, device_id: &str, credential_json: &str) -> bool {
        self.callback
            .save_thp_credential(device_id.to_string(), credential_json.to_string())
    }

    fn load_thp_credential(&self, device_id: &str) -> Option<String> {
        self.callback.load_thp_credential(device_id.to_string())
    }

    fn log_debug(&self, tag: &str, message: &str) {
        self.callback
            .log_debug(tag.to_string(), message.to_string());
    }
}

/// Adapter bridging bitkit-core's UniFFI-exported `TrezorUiCallback` to
/// trezor-connect-rs's `TrezorUiCallback`.
///
/// PIN: empty string → `None` (cancel), non-empty → `Some(value)`. (No
/// upstream enum exists for PIN; we keep the legacy `String → Option` encoding.)
///
/// Passphrase: a pure pass-through. Both traits return the same upstream
/// `PassphraseResponse` type — bitkit-core attaches UniFFI scaffolding to it
/// via `#[uniffi::remote(Enum)]` (see `callbacks.rs`).
pub(super) struct UiCallbackAdapter {
    pub(super) callback: Arc<dyn crate::TrezorUiCallback>,
}

impl trezor_connect_rs::TrezorUiCallback for UiCallbackAdapter {
    fn on_pin_request(&self) -> Option<String> {
        let result = self.callback.on_pin_request();
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    fn on_passphrase_request(&self, on_device: bool) -> trezor_connect_rs::PassphraseResponse {
        self.callback.on_passphrase_request(on_device)
    }
}

/// Main Trezor manager.
///
/// On desktop: uses trezor-connect-rs library for USB/Bluetooth.
/// On mobile: uses native callbacks for USB/Bluetooth I/O.
pub struct TrezorManager {
    /// The underlying trezor-connect-rs manager (desktop only)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    inner: Mutex<Option<Trezor>>,

    /// Currently connected device (desktop only)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    connected_device: Mutex<Option<ConnectedDevice>>,

    /// Cached device list from last scan (desktop only)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    device_list: Mutex<Vec<DeviceInfo>>,

    /// Cached native device list (mobile only)
    #[cfg(any(target_os = "android", target_os = "ios"))]
    native_device_list: Mutex<Vec<CachedNativeDevice>>,

    /// Currently connected device path (mobile only)
    #[cfg(any(target_os = "android", target_os = "ios"))]
    connected_path: Mutex<Option<String>>,

    /// Currently connected device (mobile only) - reused for all operations
    #[cfg(any(target_os = "android", target_os = "ios"))]
    connected_device: Mutex<Option<ConnectedDevice>>,

    /// Initialization state
    initialized: Mutex<bool>,
}

impl TrezorManager {
    /// Create a new TrezorManager instance.
    pub fn new() -> Self {
        Self {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            inner: Mutex::new(None),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            connected_device: Mutex::new(None),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            device_list: Mutex::new(Vec::new()),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            native_device_list: Mutex::new(Vec::new()),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            connected_path: Mutex::new(None),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            connected_device: Mutex::new(None),
            initialized: Mutex::new(false),
        }
    }

    /// Initialize the Trezor manager.
    ///
    /// On mobile: Verifies that the transport callback is set.
    /// On desktop: Initializes trezor-connect-rs library.
    pub async fn initialize(&self, _credential_path: Option<String>) -> Result<(), TrezorError> {
        // Mobile: Just verify callback is set
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            use crate::get_transport_callback;
            if get_transport_callback().is_none() {
                return Err(TrezorError::NotInitialized);
            }
            let mut initialized = self.initialized.lock().await;
            *initialized = true;
            Ok(())
        }

        // Desktop: Initialize trezor-connect-rs
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let mut builder = Trezor::new().with_app_identity("Bitkit", "Bitkit");

            if let Some(ref path) = _credential_path {
                builder = builder.with_credential_store(path);
            }

            let trezor = builder.build().await.map_err(TrezorError::from)?;

            let mut inner = self.inner.lock().await;
            *inner = Some(trezor);

            let mut initialized = self.initialized.lock().await;
            *initialized = true;

            Ok(())
        }
    }

    /// Check if the manager is initialized.
    pub async fn is_initialized(&self) -> bool {
        *self.initialized.lock().await
    }

    /// Scan for available Trezor devices (USB + Bluetooth).
    ///
    /// On mobile: Uses native callback to enumerate devices.
    /// On desktop: Uses trezor-connect-rs library.
    pub async fn scan(&self) -> Result<Vec<TrezorDeviceInfo>, TrezorError> {
        if !*self.initialized.lock().await {
            return Err(TrezorError::NotInitialized);
        }

        // Mobile: Use callback-based enumeration
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            use crate::get_transport_callback;

            let callback = get_transport_callback().ok_or(TrezorError::NotInitialized)?;

            let native_devices = callback.enumerate_devices();

            // Cache the device list
            let mut device_list = self.native_device_list.lock().await;
            device_list.clear();

            let result: Vec<TrezorDeviceInfo> = native_devices
                .into_iter()
                .map(|d| {
                    let transport_type = if d.transport_type == "bluetooth" {
                        TrezorTransportType::Bluetooth
                    } else {
                        TrezorTransportType::Usb
                    };

                    // Cache for later connection
                    device_list.push(CachedNativeDevice {
                        path: d.path.clone(),
                        transport_type: transport_type.clone(),
                        name: d.name.clone(),
                    });

                    TrezorDeviceInfo {
                        id: d.path.clone(),
                        transport_type,
                        name: d.name,
                        path: d.path,
                        label: None,
                        model: None,
                        is_bootloader: false,
                    }
                })
                .collect();

            Ok(result)
        }

        // Desktop: Use trezor-connect-rs
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let mut inner = self.inner.lock().await;
            let trezor = inner.as_mut().ok_or(TrezorError::NotInitialized)?;

            let devices = trezor.scan().await.map_err(TrezorError::from)?;

            // Cache the device list for later connection
            let mut device_list = self.device_list.lock().await;
            *device_list = devices.clone();

            Ok(devices.into_iter().map(TrezorDeviceInfo::from).collect())
        }
    }

    /// List previously discovered devices without triggering a new scan.
    pub async fn list_devices(&self) -> Result<Vec<TrezorDeviceInfo>, TrezorError> {
        if !*self.initialized.lock().await {
            return Err(TrezorError::NotInitialized);
        }

        // Mobile: Return cached devices
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let device_list = self.native_device_list.lock().await;
            Ok(device_list
                .iter()
                .map(|d| TrezorDeviceInfo {
                    id: d.path.clone(),
                    transport_type: d.transport_type.clone(),
                    name: d.name.clone(),
                    path: d.path.clone(),
                    label: None,
                    model: None,
                    is_bootloader: false,
                })
                .collect())
        }

        // Desktop: Use trezor-connect-rs
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let inner = self.inner.lock().await;
            let trezor = inner.as_ref().ok_or(TrezorError::NotInitialized)?;

            let devices = trezor.list_devices().await.map_err(TrezorError::from)?;

            Ok(devices.into_iter().map(TrezorDeviceInfo::from).collect())
        }
    }

    /// Connect to a device by its ID, opening the wallet given by `selection`.
    ///
    /// On mobile: Opens device connection via native callback. For THP devices
    /// the `selection` passphrase is bound to the session created during
    /// `acquire()`.
    /// On desktop: Uses trezor-connect-rs library; the passphrase is supplied
    /// via the UI callback, so `selection` is not consumed on that path.
    pub async fn connect(
        &self,
        device_id: &str,
        selection: WalletSelection,
    ) -> Result<TrezorFeatures, TrezorError> {
        if !*self.initialized.lock().await {
            return Err(TrezorError::NotInitialized);
        }

        // Mobile: Use callback-based connection
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            use crate::get_transport_callback;

            let callback = get_transport_callback().ok_or(TrezorError::NotInitialized)?;

            // Find the device in cached list
            let device = {
                let device_list = self.native_device_list.lock().await;
                device_list
                    .iter()
                    .find(|d| d.path == device_id)
                    .cloned()
                    .ok_or(TrezorError::DeviceNotFound)?
            };

            // Create transport and connect - this will be reused for all operations
            let adapter = Arc::new(CallbackAdapter {
                callback: callback.clone(),
            });
            // Bind the selected wallet's passphrase to the THP session that
            // acquire() creates. On THP devices the passphrase is set at session
            // creation, not via a mid-operation prompt. We hold our copy in
            // `Zeroizing` so it is wiped on drop (including error paths); the
            // transport re-wraps the value it receives in `Zeroizing` as well.
            let (session_passphrase, session_on_device) = selection.into_session_passphrase();
            let mut transport = CallbackTransport::new(adapter)
                .with_app_identity("Bitkit", "Bitkit")
                .with_session_passphrase(session_passphrase.to_string(), session_on_device);
            transport.init().await.map_err(TrezorError::from)?;

            // Acquire a session (this triggers THP handshake for BLE)
            let session = transport
                .acquire(&device.path, None)
                .await
                .map_err(TrezorError::from)?;

            // Store connected path
            {
                let mut connected_path = self.connected_path.lock().await;
                *connected_path = Some(device.path.clone());
            }

            // Determine transport type from cached device info
            let transport_type = match device.transport_type {
                TrezorTransportType::Bluetooth => TransportType::Bluetooth,
                TrezorTransportType::Usb => TransportType::Usb,
            };

            // Create device info
            let device_info = DeviceInfo {
                id: device.path.clone(),
                transport_type,
                name: device.name.clone(),
                path: device.path.clone(),
                label: None,
                model: None,
                is_bootloader: false,
            };

            // Check if the transport negotiated THP during acquire (BLE always,
            // USB for newer devices like Safe 7).
            let uses_thp = transport.has_thp(&device.path).await;

            // Create connected device and initialize it
            let mut connected = ConnectedDevice::new(device_info, Box::new(transport), session);

            // When THP was negotiated, initialize() must use GetFeatures instead
            // of Initialize (the THP session was already created during acquire
            // via ThpCreateNewSession).
            if uses_thp {
                connected.set_uses_thp(true);
            }

            // Wire UI callback if set
            if let Some(ui_cb) = crate::modules::trezor::get_ui_callback() {
                let adapter = Arc::new(UiCallbackAdapter {
                    callback: ui_cb.clone(),
                });
                connected.set_ui_callback(adapter);
            }

            let features = connected.initialize().await.map_err(TrezorError::from)?;

            // Store the connected device for reuse
            {
                let mut stored_device = self.connected_device.lock().await;
                *stored_device = Some(connected);
            }

            Ok(TrezorFeatures::from(features))
        }

        // Desktop: Use trezor-connect-rs
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            // Desktop binds the passphrase through the UI callback
            // (on_passphrase_request), not at connect time, so the selection is
            // not consumed on this path.
            let _ = selection;

            // Find the device in the cached list
            let device = {
                let device_list = self.device_list.lock().await;
                device_list
                    .iter()
                    .find(|d| d.id == device_id)
                    .cloned()
                    .ok_or(TrezorError::DeviceNotFound)?
            };

            // Connect to the device
            let mut inner = self.inner.lock().await;
            let trezor = inner.as_mut().ok_or(TrezorError::NotInitialized)?;

            let mut connected = trezor.connect(&device).await.map_err(TrezorError::from)?;

            // Wire UI callback if set
            if let Some(ui_cb) = crate::modules::trezor::get_ui_callback() {
                let adapter = Arc::new(UiCallbackAdapter {
                    callback: ui_cb.clone(),
                });
                connected.set_ui_callback(adapter);
            }

            // Initialize the device and get features
            let features = connected.initialize().await.map_err(TrezorError::from)?;

            // Store the connected device
            let mut connected_device = self.connected_device.lock().await;
            *connected_device = Some(connected);

            Ok(TrezorFeatures::from(features))
        }
    }

    /// Check if a device is currently connected.
    pub async fn is_connected(&self) -> bool {
        self.connected_device.lock().await.is_some()
    }

    /// Get a Bitcoin address from the connected device.
    ///
    /// # Arguments
    /// * `params` - Address parameters including BIP32 path and display options
    pub async fn get_address(
        &self,
        params: TrezorGetAddressParams,
    ) -> Result<TrezorAddressResponse, TrezorError> {
        // Validate the derivation path
        validate_derivation_path(&params.path)?;

        // Both mobile and desktop: use stored connected device
        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let tc_params: GetAddressParams = params.into();
        let response = device
            .get_address(tc_params)
            .await
            .map_err(TrezorError::from)?;

        Ok(TrezorAddressResponse::from(response))
    }

    /// Get a public key (xpub) from the connected device.
    ///
    /// # Arguments
    /// * `params` - Public key parameters including BIP32 path
    pub async fn get_public_key(
        &self,
        params: TrezorGetPublicKeyParams,
    ) -> Result<TrezorPublicKeyResponse, TrezorError> {
        // Validate the derivation path
        validate_derivation_path(&params.path)?;

        // Both mobile and desktop: use stored connected device
        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let tc_params: GetPublicKeyParams = params.into();
        let response = device
            .get_public_key(tc_params)
            .await
            .map_err(TrezorError::from)?;

        Ok(TrezorPublicKeyResponse::from(response))
    }

    /// Sign a message with the connected device.
    ///
    /// # Arguments
    /// * `params` - Sign message parameters including BIP32 path and message
    pub async fn sign_message(
        &self,
        params: TrezorSignMessageParams,
    ) -> Result<TrezorSignedMessageResponse, TrezorError> {
        // Validate the derivation path
        validate_derivation_path(&params.path)?;

        // Both mobile and desktop: use stored connected device
        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let tc_params: SignMessageParams = params.into();
        let response = device
            .sign_message(tc_params)
            .await
            .map_err(TrezorError::from)?;

        Ok(TrezorSignedMessageResponse::from(response))
    }

    /// Verify a message signature with the connected device.
    ///
    /// # Arguments
    /// * `params` - Verify message parameters including address, signature, and message
    pub async fn verify_message(
        &self,
        params: TrezorVerifyMessageParams,
    ) -> Result<bool, TrezorError> {
        // Both mobile and desktop: use stored connected device
        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let tc_params: VerifyMessageParams = params.into();
        device
            .verify_message(tc_params)
            .await
            .map_err(TrezorError::from)
    }

    /// Sign a Bitcoin transaction with the connected device.
    ///
    /// # Arguments
    /// * `params` - Transaction parameters including inputs, outputs, and options
    pub async fn sign_tx(&self, params: TrezorSignTxParams) -> Result<TrezorSignedTx, TrezorError> {
        let tc_params: SignTxParams = params.into();
        validate_sign_tx_params(&tc_params)?;

        // Both mobile and desktop: use stored connected device
        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let response = device
            .sign_transaction(tc_params)
            .await
            .map_err(TrezorError::from)?;

        Ok(TrezorSignedTx::from(response))
    }

    /// Sign a Bitcoin transaction from a base64-encoded PSBT.
    ///
    /// Parses the PSBT, extracts inputs/outputs/prev_txs, and signs via the
    /// connected Trezor device.
    ///
    /// # Arguments
    /// * `psbt_base64` - Base64-encoded PSBT data
    /// * `network` - Bitcoin network type. Defaults to Bitcoin (mainnet) if None.
    pub async fn sign_tx_from_psbt(
        &self,
        psbt_base64: String,
        network: Option<TrezorCoinType>,
    ) -> Result<TrezorSignedTx, TrezorError> {
        let btc_network = match network {
            Some(TrezorCoinType::Testnet) => bitcoin::Network::Testnet,
            Some(TrezorCoinType::Signet) => bitcoin::Network::Signet,
            Some(TrezorCoinType::Regtest) => bitcoin::Network::Regtest,
            _ => bitcoin::Network::Bitcoin,
        };

        let psbt_bytes = general_purpose::STANDARD
            .decode(&psbt_base64)
            .map_err(|e| TrezorError::DeviceError {
                error_details: format!("Invalid PSBT base64: {}", e),
            })?;
        let sign_params = trezor_connect_rs::psbt::psbt_to_sign_tx_params(&psbt_bytes, btc_network)
            .map_err(|e| TrezorError::DeviceError {
                error_details: format!("PSBT conversion error: {}", e),
            })?;

        validate_sign_tx_params(&sign_params)?;

        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let response = device
            .sign_transaction(sign_params)
            .await
            .map_err(TrezorError::from)?;
        Ok(TrezorSignedTx::from(response))
    }

    /// Disconnect from the currently connected device.
    pub async fn disconnect(&self) -> Result<(), TrezorError> {
        // Clear stored connected device (both mobile and desktop)
        {
            let mut connected_device = self.connected_device.lock().await;
            if let Some(mut device) = connected_device.take() {
                device.disconnect().await.map_err(TrezorError::from)?;
            }
        }

        // Mobile: Also clear connected path and close native device
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            use crate::get_transport_callback;

            let mut connected_path = self.connected_path.lock().await;
            if let Some(path) = connected_path.take() {
                if let Some(callback) = get_transport_callback() {
                    let result = callback.close_device(path);
                    if !result.success {
                        return Err(TrezorError::ConnectionError {
                            error_details: result.error,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the currently connected device info.
    pub async fn get_connected_device(&self) -> Option<TrezorDeviceInfo> {
        // Mobile: Return cached device info
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let connected_path = self.connected_path.lock().await;
            if let Some(ref path) = *connected_path {
                let device_list = self.native_device_list.lock().await;
                device_list
                    .iter()
                    .find(|d| &d.path == path)
                    .map(|d| TrezorDeviceInfo {
                        id: d.path.clone(),
                        transport_type: d.transport_type.clone(),
                        name: d.name.clone(),
                        path: d.path.clone(),
                        label: None,
                        model: None,
                        is_bootloader: false,
                    })
            } else {
                None
            }
        }

        // Desktop: Use trezor-connect-rs
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let connected_device = self.connected_device.lock().await;
            connected_device
                .as_ref()
                .map(|d| TrezorDeviceInfo::from(d.info().clone()))
        }
    }

    /// Get the cached features of the currently connected Trezor device.
    ///
    /// Returns the features that were obtained during `connect()`, without
    /// triggering any device interaction. Returns None if no device is connected.
    pub async fn get_features(&self) -> Option<TrezorFeatures> {
        let connected_device = self.connected_device.lock().await;
        connected_device
            .as_ref()
            .and_then(|device| device.features())
            .map(|f| TrezorFeatures::from(f.clone()))
    }

    /// Refresh features from the currently connected Trezor device.
    ///
    /// This is an explicit one-shot device interaction. It does not start any
    /// polling or retry loop. THP devices use `GetFeatures` via
    /// trezor-connect-rs; V1 devices use the dependency's `Initialize` path,
    /// which also refreshes its cached features.
    pub async fn refresh_features(&self) -> Result<TrezorFeatures, TrezorError> {
        let mut connected_device = self.connected_device.lock().await;
        let device = connected_device.as_mut().ok_or(TrezorError::NotConnected)?;

        let features = device.initialize().await.map_err(TrezorError::from)?;
        Ok(TrezorFeatures::from(features))
    }

    /// Get the device's master root fingerprint as an 8-character hex string.
    ///
    /// This is a convenience wrapper around `get_public_key()` that returns
    /// the root fingerprint in the standard format used in output descriptors
    /// (4 bytes, 8 hex chars, e.g., "73c5da0a").
    ///
    /// Internally calls `get_public_key` with `m/84'/0'/0'` and
    /// `show_on_trezor: false`.
    pub async fn get_device_fingerprint(&self) -> Result<String, TrezorError> {
        let params = TrezorGetPublicKeyParams {
            path: "m/84'/0'/0'".to_string(),
            show_on_trezor: false,
            coin: None,
        };
        let response = self.get_public_key(params).await?;
        let fingerprint = response.root_fingerprint.ok_or(TrezorError::DeviceError {
            error_details: "Device did not return root fingerprint".to_string(),
        })?;
        Ok(format!("{:08x}", fingerprint))
    }

    /// Clear stored credentials for a device.
    ///
    /// This removes any stored Bluetooth pairing credentials for the specified device,
    /// requiring re-pairing on the next connection.
    ///
    /// # Arguments
    /// * `device_id` - The device identifier (e.g., BLE address like "ble:AA:BB:CC:DD:EE:FF")
    pub async fn clear_credentials(&self, device_id: &str) -> Result<(), TrezorError> {
        // Mobile: Use the transport callback to clear credentials
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            use crate::get_transport_callback;

            let callback = get_transport_callback().ok_or(TrezorError::NotInitialized)?;

            // The native layer's save_thp_credential with empty string can be used
            // to clear, or we can define a new method. For now, we'll save empty
            // credentials which the native layer should interpret as deletion.
            // Alternatively, the native layer can implement a dedicated delete method.
            let success = callback.save_thp_credential(device_id.to_string(), "".to_string());
            if !success {
                return Err(TrezorError::IoError {
                    error_details: format!("Failed to clear credentials for device: {}", device_id),
                });
            }
            Ok(())
        }

        // Desktop: Clear credentials from the credential store
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let mut inner = self.inner.lock().await;
            let trezor = inner.as_mut().ok_or(TrezorError::NotInitialized)?;

            trezor
                .clear_credentials(device_id)
                .await
                .map_err(TrezorError::from)
        }
    }
}

impl Default for TrezorManager {
    fn default() -> Self {
        Self::new()
    }
}
