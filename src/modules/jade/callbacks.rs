//! The transport contract the native application implements.
//!
//! Rust owns the Jade protocol; the application owns the bytes. On iOS that
//! means CoreBluetooth against the Nordic UART Service, and on Android the
//! Bluetooth API plus, optionally, the USB Host API for CDC serial. Desktop and
//! Python builds can skip this entirely and use the Rust serial transport.
//!
//! Methods are synchronous. UniFFI can express async foreign callbacks, but the
//! trezor module established the synchronous shape here and the transport layer
//! runs every one of these on the blocking pool, so there is nothing to gain by
//! diverging.

use std::sync::{Arc, RwLock};

use super::types::JadeTransportKind;

/// A failure the native layer can report in a way Rust can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JadeTransportErrorCode {
    /// Another operation holds the device.
    DeviceBusy,
    /// The device is not currently open.
    NotConnected,
    /// The link dropped.
    Disconnected,
    /// The operation exceeded its deadline.
    Timeout,
    /// The OS refused access, typically a missing Bluetooth or USB permission.
    PermissionDenied,
}

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
///    message after two seconds of silence (three on Jade v1) and answers with
///    an unattributed error. A 30 KB PSBT is roughly 60 writes, so any UI thread
///    stall in the middle of a send breaks the operation.
/// 3. **`read_chunk` must return promptly.** Honour `timeout_ms`, which this
///    crate keeps short. The long per-operation deadline is enforced in Rust so
///    the user can cancel; blocking here for minutes would defeat that.
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
    /// For Bluetooth this is `min(negotiated_mtu - 3, 509)`. Rust clamps the
    /// answer into a usable range, so an unnegotiated `0` is not fatal.
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
/// Any live connection is invalidated by the caller before this takes effect.
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
