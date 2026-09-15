use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tokio::sync::Semaphore;

use super::{
    jade_set_transport_callback, JadeError, JadeManager, JadeNativeDevice, JadeNetwork,
    JadeTransportCallback, JadeTransportKind, JadeTransportReadResult, JadeTransportResult,
};

#[derive(Default)]
struct NativeState {
    open: HashSet<String>,
    closed: Vec<String>,
    replies: HashMap<String, VecDeque<Vec<u8>>>,
}

struct Callback {
    state: Mutex<NativeState>,
    hold_replies: AtomicBool,
    hold_open: Mutex<bool>,
    open_gate: Condvar,
    hold_close: Mutex<bool>,
    close_gate: Condvar,
    fail_read: AtomicBool,
    reads_after_close: AtomicBool,
    only_first_close: AtomicBool,
    close_count: AtomicUsize,
    closes: Semaphore,
    opens: Semaphore,
    writes: Semaphore,
}

impl Callback {
    fn new() -> Arc<Self> {
        let callback = Arc::new(Self {
            state: Mutex::new(NativeState::default()),
            hold_replies: AtomicBool::new(true),
            hold_open: Mutex::new(false),
            open_gate: Condvar::new(),
            hold_close: Mutex::new(false),
            close_gate: Condvar::new(),
            fail_read: AtomicBool::new(false),
            reads_after_close: AtomicBool::new(false),
            only_first_close: AtomicBool::new(false),
            close_count: AtomicUsize::new(0),
            closes: Semaphore::new(0),
            opens: Semaphore::new(0),
            writes: Semaphore::new(0),
        });
        jade_set_transport_callback(callback.clone());
        callback
    }

    fn release_open(&self) {
        *self.hold_open.lock().unwrap() = false;
        self.open_gate.notify_all();
    }

    fn release_close(&self) {
        *self.hold_close.lock().unwrap() = false;
        self.close_gate.notify_all();
    }

    fn release_replies(&self) {
        self.hold_replies.store(false, Ordering::SeqCst);
    }

    fn open_paths(&self) -> HashSet<String> {
        self.state.lock().unwrap().open.clone()
    }
}

fn success() -> JadeTransportResult {
    JadeTransportResult {
        success: true,
        error: String::new(),
        error_code: None,
    }
}

fn reply(data: &[u8]) -> Vec<u8> {
    let mut decoder = minicbor::Decoder::new(data);
    let fields = decoder.map().unwrap().unwrap();
    let mut id = "";
    let mut method = "";
    for _ in 0..fields {
        match decoder.str().unwrap() {
            "id" => id = decoder.str().unwrap(),
            "method" => method = decoder.str().unwrap(),
            _ => decoder.skip().unwrap(),
        }
    }
    let mut encoder = minicbor::Encoder::new(Vec::new());
    encoder.map(2).unwrap().str("id").unwrap().str(id).unwrap();
    encoder.str("result").unwrap();
    if method == "get_version_info" {
        encoder
            .map(2)
            .unwrap()
            .str("JADE_VERSION")
            .unwrap()
            .str("1.0.41")
            .unwrap()
            .str("JADE_STATE")
            .unwrap()
            .str("READY")
            .unwrap();
    } else {
        encoder.bool(true).unwrap();
    }
    encoder.into_writer()
}

impl JadeTransportCallback for Callback {
    fn scan_devices(&self, _timeout_ms: u32) -> Vec<JadeNativeDevice> {
        Vec::new()
    }

    fn open_device(&self, path: String) -> JadeTransportResult {
        self.opens.add_permits(1);
        let mut hold = self.hold_open.lock().unwrap();
        while *hold {
            hold = self.open_gate.wait(hold).unwrap();
        }
        self.state.lock().unwrap().open.insert(path);
        success()
    }

    fn close_device(&self, path: String) -> JadeTransportResult {
        let first_close = self.close_count.fetch_add(1, Ordering::SeqCst) == 0;
        self.closes.add_permits(1);
        let must_wait = first_close || !self.only_first_close.load(Ordering::SeqCst);
        let mut hold = self.hold_close.lock().unwrap();
        while *hold && must_wait {
            hold = self.close_gate.wait(hold).unwrap();
        }
        let mut state = self.state.lock().unwrap();
        state.open.remove(&path);
        state.closed.push(path);
        success()
    }

    fn write_chunk(&self, path: String, data: Vec<u8>) -> JadeTransportResult {
        self.state
            .lock()
            .unwrap()
            .replies
            .entry(path)
            .or_default()
            .push_back(reply(&data));
        self.writes.add_permits(1);
        success()
    }

    fn read_chunk(&self, path: String, _timeout_ms: u32) -> JadeTransportReadResult {
        let released = !self.reads_after_close.load(Ordering::SeqCst)
            && !self.state.lock().unwrap().open.contains(&path);
        if self.fail_read.load(Ordering::SeqCst) || released {
            return JadeTransportReadResult {
                success: false,
                data: Vec::new(),
                error: String::new(),
                error_code: Some(super::JadeTransportErrorCode::Disconnected),
            };
        }
        let data = if self.hold_replies.load(Ordering::SeqCst) {
            Vec::new()
        } else {
            self.state
                .lock()
                .unwrap()
                .replies
                .entry(path)
                .or_default()
                .pop_front()
                .unwrap_or_default()
        };
        JadeTransportReadResult {
            success: true,
            data,
            error: String::new(),
            error_code: None,
        }
    }

    fn get_chunk_size(&self, _path: String) -> u32 {
        509
    }
}

async fn wait_for(signal: &Semaphore) {
    tokio::time::timeout(Duration::from_secs(2), signal.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
}

fn connect(
    manager: &Arc<JadeManager>,
    path: &str,
) -> tokio::task::JoinHandle<Result<super::JadeVersionInfo, JadeError>> {
    let manager = Arc::clone(manager);
    let path = path.to_string();
    tokio::spawn(async move { manager.connect(JadeTransportKind::Bluetooth, &path).await })
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn disconnect_during_handshake_closes_the_pending_connection() {
    let callback = Callback::new();
    let manager = Arc::new(JadeManager::new());
    let connection = connect(&manager, "jade");
    wait_for(&callback.writes).await;

    tokio::time::timeout(Duration::from_secs(1), manager.disconnect())
        .await
        .unwrap()
        .unwrap();
    callback.release_replies();

    assert!(matches!(
        connection.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    assert!(!manager.is_connected());
    assert!(manager.connected_device().await.is_none());
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn cancel_during_handshake_closes_the_pending_connection() {
    let callback = Callback::new();
    let manager = Arc::new(JadeManager::new());
    let connection = connect(&manager, "jade");
    wait_for(&callback.writes).await;

    tokio::time::timeout(Duration::from_secs(1), manager.cancel())
        .await
        .unwrap()
        .unwrap();
    callback.release_replies();

    assert!(matches!(
        connection.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    assert!(!manager.is_connected());
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn overlapping_connections_close_the_previous_native_handle() {
    let callback = Callback::new();
    let manager = Arc::new(JadeManager::new());
    let first = connect(&manager, "first");
    wait_for(&callback.writes).await;
    let second = connect(&manager, "second");

    // Poll the second connection until it either queues or opens its native handle.
    let _ = tokio::time::timeout(Duration::from_millis(50), callback.opens.acquire_many(2)).await;
    callback.release_replies();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();

    assert_eq!(manager.connected_device().await.unwrap().path, "second");
    assert_eq!(callback.open_paths(), HashSet::from(["second".to_string()]));
    manager.disconnect().await.unwrap();
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn scan_refuses_to_interrupt_a_pending_handshake() {
    let callback = Callback::new();
    let manager = Arc::new(JadeManager::new());
    let connection = connect(&manager, "jade");
    wait_for(&callback.writes).await;

    let result = manager.scan(1).await;
    manager.disconnect().await.unwrap();
    callback.release_replies();
    let _ = connection.await.unwrap();
    assert!(matches!(result, Err(JadeError::DeviceBusy)));
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn disconnect_waits_for_native_open_and_closes_its_result() {
    let callback = Callback::new();
    *callback.hold_open.lock().unwrap() = true;
    let manager = Arc::new(JadeManager::new());
    let connection = connect(&manager, "jade");
    wait_for(&callback.opens).await;

    let disconnect = manager.disconnect();
    tokio::pin!(disconnect);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut disconnect)
            .await
            .is_err()
    );
    callback.release_open();
    disconnect.await.unwrap();

    assert!(matches!(
        connection.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    assert!(!manager.is_connected());
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn disconnect_invalidates_a_connection_queued_behind_native_open() {
    let callback = Callback::new();
    *callback.hold_open.lock().unwrap() = true;
    let manager = Arc::new(JadeManager::new());
    let first = connect(&manager, "first");
    wait_for(&callback.opens).await;
    let second = manager.connect(JadeTransportKind::Bluetooth, "second");
    tokio::pin!(second);
    let queued = tokio::time::timeout(Duration::from_millis(20), &mut second)
        .await
        .is_err();
    let disconnect = manager.disconnect();
    tokio::pin!(disconnect);
    let waiting = tokio::time::timeout(Duration::from_millis(20), &mut disconnect)
        .await
        .is_err();
    callback.release_open();

    assert!(queued && waiting);
    assert!(matches!(
        first.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    assert!(matches!(second.await, Err(JadeError::UserCancelled)));
    disconnect.await.unwrap();
    assert_eq!(callback.opens.available_permits(), 0);
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn cancellation_during_previous_teardown_does_not_open_a_replacement() {
    let callback = Callback::new();
    callback.release_replies();
    let manager = Arc::new(JadeManager::new());
    connect(&manager, "first").await.unwrap().unwrap();
    wait_for(&callback.opens).await;
    *callback.hold_close.lock().unwrap() = true;
    let replacement = connect(&manager, "second");
    wait_for(&callback.closes).await;

    let cancel = manager.cancel();
    tokio::pin!(cancel);
    let waiting = tokio::time::timeout(Duration::from_millis(20), &mut cancel)
        .await
        .is_err();
    callback.release_close();

    assert!(waiting);
    assert!(matches!(
        replacement.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    cancel.await.unwrap();
    assert_eq!(callback.opens.available_permits(), 0);
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn cancellation_waits_for_failed_handshake_cleanup_before_reconnecting() {
    let callback = Callback::new();
    callback.fail_read.store(true, Ordering::SeqCst);
    callback.only_first_close.store(true, Ordering::SeqCst);
    *callback.hold_close.lock().unwrap() = true;
    let manager = Arc::new(JadeManager::new());
    let first = connect(&manager, "jade");
    wait_for(&callback.closes).await;
    wait_for(&callback.opens).await;

    let cancel = manager.cancel();
    tokio::pin!(cancel);
    let waiting = tokio::time::timeout(Duration::from_millis(20), &mut cancel)
        .await
        .is_err();
    callback.fail_read.store(false, Ordering::SeqCst);
    callback.release_replies();
    let replacement = connect(&manager, "jade");
    let opened_early = tokio::time::timeout(Duration::from_millis(20), callback.opens.acquire())
        .await
        .is_ok();
    callback.release_close();

    assert!(waiting && !opened_early);
    assert!(matches!(
        first.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    cancel.await.unwrap();
    replacement.await.unwrap().unwrap();
    assert_eq!(callback.open_paths(), HashSet::from(["jade".to_string()]));
    manager.disconnect().await.unwrap();
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn disconnect_does_not_wait_for_a_native_layer_that_keeps_reading_after_close() {
    let callback = Callback::new();
    callback.reads_after_close.store(true, Ordering::SeqCst);
    let manager = Arc::new(JadeManager::new());
    let connection = connect(&manager, "jade");
    wait_for(&callback.writes).await;

    tokio::time::timeout(Duration::from_secs(1), manager.disconnect())
        .await
        .unwrap()
        .unwrap();

    assert!(matches!(
        connection.await.unwrap(),
        Err(JadeError::UserCancelled)
    ));
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn native_disconnect_clears_only_the_connected_path() {
    let callback = Callback::new();
    callback.release_replies();
    let manager = Arc::new(JadeManager::new());
    connect(&manager, "jade").await.unwrap().unwrap();

    manager.notify_disconnected("other").await;
    assert!(manager.is_connected());
    assert!(manager.version_info().await.is_some());

    manager.notify_disconnected("jade").await;
    assert!(!manager.is_connected());
    assert!(manager.connected_device().await.is_none());
    assert!(manager.version_info().await.is_none());
    assert!(callback.open_paths().is_empty());
}

#[tokio::test]
#[serial_test::serial(jade_callback)]
async fn version_info_does_not_wait_for_an_operation_in_flight() {
    let callback = Callback::new();
    callback.release_replies();
    let manager = Arc::new(JadeManager::new());
    connect(&manager, "jade").await.unwrap().unwrap();
    callback.hold_replies.store(true, Ordering::SeqCst);
    callback.writes.forget_permits(usize::MAX);
    let ping = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move { manager.ping().await })
    };
    wait_for(&callback.writes).await;

    let version = tokio::time::timeout(Duration::from_millis(200), manager.version_info())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(version.jade_version, "1.0.41");
    manager.disconnect().await.unwrap();
    assert!(ping.await.unwrap().is_err());
}

#[tokio::test]
async fn malformed_base64_psbt_is_rejected_before_reaching_the_device() {
    let manager = JadeManager::new();

    let result = manager
        .sign_psbt(JadeNetwork::Regtest, "not a psbt!".to_string())
        .await;

    assert!(matches!(result, Err(JadeError::InvalidPsbt { .. })));
}
