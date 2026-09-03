//! Session state and the operations exposed over FFI.
//!
//! State is deliberately split from the I/O lock. A single mutex guarding every
//! operation would make `jade_disconnect` and every status read queue behind a
//! five minute `sign_psbt`, and UniFFI async exports are detached onto the
//! runtime, so a cancelled Swift or Kotlin task does not cancel the Rust future
//! either. The abort path therefore never waits on the I/O lock.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use bitcoin::bip32::{DerivationPath, Xpub};
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Secp256k1;
use rand::RngCore;
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

use super::callbacks::{transport_callback, JadeNativeDevice};
use super::errors::JadeError;
use super::path;
use super::pinserver::{self, PinServerHttp, ReqwestPinServer};
use super::protocol::{result_bool, result_text};
use super::transport::{CallbackTransport, JadeConnection, JadeTransport};
use super::types::*;
use crate::onchain::AccountType;

/// Timeout for calls the device answers on its own.
const QUICK_TIMEOUT: Duration = Duration::from_secs(60);

/// Timeout for calls that wait on a physical button press.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

/// Largest PSBT this module will send.
///
/// Jade's input buffer is 17 KiB without SPIRAM. Composed PSBTs carry full
/// previous transactions, so this is worth checking before a long transfer that
/// the device would reject at the end.
const MAX_PSBT_BYTES: u64 = 16 * 1024;

/// What is known about the open session.
#[derive(Debug, Clone)]
struct SessionState {
    device: JadeDeviceInfo,
    version: JadeVersionInfo,
    /// The network `auth_user` unlocked, once it has succeeded. Later calls are
    /// checked against it so a mismatch is reported here rather than surfacing
    /// as an opaque device error.
    unlocked_network: Option<JadeNetwork>,
}

pub struct JadeManager {
    device_list: Mutex<Vec<JadeNativeDevice>>,
    state: RwLock<Option<SessionState>>,
    /// Held for exactly one round trip.
    io: Mutex<Option<JadeConnection>>,
    /// Cloned out by the abort path, which must not wait on `io`.
    transport: RwLock<Option<Arc<dyn JadeTransport>>>,
    aborted: Arc<AtomicBool>,
    /// Cheap status reads that never touch a lock held across I/O.
    connected: AtomicBool,
    pinserver: Arc<dyn PinServerHttp>,
}

impl Default for JadeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JadeManager {
    pub fn new() -> Self {
        Self::with_pinserver(Arc::new(ReqwestPinServer))
    }

    /// Build a manager with a specific pinserver implementation.
    pub(crate) fn with_pinserver(pinserver: Arc<dyn PinServerHttp>) -> Self {
        Self {
            device_list: Mutex::new(Vec::new()),
            state: RwLock::new(None),
            io: Mutex::new(None),
            transport: RwLock::new(None),
            aborted: Arc::new(AtomicBool::new(false)),
            connected: AtomicBool::new(false),
            pinserver,
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

        let mut discovered = Vec::new();

        if let Some(callback) = transport_callback() {
            let found = tokio::task::spawn_blocking(move || callback.scan_devices(timeout_ms))
                .await
                .map_err(|error| JadeError::IoError {
                    error_details: format!("scan task failed: {error}"),
                })?;
            discovered.extend(found);
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        discovered.extend(super::serial::enumerate_devices());

        let infos: Vec<JadeDeviceInfo> = discovered
            .iter()
            .map(|device| JadeDeviceInfo {
                id: JadeDeviceInfo::build_id(device.transport, &device.path),
                transport: device.transport,
                name: device.name.clone(),
                path: device.path.clone(),
                serial_number: device.serial_number.clone(),
            })
            .collect();

        *self.device_list.lock().await = discovered;
        Ok(infos)
    }

    /// The devices found by the last scan.
    pub async fn list_devices(&self) -> Vec<JadeDeviceInfo> {
        self.device_list
            .lock()
            .await
            .iter()
            .map(|device| JadeDeviceInfo {
                id: JadeDeviceInfo::build_id(device.transport, &device.path),
                transport: device.transport,
                name: device.name.clone(),
                path: device.path.clone(),
                serial_number: device.serial_number.clone(),
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Connection lifecycle
    // ------------------------------------------------------------------

    /// Open a device and read its version summary.
    pub async fn connect(&self, device_id: &str) -> Result<JadeVersionInfo, JadeError> {
        let (kind, path) = JadeDeviceInfo::parse_id(device_id).ok_or(JadeError::DeviceNotFound)?;
        let path = path.to_string();

        let device = {
            let devices = self.device_list.lock().await;
            devices
                .iter()
                .find(|candidate| candidate.transport == kind && candidate.path == path)
                .cloned()
                .ok_or(JadeError::DeviceNotFound)?
        };

        // Close anything already open first. Overwriting the connection would
        // strand the native handle with no path left to close it.
        self.disconnect().await?;
        self.aborted.store(false, Ordering::SeqCst);

        let transport: Arc<dyn JadeTransport> = match kind {
            JadeTransportKind::Bluetooth => {
                let callback = transport_callback().ok_or(JadeError::NotInitialized)?;
                let open_path = path.clone();
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
                Arc::new(CallbackTransport::new(callback, path.clone()))
            }
            JadeTransportKind::Serial => {
                #[cfg(not(any(target_os = "ios", target_os = "android")))]
                {
                    Arc::new(super::serial::SerialTransport::open(&path)?)
                }
                // On mobile a serial device can only have come from the native
                // layer, so it is driven through the callback like Bluetooth.
                #[cfg(any(target_os = "ios", target_os = "android"))]
                {
                    let callback = transport_callback().ok_or(JadeError::NotInitialized)?;
                    let open_path = path.clone();
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
                    Arc::new(CallbackTransport::new(callback, path.clone()))
                }
            }
        };

        *self.transport.write().await = Some(Arc::clone(&transport));
        let mut connection = JadeConnection::new(transport, Arc::clone(&self.aborted));

        let version = Self::read_version(&mut connection).await?;

        // Contribute host entropy to the device's pool. The buffer is zeroized
        // on drop rather than left in a freed allocation.
        Self::add_entropy(&mut connection).await?;

        let info = JadeDeviceInfo {
            id: device_id.to_string(),
            transport: kind,
            name: device.name.clone(),
            path: device.path.clone(),
            serial_number: device.serial_number.clone(),
        };

        *self.io.lock().await = Some(connection);
        *self.state.write().await = Some(SessionState {
            device: info,
            version: version.clone(),
            unlocked_network: None,
        });
        self.connected.store(true, Ordering::SeqCst);

        Ok(version)
    }

    async fn read_version(connection: &mut JadeConnection) -> Result<JadeVersionInfo, JadeError> {
        let reply = connection
            .exchange("get_version_info", Option::<()>::None, QUICK_TIMEOUT)
            .await?;
        let value = reply.into_result(MIN_JADE_FIRMWARE)?;
        let wire: WireVersionInfo = value.deserialized().map_err(|error| {
            JadeError::protocol(format!("unexpected get_version_info reply: {error}"))
        })?;
        Ok(JadeVersionInfo::from(wire))
    }

    async fn add_entropy(connection: &mut JadeConnection) -> Result<(), JadeError> {
        #[derive(Serialize)]
        struct AddEntropyParams {
            #[serde(with = "serde_bytes")]
            entropy: Vec<u8>,
        }

        let mut entropy = Zeroizing::new(vec![0u8; 32]);
        rand::rngs::OsRng.fill_bytes(&mut entropy);
        let params = AddEntropyParams {
            entropy: entropy.to_vec(),
        };
        let reply = connection
            .exchange("add_entropy", Some(params), QUICK_TIMEOUT)
            .await?;
        result_bool(&reply.into_result(MIN_JADE_FIRMWARE)?)?;
        Ok(())
    }

    /// Close the device and clear session state.
    ///
    /// Safe to call while an operation is in flight: the abort flag is set and
    /// the transport closed without taking the I/O lock, so a blocked request
    /// returns promptly instead of running out its deadline.
    pub async fn disconnect(&self) -> Result<(), JadeError> {
        self.aborted.store(true, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);

        let transport = self.transport.write().await.take();
        if let Some(transport) = transport {
            if let Err(error) = transport.close().await {
                log::debug!("[jade] error closing the transport: {error}");
            }
        }

        *self.state.write().await = None;
        *self.io.lock().await = None;
        Ok(())
    }

    /// Abort the operation in flight without tearing down session state.
    ///
    /// Jade has no cancel message, so closing the link is the only way to stop
    /// a pending confirmation. The application is expected to reconnect.
    pub async fn cancel(&self) -> Result<(), JadeError> {
        self.aborted.store(true, Ordering::SeqCst);
        let transport = self.transport.read().await.clone();
        if let Some(transport) = transport {
            let _ = transport.close().await;
        }
        Ok(())
    }

    /// Record a disconnect the native layer noticed while nothing was in flight.
    pub async fn notify_disconnected(&self, path: &str) {
        let matches = self
            .state
            .read()
            .await
            .as_ref()
            .map(|state| state.device.path == path)
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
        self.state
            .read()
            .await
            .as_ref()
            .map(|state| state.device.clone())
    }

    /// The version summary read at connect, or refreshed since.
    pub async fn version_info(&self) -> Option<JadeVersionInfo> {
        self.state
            .read()
            .await
            .as_ref()
            .map(|state| state.version.clone())
    }

    /// Re-read the version summary from the device.
    pub async fn refresh_version_info(&self) -> Result<JadeVersionInfo, JadeError> {
        let mut guard = self.io.lock().await;
        let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
        let version = Self::read_version(connection).await?;
        drop(guard);
        self.store_version(version.clone()).await;
        Ok(version)
    }

    async fn store_version(&self, version: JadeVersionInfo) {
        if let Some(state) = self.state.write().await.as_mut() {
            state.version = version;
        }
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    pub async fn ping(&self) -> Result<JadePingStatus, JadeError> {
        let mut guard = self.io.lock().await;
        let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
        let reply = connection
            .exchange("ping", Option::<()>::None, QUICK_TIMEOUT)
            .await?;
        let value = reply.into_result(MIN_JADE_FIRMWARE)?;
        let raw = value
            .as_integer()
            .and_then(|integer| u64::try_from(integer).ok())
            .ok_or_else(|| JadeError::protocol("expected an integer ping result"))?;
        Ok(JadePingStatus::from_wire(raw))
    }

    /// Unlock the device, running the blind pinserver exchange if it asks.
    pub async fn unlock(&self, network: JadeNetwork) -> Result<(), JadeError> {
        // A device with no wallet starts an on-device setup flow that can take
        // minutes and cannot be driven from here.
        if let Some(state) = self.state.read().await.as_ref() {
            if state.version.jade_state == JadeState::Uninit {
                return Err(JadeError::DeviceUninitialized);
            }
        }

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);

        {
            let mut guard = self.io.lock().await;
            let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
            pinserver::run_unlock(connection, network, self.pinserver.as_ref(), epoch).await?;
        }

        if let Some(state) = self.state.write().await.as_mut() {
            state.unlocked_network = Some(network);
        }
        // The cached state still says LOCKED until this is refreshed, and the
        // whole point of exposing it is telling the app whether to prompt.
        let _ = self.refresh_version_info().await;
        Ok(())
    }

    pub async fn logout(&self) -> Result<(), JadeError> {
        {
            let mut guard = self.io.lock().await;
            let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
            let reply = connection
                .exchange("logout", Option::<()>::None, QUICK_TIMEOUT)
                .await?;
            result_bool(&reply.into_result(MIN_JADE_FIRMWARE)?)?;
        }
        if let Some(state) = self.state.write().await.as_mut() {
            state.unlocked_network = None;
        }
        let _ = self.refresh_version_info().await;
        Ok(())
    }

    /// Check the requested network against the one that was unlocked.
    async fn check_network(&self, network: JadeNetwork) -> Result<(), JadeError> {
        let unlocked = self
            .state
            .read()
            .await
            .as_ref()
            .and_then(|state| state.unlocked_network);
        match unlocked {
            Some(unlocked) if unlocked != network => Err(JadeError::NetworkMismatch {
                error_details: format!(
                    "the device was unlocked for {} but the request is for {}",
                    unlocked.wire_name(),
                    network.wire_name()
                ),
            }),
            _ => Ok(()),
        }
    }

    async fn raw_xpub(
        &self,
        network: JadeNetwork,
        derivation_path: &str,
        allow_master: bool,
    ) -> Result<String, JadeError> {
        #[derive(Serialize)]
        struct GetXpubParams<'a> {
            network: &'a str,
            path: Vec<u32>,
        }

        let wire_path = path::to_wire(derivation_path, allow_master)?;
        let params = GetXpubParams {
            network: network.wire_name(),
            path: wire_path,
        };

        let mut guard = self.io.lock().await;
        let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
        let reply = connection
            .exchange("get_xpub", Some(params), QUICK_TIMEOUT)
            .await?;
        result_text(&reply.into_result(MIN_JADE_FIRMWARE)?)
    }

    /// The device's master fingerprint, eight lowercase hex characters.
    ///
    /// Derived from `m/0'`'s parent fingerprint rather than by asking for the
    /// master xpub directly, which is how HWI does it and which avoids relying
    /// on the device accepting an empty path.
    pub async fn master_fingerprint(&self, network: JadeNetwork) -> Result<String, JadeError> {
        self.check_network(network).await?;
        let xpub = self.raw_xpub(network, "m/0'", false).await?;
        let parsed = Xpub::from_str(&xpub).map_err(|error| {
            JadeError::protocol(format!("device returned an unparsable xpub: {error}"))
        })?;
        Ok(format!("{:08x}", parsed.parent_fingerprint))
    }

    /// Fetch an extended public key, echoed back with the request it answers.
    pub async fn get_xpub(&self, params: JadeGetXpubParams) -> Result<JadeXpubResponse, JadeError> {
        self.check_network(params.network).await?;
        let fingerprint = self.master_fingerprint(params.network).await?;
        let xpub = self
            .raw_xpub(params.network, &params.derivation_path, false)
            .await?;
        self.verify_xpub(&xpub, &params.derivation_path)?;

        Ok(JadeXpubResponse {
            xpub,
            derivation_path: params.derivation_path,
            master_fingerprint: fingerprint,
        })
    }

    /// Confirm the device answered the question that was asked.
    fn verify_xpub(&self, xpub: &str, derivation_path: &str) -> Result<(), JadeError> {
        let parsed = Xpub::from_str(xpub).map_err(|error| {
            JadeError::protocol(format!("device returned an unparsable xpub: {error}"))
        })?;
        let expected = DerivationPath::from_str(derivation_path.trim()).map_err(|error| {
            JadeError::InvalidPath {
                error_details: error.to_string(),
            }
        })?;
        let depth = expected.len();
        if usize::from(parsed.depth) != depth {
            return Err(JadeError::protocol(format!(
                "device returned a key at depth {} for a path of depth {depth}",
                parsed.depth
            )));
        }
        Ok(())
    }

    /// Fetch the account xpubs an import needs in one round of I/O.
    pub async fn account_export(
        &self,
        network: JadeNetwork,
        account_index: u32,
        account_types: Vec<AccountType>,
    ) -> Result<JadeAccountExport, JadeError> {
        self.check_network(network).await?;
        let fingerprint = self.master_fingerprint(network).await?;

        let mut accounts = Vec::with_capacity(account_types.len());
        for account_type in account_types {
            let purpose = JadeAddressVariant::from(account_type).purpose();
            let derivation_path = format!("m/{purpose}'/{}'/{account_index}'", network.coin_type());
            let xpub = self.raw_xpub(network, &derivation_path, false).await?;
            self.verify_xpub(&xpub, &derivation_path)?;
            accounts.push(JadeAccount {
                account_type,
                xpub,
                derivation_path,
            });
        }

        Ok(JadeAccountExport {
            master_fingerprint: fingerprint,
            account_index,
            accounts,
        })
    }

    /// Ask the device to display an address, and check it against the expected one.
    ///
    /// This call always prompts on the device screen, so it is a verification
    /// step rather than a fetch: the application already knows the address from
    /// the account xpub. Comparing the two catches corruption and firmware bugs.
    /// A wholly malicious device is still the user's job to catch by reading the
    /// device screen.
    pub async fn verify_address(&self, params: JadeVerifyAddressParams) -> Result<(), JadeError> {
        #[derive(Serialize)]
        struct GetReceiveAddressParams<'a> {
            network: &'a str,
            variant: &'a str,
            path: Vec<u32>,
        }

        self.check_network(params.network).await?;

        // A legacy variant under an m/84' path is a caller bug worth catching
        // before the device is asked to display something misleading.
        if let Some(purpose) = path::purpose(&params.derivation_path) {
            if purpose != params.variant.purpose() {
                return Err(JadeError::InvalidPath {
                    error_details: format!(
                        "path purpose {purpose} does not match the {} variant",
                        params.variant.wire_name()
                    ),
                });
            }
        }

        // Taproot addresses arrived in 1.0.34. Older firmware answers
        // BAD_PARAMETERS, which says nothing useful to the user.
        if params.variant == JadeAddressVariant::Tr {
            if let Some(state) = self.state.read().await.as_ref() {
                let installed = &state.version.jade_version;
                if !version_at_least(installed, MIN_JADE_FIRMWARE_TAPROOT) {
                    return Err(JadeError::UnsupportedFirmware {
                        installed: installed.clone(),
                        required: MIN_JADE_FIRMWARE_TAPROOT.to_string(),
                    });
                }
            }
        }

        let wire_path = path::to_wire(&params.derivation_path, false)?;
        let request = GetReceiveAddressParams {
            network: params.network.wire_name(),
            variant: params.variant.wire_name(),
            path: wire_path,
        };

        let returned = {
            let mut guard = self.io.lock().await;
            let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
            let reply = connection
                .exchange("get_receive_address", Some(request), CONFIRM_TIMEOUT)
                .await?;
            result_text(&reply.into_result(MIN_JADE_FIRMWARE)?)?
        };

        if returned != params.expected_address {
            return Err(JadeError::AddressMismatch {
                expected: params.expected_address,
                returned,
            });
        }
        Ok(())
    }

    /// Sign a message, returning the signature with the address that verifies it.
    pub async fn sign_message(
        &self,
        params: JadeSignMessageParams,
        network: JadeNetwork,
    ) -> Result<JadeSignedMessage, JadeError> {
        #[derive(Serialize)]
        struct SignMessageParams<'a> {
            message: &'a str,
            path: Vec<u32>,
        }

        self.check_network(network).await?;
        let wire_path = path::to_wire(&params.derivation_path, false)?;

        // Derive the address host side so the caller can verify without a
        // second round trip.
        let xpub = self
            .raw_xpub(network, &params.derivation_path, false)
            .await?;
        let parsed = Xpub::from_str(&xpub).map_err(|error| {
            JadeError::protocol(format!("device returned an unparsable xpub: {error}"))
        })?;
        let address = bitcoin::Address::p2wpkh(
            &bitcoin::CompressedPublicKey(parsed.public_key),
            bitcoin::Network::from(network),
        )
        .to_string();

        let request = SignMessageParams {
            message: &params.message,
            path: wire_path,
        };
        let mut guard = self.io.lock().await;
        let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
        let reply = connection
            .exchange("sign_message", Some(request), CONFIRM_TIMEOUT)
            .await?;
        let signature = result_text(&reply.into_result(MIN_JADE_FIRMWARE)?)?;

        Ok(JadeSignedMessage {
            signature,
            address,
            derivation_path: params.derivation_path,
        })
    }

    /// Sign a PSBT.
    ///
    /// The reply is checked against what was sent before it is returned, so the
    /// guarantee holds at this boundary even for a caller that does not go on to
    /// use `finalize_psbt`.
    pub async fn sign_psbt(&self, params: JadeSignPsbtParams) -> Result<String, JadeError> {
        #[derive(Serialize)]
        struct SignPsbtParams<'a> {
            network: &'a str,
            #[serde(with = "serde_bytes")]
            psbt: Vec<u8>,
        }

        self.check_network(params.network).await?;

        let bytes =
            STANDARD
                .decode(params.psbt.trim())
                .map_err(|error| JadeError::InvalidPsbt {
                    error_details: format!("base64 decoding failed: {error}"),
                })?;
        let sent = Psbt::deserialize(&bytes).map_err(|error| JadeError::InvalidPsbt {
            error_details: format!("parsing failed: {error}"),
        })?;

        if bytes.len() as u64 > MAX_PSBT_BYTES {
            return Err(JadeError::PsbtTooLarge {
                size: bytes.len() as u64,
                max: MAX_PSBT_BYTES,
            });
        }

        self.check_signable(&sent, params.network).await?;

        let request = SignPsbtParams {
            network: params.network.wire_name(),
            psbt: bytes,
        };

        let signed_bytes = {
            let mut guard = self.io.lock().await;
            let connection = guard.as_mut().ok_or(JadeError::NotConnected)?;
            connection
                .exchange_reassembled("sign_psbt", Some(request), CONFIRM_TIMEOUT)
                .await?
        };

        let signed = Psbt::deserialize(&signed_bytes).map_err(|error| JadeError::InvalidPsbt {
            error_details: format!("device returned an unparsable PSBT: {error}"),
        })?;
        verify_signed_psbt(&sent, &signed)?;

        Ok(STANDARD.encode(&signed_bytes))
    }

    /// Reject a PSBT the device would refuse or silently not sign.
    async fn check_signable(&self, psbt: &Psbt, network: JadeNetwork) -> Result<(), JadeError> {
        // Only SIGHASH_ALL and the taproot default are expected here. Anything
        // else arriving over FFI is worth refusing rather than signing blindly.
        for (index, input) in psbt.inputs.iter().enumerate() {
            if let Some(sighash) = input.sighash_type {
                let is_all = sighash
                    .ecdsa_hash_ty()
                    .map(|ty| ty == bitcoin::sighash::EcdsaSighashType::All)
                    .unwrap_or(false);
                let is_default = sighash
                    .taproot_hash_ty()
                    .map(|ty| ty == bitcoin::sighash::TapSighashType::Default)
                    .unwrap_or(false);
                if !is_all && !is_default {
                    return Err(JadeError::InvalidPsbt {
                        error_details: format!(
                            "input {index} requests an unsupported sighash type"
                        ),
                    });
                }
            }
        }

        // Without a matching fingerprint the device signs nothing and the
        // failure only shows up as an opaque finalization error much later.
        let Ok(device_fingerprint) = self.master_fingerprint(network).await else {
            return Ok(());
        };
        let mut seen = Vec::new();
        let mut matched = false;
        for input in &psbt.inputs {
            for (fingerprint, _) in input.bip32_derivation.values() {
                let rendered = format!("{fingerprint:08x}");
                if rendered == device_fingerprint {
                    matched = true;
                }
                seen.push(rendered);
            }
            for (_, (fingerprint, _)) in input.tap_key_origins.values() {
                let rendered = format!("{fingerprint:08x}");
                if rendered == device_fingerprint {
                    matched = true;
                }
                seen.push(rendered);
            }
        }
        if !seen.is_empty() && !matched {
            seen.sort();
            seen.dedup();
            return Err(JadeError::FingerprintMismatch {
                device: device_fingerprint,
                psbt: seen.join(", "),
            });
        }
        Ok(())
    }
}

/// Check what came back against what was sent.
fn verify_signed_psbt(sent: &Psbt, signed: &Psbt) -> Result<(), JadeError> {
    if sent.unsigned_tx != signed.unsigned_tx {
        return Err(JadeError::InvalidPsbt {
            error_details: "device returned a different unsigned transaction".to_string(),
        });
    }
    if sent.inputs.len() != signed.inputs.len() || sent.outputs.len() != signed.outputs.len() {
        return Err(JadeError::InvalidPsbt {
            error_details: "device changed the number of inputs or outputs".to_string(),
        });
    }

    for (index, (before, after)) in sent.inputs.iter().zip(signed.inputs.iter()).enumerate() {
        if before.witness_utxo != after.witness_utxo {
            return Err(JadeError::InvalidPsbt {
                error_details: format!("device altered the witness UTXO of input {index}"),
            });
        }
        if before.non_witness_utxo != after.non_witness_utxo {
            return Err(JadeError::InvalidPsbt {
                error_details: format!("device altered the previous transaction of input {index}"),
            });
        }
    }

    let gained_signature = signed.inputs.iter().enumerate().any(|(index, input)| {
        let before = &sent.inputs[index];
        input.partial_sigs.len() > before.partial_sigs.len()
            || (input.final_script_witness.is_some() && before.final_script_witness.is_none())
            || (input.final_script_sig.is_some() && before.final_script_sig.is_none())
            || (input.tap_key_sig.is_some() && before.tap_key_sig.is_none())
    });
    if !gained_signature {
        return Err(JadeError::NothingSigned);
    }

    // Keep the secp context construction close to the other PSBT handling in
    // this crate; verification only, no signing key material here.
    let _ = Secp256k1::verification_only();
    Ok(())
}
