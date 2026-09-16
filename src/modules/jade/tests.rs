//! Tests for the FFI adapter.
//!
//! Protocol level behaviour (framing, correlation, the unlock exchange, PSBT
//! checks) is tested in the `jade-client-rs` crate. What is left here is the
//! adapter: the account type mapping, and the bridge from the foreign callback
//! onto the crate's transport trait.

use super::callbacks::{
    CallbackTransport, JadeNativeDevice, JadeTransportCallback, JadeTransportReadResult,
    JadeTransportResult,
};
use super::types::{account_type_to_variant, JadeAddressVariant, JadeTransportKind};
use crate::onchain::AccountType;
use jade_client_rs::{JadeError, JadeTransport, MAX_CHUNK_BYTES};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn account_types_map_to_descriptor_variants() {
    let cases = [
        (AccountType::Legacy, JadeAddressVariant::Pkh),
        (AccountType::WrappedSegwit, JadeAddressVariant::ShWpkh),
        (AccountType::NativeSegwit, JadeAddressVariant::Wpkh),
        (AccountType::Taproot, JadeAddressVariant::Tr),
    ];
    for (account_type, expected) in cases {
        assert_eq!(account_type_to_variant(account_type), expected);
    }
}

/// A callback that records what it was asked to do.
struct MockCallback {
    chunk_size: u32,
    writes: Mutex<Vec<Vec<u8>>>,
    reads: Mutex<Vec<Vec<u8>>>,
    fail_write: bool,
}

impl MockCallback {
    fn with_chunk_size(chunk_size: u32) -> Arc<Self> {
        Arc::new(Self {
            chunk_size,
            writes: Mutex::new(Vec::new()),
            reads: Mutex::new(Vec::new()),
            fail_write: false,
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            chunk_size: 64,
            writes: Mutex::new(Vec::new()),
            reads: Mutex::new(Vec::new()),
            fail_write: true,
        })
    }
}

impl JadeTransportCallback for MockCallback {
    fn scan_devices(&self, _timeout_ms: u32) -> Vec<JadeNativeDevice> {
        vec![JadeNativeDevice {
            path: "AA:BB:CC:DD:EE:FF".to_string(),
            transport: JadeTransportKind::Bluetooth,
            name: Some("Jade C0FFEE".to_string()),
            serial_number: Some("C0FFEE".to_string()),
        }]
    }

    fn open_device(&self, _path: String) -> JadeTransportResult {
        JadeTransportResult {
            success: true,
            error: String::new(),
            error_code: None,
        }
    }

    fn close_device(&self, _path: String) -> JadeTransportResult {
        JadeTransportResult {
            success: true,
            error: String::new(),
            error_code: None,
        }
    }

    fn write_chunk(&self, _path: String, data: Vec<u8>) -> JadeTransportResult {
        if self.fail_write {
            return JadeTransportResult {
                success: false,
                error: "device went away".to_string(),
                error_code: Some(jade_client_rs::JadeTransportErrorCode::Disconnected),
            };
        }
        self.writes.lock().unwrap().push(data);
        JadeTransportResult {
            success: true,
            error: String::new(),
            error_code: None,
        }
    }

    fn read_chunk(&self, _path: String, _timeout_ms: u32) -> JadeTransportReadResult {
        let data = self.reads.lock().unwrap().pop().unwrap_or_default();
        JadeTransportReadResult {
            success: true,
            data,
            error: String::new(),
            error_code: None,
        }
    }

    fn get_chunk_size(&self, _path: String) -> u32 {
        self.chunk_size
    }
}

#[tokio::test]
async fn writes_are_split_at_the_reported_chunk_size() {
    let callback = MockCallback::with_chunk_size(4);
    let transport = CallbackTransport::new(Arc::clone(&callback) as Arc<_>, "path".to_string());

    transport
        .write_all(vec![1, 2, 3, 4, 5, 6, 7, 8, 9])
        .await
        .unwrap();

    let writes = callback.writes.lock().unwrap();
    assert_eq!(writes.len(), 3);
    assert_eq!(writes[0], vec![1, 2, 3, 4]);
    assert_eq!(writes[1], vec![5, 6, 7, 8]);
    assert_eq!(writes[2], vec![9]);
}

#[tokio::test]
async fn a_zero_chunk_size_does_not_stall_the_write_loop() {
    // A native implementation can report 0 before the MTU is negotiated.
    // Without clamping, chunks(0) panics and the loop never advances.
    let callback = MockCallback::with_chunk_size(0);
    let transport = CallbackTransport::new(Arc::clone(&callback) as Arc<_>, "path".to_string());

    transport.write_all(vec![1, 2, 3]).await.unwrap();

    let writes = callback.writes.lock().unwrap();
    assert_eq!(
        writes.len(),
        3,
        "a clamped size of 1 sends one byte per write"
    );
}

#[tokio::test]
async fn an_oversized_chunk_size_is_capped_to_the_bluetooth_limit() {
    let callback = MockCallback::with_chunk_size(100_000);
    let transport = CallbackTransport::new(Arc::clone(&callback) as Arc<_>, "path".to_string());

    let payload = vec![7u8; MAX_CHUNK_BYTES as usize + 10];
    transport.write_all(payload).await.unwrap();

    let writes = callback.writes.lock().unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].len(), MAX_CHUNK_BYTES as usize);
    assert_eq!(writes[1].len(), 10);
}

#[tokio::test]
async fn a_typed_transport_error_survives_the_bridge() {
    // The trezor adapter has to encode its error code into a string and parse it
    // back out, because its upstream crate offers no typed channel. This one
    // carries the code the whole way, so the mapping is exact.
    let callback = MockCallback::failing();
    let transport = CallbackTransport::new(callback as Arc<_>, "path".to_string());

    let error = transport.write_all(vec![1]).await.unwrap_err();
    assert_eq!(error, JadeError::DeviceDisconnected);
}

#[tokio::test]
async fn an_empty_read_is_not_an_error() {
    // Success with no data means "nothing yet", which is the normal state while
    // the user is deciding on the device.
    let callback = MockCallback::with_chunk_size(64);
    let transport = CallbackTransport::new(callback as Arc<_>, "path".to_string());

    let data = transport
        .read_some(Duration::from_millis(10))
        .await
        .unwrap();
    assert!(data.is_empty());
}

/// A callback whose `read_chunk` parks until the test releases it, so a close
/// can land while a read is already inside the native layer.
struct BlockingReadCallback {
    started: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl JadeTransportCallback for BlockingReadCallback {
    fn scan_devices(&self, _timeout_ms: u32) -> Vec<JadeNativeDevice> {
        Vec::new()
    }

    fn open_device(&self, _path: String) -> JadeTransportResult {
        JadeTransportResult {
            success: true,
            error: String::new(),
            error_code: None,
        }
    }

    fn close_device(&self, _path: String) -> JadeTransportResult {
        JadeTransportResult {
            success: true,
            error: String::new(),
            error_code: None,
        }
    }

    fn write_chunk(&self, _path: String, _data: Vec<u8>) -> JadeTransportResult {
        JadeTransportResult {
            success: true,
            error: String::new(),
            error_code: None,
        }
    }

    fn read_chunk(&self, _path: String, _timeout_ms: u32) -> JadeTransportReadResult {
        let started = self.started.lock().unwrap().take().expect("one read only");
        started.send(()).unwrap();
        let release = self.release.lock().unwrap().take().expect("one read only");
        release.recv().unwrap();
        JadeTransportReadResult {
            success: true,
            data: vec![1, 2, 3],
            error: String::new(),
            error_code: None,
        }
    }

    fn get_chunk_size(&self, _path: String) -> u32 {
        64
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bytes_arriving_after_close_are_discarded() {
    // `close` cannot interrupt a `read_chunk` that is already inside the native
    // layer, so the guard has to run after the callback returns too. Otherwise a
    // read that completes for a released path feeds its bytes back to the
    // parser.
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let callback = Arc::new(BlockingReadCallback {
        started: Mutex::new(Some(started_tx)),
        release: Mutex::new(Some(release_rx)),
    });
    let transport = Arc::new(CallbackTransport::new(
        callback as Arc<_>,
        "path".to_string(),
    ));

    let reader = tokio::spawn({
        let transport = Arc::clone(&transport);
        async move { transport.read_some(Duration::from_millis(250)).await }
    });

    // Only close once the read is parked inside the callback.
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
        .await
        .unwrap();
    transport.close().await.unwrap();
    release_tx.send(()).unwrap();

    let error = reader.await.unwrap().unwrap_err();
    assert_eq!(error, JadeError::DeviceDisconnected);
}
