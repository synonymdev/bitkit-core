# Trezor Module - Technical Overview

This module provides Trezor hardware wallet integration for bitkit-core, supporting both USB and Bluetooth (BLE) connections on mobile (Android/iOS) and desktop platforms.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              bitkit-android                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│  TrezorTestView.kt    TrezorViewModel.kt    TrezorRepo.kt    TrezorService.kt│
│       (UI)                (State)            (State Mgmt)     (FFI Bridge)   │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │ UniFFI
┌──────────────────────────────────▼──────────────────────────────────────────┐
│                               bitkit-core                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│  lib.rs (exports)  ←→  TrezorManager  ←→  TrezorTransportCallback (trait)   │
│                              │                        ▲                      │
│                              │                        │ implemented by       │
│                              ▼                        │ native layer         │
│                       ConnectedDevice                 │                      │
└──────────────────────────────┬────────────────────────┼─────────────────────┘
                               │                        │
┌──────────────────────────────▼────────────────────────┼─────────────────────┐
│                         trezor-connect-rs                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  CallbackTransport  ←→  THP Protocol  ←→  TransportCallback (trait)         │
│        │                     │                                               │
│        │              ┌──────┴──────┐                                        │
│        │              │  Handshake  │                                        │
│        │              │  Pairing    │                                        │
│        │              │  Encryption │                                        │
│        │              └─────────────┘                                        │
│        ▼                                                                     │
│  Protocol V1 (USB) / THP (BLE)                                              │
└─────────────────────────────────────────────────────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────────────────────┐
│                        Native Layer (Kotlin/Swift)                           │
├─────────────────────────────────────────────────────────────────────────────┤
│  TrezorTransport.kt implements TrezorTransportCallback                       │
│       │                                                                      │
│       ├── USB: Android USB Host API                                          │
│       └── BLE: Android Bluetooth API (CoreBluetooth on iOS)                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Key Components

### bitkit-core (`src/modules/trezor/`)

#### `types.rs`
FFI-compatible types exposed via UniFFI:
- `TrezorDeviceInfo` - Device metadata (id, transport type, name, model)
- `TrezorFeatures` - Device capabilities and state
- `TrezorGetAddressParams` / `TrezorAddressResponse` - Address derivation
- `TrezorSignMessageParams` / `TrezorSignedMessageResponse` - Message signing
- `TrezorVerifyMessageParams` - Signature verification
- `TrezorScriptType` - Bitcoin script types (P2PKH, P2SH-P2WPKH, P2WPKH, P2TR)

#### `errors.rs`
Error types with UniFFI support:
- `TrezorError` - Main error enum with variants for transport, connection, device errors

#### `implementation.rs`
Core `TrezorManager` struct:
- Manages device lifecycle (scan, connect, disconnect)
- Stores connected `ConnectedDevice` for session reuse
- Bridges between UniFFI callbacks and trezor-connect-rs library

### lib.rs Exports

```rust
// Callback trait - implemented by native layer
#[uniffi::export(with_foreign)]
pub trait TrezorTransportCallback: Send + Sync {
    fn enumerate_devices(&self) -> Vec<NativeDeviceInfo>;
    fn open_device(&self, path: String) -> TrezorTransportWriteResult;
    fn close_device(&self, path: String) -> TrezorTransportWriteResult;
    fn read_chunk(&self, path: String) -> TrezorTransportReadResult;
    fn write_chunk(&self, path: String, data: Vec<u8>) -> TrezorTransportWriteResult;
    fn get_chunk_size(&self, path: String) -> u32;
    fn get_pairing_code(&self) -> String;
    fn save_thp_credential(&self, device_id: String, credential_json: String) -> bool;
    fn load_thp_credential(&self, device_id: String) -> Option<String>;
}

// Main API functions
pub async fn trezor_initialize(credential_path: Option<String>) -> Result<(), TrezorError>;
pub async fn trezor_scan() -> Result<Vec<TrezorDeviceInfo>, TrezorError>;
pub async fn trezor_connect(device_id: String) -> Result<TrezorFeatures, TrezorError>;
pub async fn trezor_get_address(params: TrezorGetAddressParams) -> Result<TrezorAddressResponse, TrezorError>;
pub async fn trezor_sign_message(params: TrezorSignMessageParams) -> Result<TrezorSignedMessageResponse, TrezorError>;
pub async fn trezor_verify_message(params: TrezorVerifyMessageParams) -> Result<bool, TrezorError>;
pub async fn trezor_disconnect() -> Result<(), TrezorError>;
```

## Connection Flow

### 1. Initialization
```
App Start
    │
    ▼
trezor_set_transport_callback(callback)  ← Native layer provides I/O implementation
    │
    ▼
trezor_initialize(credential_path)       ← Validates callback is set
```

### 2. Device Discovery
```
trezor_scan()
    │
    ▼
callback.enumerate_devices()              ← Native scans USB + BLE
    │
    ▼
Returns Vec<TrezorDeviceInfo>            ← Device list with transport type
```

### 3. Connection (Critical Path)
```
trezor_connect(device_id)
    │
    ▼
callback.open_device(path)               ← Native opens USB/BLE connection
    │
    ▼
CallbackTransport::new(adapter)          ← Create transport wrapper
    │
    ▼
transport.acquire(path, None)            ← Acquire session
    │                                       │
    │                    ┌──────────────────┴──────────────────┐
    │                    │           USB Path                   │
    │                    │  Protocol V1 (unencrypted chunks)   │
    │                    └─────────────────────────────────────┘
    │                                       │
    │                    ┌──────────────────┴──────────────────┐
    │                    │           BLE Path                   │
    │                    │                                      │
    │                    │  1. Channel Allocation               │
    │                    │  2. THP Handshake                    │
    │                    │     - Load stored credentials?       │
    │                    │     - If yes: try_to_unlock=true     │
    │                    │     - If no: fresh pairing           │
    │                    │  3. Pairing (if needed)              │
    │                    │     - callback.get_pairing_code()    │
    │                    │     - User enters 6-digit code       │
    │                    │  4. Save credentials                 │
    │                    │     - callback.save_thp_credential() │
    │                    │  5. Encrypted channel established    │
    │                    └─────────────────────────────────────┘
    │
    ▼
ConnectedDevice::new(info, transport, session)
    │
    ▼
device.initialize()                      ← Get device features
    │
    ▼
Store in TrezorManager.connected_device  ← REUSED for all operations
```

### 4. Operations (Reuse Connected Device)
```
trezor_get_address(params)
    │
    ▼
TrezorManager.connected_device.lock()    ← Get stored device
    │
    ▼
device.get_address(params)               ← Uses existing encrypted channel
    │
    ▼
Return address                           ← No new handshake needed!
```

**Important:** The `ConnectedDevice` is stored and reused for all operations. This is critical for BLE - creating a new transport for each operation would trigger a fresh THP handshake and pairing prompt every time.

### 5. Disconnection
```
trezor_disconnect()
    │
    ▼
device.disconnect()                      ← Release session
    │
    ▼
callback.close_device(path)              ← Native closes connection
    │
    ▼
Clear TrezorManager.connected_device     ← Ready for new connection
```

## THP Protocol (Bluetooth)

Trezor Host Protocol provides encrypted communication over BLE:

### Handshake Flow
```
Host                                    Trezor
  │                                        │
  │──── Channel Allocation Request ───────▶│
  │◀─── Channel Allocation Response ───────│
  │                                        │
  │──── Handshake Init (ephemeral key) ───▶│
  │◀─── Handshake Init Response ───────────│
  │                                        │
  │──── Handshake Completion ─────────────▶│
  │◀─── Handshake Completion Response ─────│
  │                                        │
  │         [If pairing needed]            │
  │◀─── Pairing Request (code on screen) ──│
  │──── Pairing Code (user enters) ───────▶│
  │◀─── Credential Response ───────────────│
  │                                        │
  │         [Encrypted channel ready]      │
  │◀════ Encrypted Messages ══════════════▶│
```

### Credential Persistence
After successful pairing, credentials are saved:
```json
{
  "host_static_key": "hex-encoded-32-bytes",
  "trezor_static_public_key": "hex-encoded-32-bytes",
  "credential": "hex-encoded-credential-token"
}
```

On reconnection:
1. `load_thp_credential(device_id)` retrieves stored JSON
2. Handshake uses `try_to_unlock=true` with stored credential
3. If valid, pairing dialog is skipped

## Android Implementation (bitkit-android)

### TrezorTransport.kt
Implements `TrezorTransportCallback`:

```kotlin
@Singleton
class TrezorTransport @Inject constructor(
    private val context: Context,
    private val bleManager: TrezorBleManager,
    private val usbManager: TrezorUsbManager,
) : TrezorTransportCallback {

    // Device enumeration
    override fun enumerateDevices(): List<NativeDeviceInfo>

    // Connection management
    override fun openDevice(path: String): TrezorTransportWriteResult
    override fun closeDevice(path: String): TrezorTransportWriteResult

    // Raw I/O (64-byte USB chunks, 244-byte BLE chunks)
    override fun readChunk(path: String): TrezorTransportReadResult
    override fun writeChunk(path: String, data: ByteArray): TrezorTransportWriteResult
    override fun getChunkSize(path: String): UInt

    // Pairing UI
    override fun getPairingCode(): String  // Blocks, shows dialog, returns code

    // Credential storage
    override fun saveThpCredential(deviceId: String, credentialJson: String): Boolean
    override fun loadThpCredential(deviceId: String): String?
}
```

### TrezorService.kt
Suspending wrapper for FFI calls:
```kotlin
@Singleton
class TrezorService @Inject constructor(private val transport: TrezorTransport) {
    suspend fun initialize(credentialPath: String?) = ServiceQueue.CORE.background { ... }
    suspend fun scan(): List<TrezorDeviceInfo> = ServiceQueue.CORE.background { ... }
    suspend fun connect(deviceId: String): TrezorFeatures = ServiceQueue.CORE.background { ... }
    suspend fun getAddress(...): TrezorAddressResponse = ServiceQueue.CORE.background { ... }
    suspend fun signMessage(...): TrezorSignedMessageResponse = ServiceQueue.CORE.background { ... }
    suspend fun verifyMessage(...): Boolean = ServiceQueue.CORE.background { ... }
    suspend fun disconnect() = ServiceQueue.CORE.background { ... }
}
```

### TrezorRepo.kt
State management with Kotlin Flow:
```kotlin
data class TrezorState(
    val isInitialized: Boolean = false,
    val isScanning: Boolean = false,
    val isConnecting: Boolean = false,
    val devices: List<TrezorDeviceInfo> = emptyList(),
    val connectedDevice: TrezorFeatures? = null,
    val lastAddress: TrezorAddressResponse? = null,
    val error: String? = null,
)

@Singleton
class TrezorRepo @Inject constructor(...) {
    private val _state = MutableStateFlow(TrezorState())
    val state = _state.asStateFlow()

    val needsPairingCode = trezorTransport.needsPairingCode  // For UI dialog

    suspend fun initialize(): Result<Unit>
    suspend fun scan(): Result<List<TrezorDeviceInfo>>
    suspend fun connect(deviceId: String): Result<TrezorFeatures>
    suspend fun getAddress(...): Result<TrezorAddressResponse>
    // ...
}
```

### TrezorViewModel.kt
UI state and actions:
```kotlin
@HiltViewModel
class TrezorViewModel @Inject constructor(
    private val trezorRepo: TrezorRepo,
) : ViewModel() {
    val trezorState = trezorRepo.state.stateIn(...)
    val needsPairingCode = trezorRepo.needsPairingCode.stateIn(...)

    fun initialize() { ... }
    fun scan() { ... }
    fun connect() { ... }
    fun getAddress(showOnTrezor: Boolean) { ... }
    fun signMessage() { ... }
    fun verifyMessage() { ... }
    fun disconnect() { ... }

    fun submitPairingCode(code: String) { ... }
    fun cancelPairingCode() { ... }
}
```

## BLE Characteristics (Trezor Safe 3/5)

```
Service UUID:          8c000001-a59b-4d58-a9ad-073df69fa1b1
Read Characteristic:   8c000002-a59b-4d58-a9ad-073df69fa1b1
Write Characteristic:  8c000003-a59b-4d58-a9ad-073df69fa1b1
Chunk Size:            244 bytes
```

## Common Issues

### 1. Pairing Code Requested Every Time
**Cause:** Creating new `CallbackTransport` for each operation instead of reusing `ConnectedDevice`.
**Fix:** Store `ConnectedDevice` after `connect()` and reuse for all operations.

### 2. BLE Read Timeout
**Cause:** Multi-chunk responses not fully received.
**Fix:** Loop reading until complete message assembled (check continuation bit in THP header).

### 3. Credential Persistence Not Working
**Cause:** Credentials not saved after pairing or not loaded before handshake.
**Fix:** Ensure `save_thp_credential` called after successful pairing, `load_thp_credential` called before handshake.

## Building

### Build for Android
```bash
cd bitkit-core
./build_android.sh
cd ../bitkit-android
JAVA_HOME=/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home ./gradlew assembleDevDebug
```

### Regenerate Kotlin Bindings Only
```bash
cd bitkit-core
cargo build
cargo run --bin uniffi-bindgen generate \
    --library target/debug/libbitkitcore.dylib \
    --language kotlin \
    --out-dir bindings/android/lib/src/main/kotlin
```

## Testing

Use `TrezorTestView.kt` in the app (dev builds only) to test:
1. Initialize - Sets up transport callback
2. Scan - Discovers USB and BLE devices
3. Connect - Opens connection, triggers pairing for BLE
4. Get Address - Derives Bitcoin address
5. Sign Message - Signs with device confirmation
6. Verify Message - Verifies signature on device
7. Disconnect - Closes connection

## References

- [Trezor Documentation](https://docs.trezor.io/)
- [THP Protocol Specification](https://github.com/trezor/trezor-firmware/blob/main/docs/common/communication/thp.md)
- [UniFFI Documentation](https://mozilla.github.io/uniffi-rs/)
