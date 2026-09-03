//! Session state for the FFI surface.
//!
//! `jade_client_rs::Jade` takes `&mut self` for every operation, which makes the
//! one-request-at-a-time rule a compile time property. The FFI surface here is a
//! set of free functions over a process global, so this adds the lock that shape
//! implies, plus the device list and the abort handle.
//!
//! The abort handle is deliberately kept outside the session lock. Holding one
//! lock for both would make `jade_disconnect` and every status read queue behind
//! a five minute confirmation, and UniFFI async exports are detached onto the
//! runtime, so a cancelled Swift or Kotlin task does not cancel the Rust future
//! by itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use bitcoin::psbt::Psbt;
use jade_client_rs::{CancelHandle, Jade, JadeTransport};
use tokio::sync::{Mutex, RwLock};

use super::callbacks::{transport_callback, CallbackTransport};
use super::types::*;
use crate::onchain::AccountType;

/// A device seen by the last scan.
#[derive(Debug, Clone)]
struct CachedDevice {
    info: JadeDeviceInfo,
}

pub struct JadeManager {
    device_list: Mutex<Vec<CachedDevice>>,
    /// Held for exactly one operation.
    session: Mutex<Option<Jade>>,
    /// Cloned out by the abort path, which must not wait on `session`.
    cancel: RwLock<Option<CancelHandle>>,
    /// Cheap status reads that never touch a lock held across device I/O.
    connected: AtomicBool,
    connected_device: RwLock<Option<JadeDeviceInfo>>,
}

impl Default for JadeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JadeManager {
    pub fn new() -> Self {
        Self {
            device_list: Mutex::new(Vec::new()),
            session: Mutex::new(None),
            cancel: RwLock::new(None),
            connected: AtomicBool::new(false),
            connected_device: RwLock::new(None),
        }
    }

    // ------------------------------------------------------------------
    // Discovery
    // ------------------------------------------------------------------

    /// Discover devices on every transport this build supports.
    pub async fn scan(&self, timeout_ms: u32) -> Result<Vec<JadeDeviceInfo>, JadeError> {
        // Starting a Bluetooth scan while a GATT link is up reliably drops it on
        // Android, so refuse rather than silently breaking the open session.
        if self.connected.load(Ordering::SeqCst) {
            return Err(JadeError::DeviceBusy);
        }

        let mut discovered: Vec<JadeDeviceInfo> = Vec::new();

        if let Some(callback) = transport_callback() {
            let found = tokio::task::spawn_blocking(move || callback.scan_devices(timeout_ms))
                .await
                .map_err(|error| JadeError::IoError {
                    error_details: format!("scan task failed: {error}"),
                })?;
            discovered.extend(found.into_iter().map(|device| JadeDeviceInfo {
                path: device.path,
                transport: device.transport,
                name: device.name,
                serial_number: device.serial_number,
            }));
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        discovered.extend(jade_client_rs::serial::enumerate_devices());

        *self.device_list.lock().await = discovered
            .iter()
            .cloned()
            .map(|info| CachedDevice { info })
            .collect();
        Ok(discovered)
    }

    /// The devices found by the last scan.
    pub async fn list_devices(&self) -> Vec<JadeDeviceInfo> {
        self.device_list
            .lock()
            .await
            .iter()
            .map(|device| device.info.clone())
            .collect()
    }

    // ------------------------------------------------------------------
    // Connection lifecycle
    // ------------------------------------------------------------------

    /// Open a device and read its version summary.
    pub async fn connect(
        &self,
        transport_kind: JadeTransportKind,
        path: &str,
    ) -> Result<JadeVersionInfo, JadeError> {
        let device = {
            let devices = self.device_list.lock().await;
            devices
                .iter()
                .find(|candidate| {
                    candidate.info.transport == transport_kind && candidate.info.path == path
                })
                .map(|candidate| candidate.info.clone())
                .ok_or(JadeError::DeviceNotFound)?
        };

        // Close anything already open first. Overwriting the session would
        // strand the native handle with no path left to close it.
        self.disconnect().await?;

        let transport = self.build_transport(transport_kind, path).await?;
        let session = Jade::connect(transport).await?;
        let version = session.version_info().clone();

        *self.cancel.write().await = Some(session.cancel_handle());
        *self.connected_device.write().await = Some(device);
        *self.session.lock().await = Some(session);
        self.connected.store(true, Ordering::SeqCst);

        Ok(version)
    }

    async fn build_transport(
        &self,
        transport_kind: JadeTransportKind,
        path: &str,
    ) -> Result<Arc<dyn JadeTransport>, JadeError> {
        // A serial device found by the crate's own enumeration is driven
        // directly; anything the native layer reported goes back through it.
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        if transport_kind == JadeTransportKind::Serial
            && jade_client_rs::serial::enumerate_devices()
                .iter()
                .any(|device| device.path == path)
        {
            return Ok(Arc::new(jade_client_rs::SerialTransport::open(path)?));
        }

        let callback = transport_callback().ok_or(JadeError::NotInitialized)?;
        let open_path = path.to_string();
        let opener = Arc::clone(&callback);
        let result = tokio::task::spawn_blocking(move || opener.open_device(open_path))
            .await
            .map_err(|error| JadeError::IoError {
                error_details: format!("open task failed: {error}"),
            })?;
        if !result.success {
            return Err(JadeError::ConnectionError {
                error_details: result.error,
            });
        }
        Ok(Arc::new(CallbackTransport::new(callback, path.to_string())))
    }

    /// Close the device and clear session state.
    ///
    /// Safe to call while an operation is in flight: the cancel handle closes
    /// the transport without taking the session lock, so a blocked request
    /// returns promptly instead of running out its deadline.
    pub async fn disconnect(&self) -> Result<(), JadeError> {
        self.connected.store(false, Ordering::SeqCst);

        if let Some(cancel) = self.cancel.write().await.take() {
            if let Err(error) = cancel.cancel().await {
                log::debug!("[jade] error closing the transport: {error}");
            }
        }
        *self.connected_device.write().await = None;
        *self.session.lock().await = None;
        Ok(())
    }

    /// Abort the operation in flight without tearing down session state.
    ///
    /// Jade has no cancel message, so closing the link is the only way to stop a
    /// pending confirmation. The application is expected to reconnect.
    pub async fn cancel(&self) -> Result<(), JadeError> {
        let handle = self.cancel.read().await.clone();
        if let Some(handle) = handle {
            handle.cancel().await?;
        }
        Ok(())
    }

    /// Record a disconnect the native layer noticed while nothing was in flight.
    pub async fn notify_disconnected(&self, path: &str) {
        let matches = self
            .connected_device
            .read()
            .await
            .as_ref()
            .map(|device| device.path == path)
            .unwrap_or(false);
        if matches {
            log::debug!("[jade] native layer reported a disconnect");
            let _ = self.disconnect().await;
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub async fn connected_device(&self) -> Option<JadeDeviceInfo> {
        self.connected_device.read().await.clone()
    }

    /// The version summary read at connect, or refreshed since.
    pub async fn version_info(&self) -> Option<JadeVersionInfo> {
        self.session
            .lock()
            .await
            .as_ref()
            .map(|session| session.version_info().clone())
    }

    /// Re-read the version summary from the device.
    pub async fn refresh_version_info(&self) -> Result<JadeVersionInfo, JadeError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or(JadeError::NotConnected)?;
        session.refresh_version_info().await.cloned()
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    pub async fn ping(&self) -> Result<JadePingStatus, JadeError> {
        let mut guard = self.session.lock().await;
        guard.as_mut().ok_or(JadeError::NotConnected)?.ping().await
    }

    pub async fn unlock(&self, network: JadeNetwork) -> Result<(), JadeError> {
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .unlock(network)
            .await
    }

    pub async fn logout(&self) -> Result<(), JadeError> {
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .logout()
            .await
    }

    pub async fn master_fingerprint(&self, network: JadeNetwork) -> Result<String, JadeError> {
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .master_fingerprint(network)
            .await
    }

    pub async fn get_xpub(
        &self,
        network: JadeNetwork,
        derivation_path: String,
    ) -> Result<JadeXpubResponse, JadeError> {
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .get_xpub(network, &derivation_path)
            .await
    }

    pub async fn account_export(
        &self,
        network: JadeNetwork,
        account_index: u32,
        account_types: Vec<AccountType>,
    ) -> Result<JadeAccountExport, JadeError> {
        let variants: Vec<JadeAddressVariant> = account_types
            .into_iter()
            .map(account_type_to_variant)
            .collect();
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .account_export(network, account_index, &variants)
            .await
    }

    pub async fn verify_address(
        &self,
        network: JadeNetwork,
        variant: JadeAddressVariant,
        derivation_path: String,
        expected_address: String,
    ) -> Result<(), JadeError> {
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .verify_address(network, variant, &derivation_path, &expected_address)
            .await
    }

    pub async fn sign_message(
        &self,
        network: JadeNetwork,
        derivation_path: String,
        message: String,
    ) -> Result<JadeSignedMessage, JadeError> {
        let mut guard = self.session.lock().await;
        guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .sign_message(network, &derivation_path, &message)
            .await
    }

    /// Sign a base64 PSBT and return the signed PSBT, base64 encoded.
    ///
    /// The FFI surface speaks base64 because that is what `compose_transaction`
    /// emits and what `finalize_psbt` expects; the protocol crate works in typed
    /// PSBTs, so the encoding boundary lives here.
    pub async fn sign_psbt(&self, network: JadeNetwork, psbt: String) -> Result<String, JadeError> {
        let bytes = STANDARD
            .decode(psbt.trim())
            .map_err(|error| JadeError::InvalidPsbt {
                error_details: format!("base64 decoding failed: {error}"),
            })?;
        let parsed = Psbt::deserialize(&bytes).map_err(|error| JadeError::InvalidPsbt {
            error_details: format!("parsing failed: {error}"),
        })?;

        let mut guard = self.session.lock().await;
        let signed = guard
            .as_mut()
            .ok_or(JadeError::NotConnected)?
            .sign_psbt(network, &parsed)
            .await?;
        Ok(STANDARD.encode(signed.serialize()))
    }
}
