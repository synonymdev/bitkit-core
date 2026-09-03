//! The transport contract the native application implements, and the bridge
//! from it to the protocol crate's transport trait.
//!
//! Rust owns the Jade protocol; the application owns the bytes. On iOS that
//! means CoreBluetooth against the Nordic UART Service, and on Android the
//! Bluetooth API plus, optionally, the USB Host API for CDC serial.
//!
//! Methods are synchronous because that is the shape the trezor module already
//! established here. Every one of them is invoked on the blocking pool, so a
//! slow implementation costs a blocking thread rather than a runtime worker.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use jade_client_rs::{JadeError, JadeTransport, JadeTransportErrorCode, MAX_CHUNK_BYTES};

use super::types::JadeTransportKind;

/// A device the native layer discovered.
#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeNativeDevice {
    /// Transport specific address: a BLE identifier or a serial device path.
    pub path: String,
    pub transport: JadeTransportKind,
    /// Advertised or descriptor name, for example "Jade C0FFEE".
    pub name: Option<String>,
    pub serial_number: Option<String>,
}

/// Outcome of an operation that returns no data.
#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeTransportResult {
    pub success: bool,
    /// Empty on success.
    pub error: String,
    pub error_code: Option<JadeTransportErrorCode>,
}

/// Outcome of a read.
#[derive(Debug, Clone, uniffi::Record)]
pub struct JadeTransportReadResult {
    pub success: bool,
    /// Bytes read. Success with an empty vector means nothing has arrived yet,
    /// which is the normal case while the user is deciding on the device.
    pub data: Vec<u8>,
    /// Empty on success.
    pub error: String,
    pub error_code: Option<JadeTransportErrorCode>,
}

/// Native transport for Jade.
///
/// # Bluetooth contract
///
/// Jade advertises the Nordic UART Service:
///
/// - service `6e400001-b5a3-f393-e0a9-e50e24dcca9e`
/// - write   `6e400002-b5a3-f393-e0a9-e50e24dcca9e` (host to Jade)
/// - notify  `6e400003-b5a3-f393-e0a9-e50e24dcca9e` (Jade to host)
///
/// Devices advertise as "Jade" or "Jade <serial>".
///
/// Three requirements that are easy to miss and break signing on real hardware:
///
/// 1. **Write with response.** Write-without-response silently drops chunks on
///    the ESP32 GATT stack.
/// 2. **Do not pause between chunks.** Firmware discards a partially received
///    message after two seconds of silence, three on Jade v1, and answers with
///    an unattributed error. A 30 KB PSBT is roughly 60 writes, so any UI thread
///    stall in the middle of a send breaks the operation.
/// 3. **`read_chunk` must return promptly.** Honour `timeout_ms`, which this
///    crate keeps short. The long per-operation deadline is enforced in Rust so
///    the user can cancel.
#[uniffi::export(with_foreign)]
pub trait JadeTransportCallback: Send + Sync {
    /// Discover devices, blocking up to `timeout_ms`.
    fn scan_devices(&self, timeout_ms: u32) -> Vec<JadeNativeDevice>;

    /// Open a connection and enable notifications.
    fn open_device(&self, path: String) -> JadeTransportResult;

    /// Close the connection and release the device.
    fn close_device(&self, path: String) -> JadeTransportResult;

    /// Write one chunk, no larger than `get_chunk_size`.
    fn write_chunk(&self, path: String, data: Vec<u8>) -> JadeTransportResult;

    /// Read whatever has arrived, waiting at most `timeout_ms`.
    ///
    /// Returning success with an empty vector is normal and means "nothing yet".
    fn read_chunk(&self, path: String, timeout_ms: u32) -> JadeTransportReadResult;

    /// Maximum bytes per write.
    ///
    /// For Bluetooth this is `min(negotiated_mtu - 3, 509)`. The value is
    /// clamped into a usable range, so an unnegotiated `0` is not fatal.
    fn get_chunk_size(&self, path: String) -> u32;
}

/// The registered callback.
///
/// A read-write cell rather than a write-once cell on purpose. An Android
/// activity restart rebuilds the Bluetooth manager and registers a fresh
/// implementation; silently keeping the first one would leave this crate calling
/// into a dead context with no recovery short of killing the process.
static TRANSPORT_CALLBACK: RwLock<Option<Arc<dyn JadeTransportCallback>>> = RwLock::new(None);

/// Register the native transport.
///
/// Returns `true` when this replaced a previously registered callback, which
/// lets the application tell a fresh registration from a re-registration.
#[uniffi::export]
pub fn jade_set_transport_callback(callback: Arc<dyn JadeTransportCallback>) -> bool {
    #[cfg(target_os = "android")]
    crate::init_android_logger();

    let mut guard = TRANSPORT_CALLBACK
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced = guard.is_some();
    if replaced {
        log::warn!("[jade] transport callback replaced");
    }
    *guard = Some(callback);
    replaced
}

/// Fetch the registered transport, if any.
pub(crate) fn transport_callback() -> Option<Arc<dyn JadeTransportCallback>> {
    TRANSPORT_CALLBACK
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn to_error(code: Option<JadeTransportErrorCode>, message: String) -> JadeError {
    match code {
        Some(code) => JadeError::from(code),
        None => JadeError::TransportError {
            error_details: message,
        },
    }
}

/// Bridges the foreign callback onto the protocol crate's transport trait.
///
/// The error code travels as a typed value the whole way, so nothing has to be
/// encoded into an error string and parsed back out. The trezor adapter in this
/// repo does exactly that, because its upstream crate offers no typed channel.
pub(crate) struct CallbackTransport {
    callback: Arc<dyn JadeTransportCallback>,
    path: String,
    chunk_size: usize,
}

impl CallbackTransport {
    pub(crate) fn new(callback: Arc<dyn JadeTransportCallback>, path: String) -> Self {
        // Clamp whatever the native layer reports. A zero would make the write
        // loop fail to advance, and anything above the Bluetooth cap would be
        // rejected by the link layer.
        let reported = callback.get_chunk_size(path.clone());
        let chunk_size = reported.clamp(1, MAX_CHUNK_BYTES) as usize;
        Self {
            callback,
            path,
            chunk_size,
        }
    }
}

#[async_trait]
impl JadeTransport for CallbackTransport {
    async fn write_all(&self, data: Vec<u8>) -> Result<(), JadeError> {
        let callback = Arc::clone(&self.callback);
        let path = self.path.clone();
        let chunk_size = self.chunk_size;

        // Foreign callbacks are synchronous and can block. Running them on a
        // worker thread would park it for the duration; the blocking pool is
        // sized for exactly this.
        tokio::task::spawn_blocking(move || {
            for chunk in data.chunks(chunk_size) {
                let result = callback.write_chunk(path.clone(), chunk.to_vec());
                if !result.success {
                    return Err(to_error(result.error_code, result.error));
                }
            }
            Ok(())
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("write task failed: {error}"),
        })?
    }

    async fn read_some(&self, timeout: Duration) -> Result<Vec<u8>, JadeError> {
        let callback = Arc::clone(&self.callback);
        let path = self.path.clone();
        let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;

        tokio::task::spawn_blocking(move || {
            let result = callback.read_chunk(path, timeout_ms);
            if !result.success {
                return Err(to_error(result.error_code, result.error));
            }
            Ok(result.data)
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("read task failed: {error}"),
        })?
    }

    async fn close(&self) -> Result<(), JadeError> {
        let callback = Arc::clone(&self.callback);
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let result = callback.close_device(path);
            if !result.success {
                return Err(to_error(result.error_code, result.error));
            }
            Ok(())
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("close task failed: {error}"),
        })?
    }
}
