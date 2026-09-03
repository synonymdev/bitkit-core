# Jade Module - Technical Overview

Blockstream Jade support for bitkit-core, over Bluetooth (all platforms) and USB
CDC serial (desktop and Python). Bitcoin single signature only.

Unlike the `trezor` module, which adapts the external `trezor-connect-rs` crate,
there is no Rust crate for Jade, so the protocol is implemented here.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                     bitkit-android / bitkit-ios                       │
│   JadeTransport.kt / JadeTransport.swift                              │
│      implements JadeTransportCallback: BLE, and USB host on Android   │
└───────────────────────────────┬──────────────────────────────────────┘
                                │ UniFFI
┌───────────────────────────────▼──────────────────────────────────────┐
│                            bitkit-core                                │
│  lib.rs            jade_* exports over a global JadeManager           │
│  implementation.rs session state, one connection, abort handling      │
│  pinserver.rs      auth_user -> http_request -> pin, over reqwest     │
│  transport.rs      JadeConnection: framing, correlation, reassembly   │
│  protocol.rs       pure CBOR framing and envelopes                    │
│  serial.rs         Rust serial transport (desktop and Python only)    │
└──────────────────────────────────────────────────────────────────────┘
```

## Wire protocol

CBOR maps written back to back with no length prefix and no framing bytes.
Requests are `{"id", "method", "params"}`; replies are `{"id", "result"}` or
`{"id", "error": {"code", "message"}}`. The device also emits unsolicited
`{"log": ...}` frames with no `id`, which are skipped.

Because CBOR is self delimiting, the reader buffers bytes and attempts an
incremental decode after each read. `protocol::try_take_frame` uses
`minicbor::Decoder::skip()` for that, because it reports an exact consumed byte
count; `ciborium` then deserializes the complete frame.

Three rules that are easy to get wrong:

- **Binary fields must be CBOR byte strings.** serde encodes a plain `Vec<u8>` as
  an array of integers, and Jade reads `psbt` and `entropy` with
  `rpc_get_bytes_ptr`, which requires major type 2. Every binary field carries
  `#[serde(with = "serde_bytes")]`. A test asserts the encoded header byte.
- **Absent params are omitted, not encoded as null.** Jade's typed getters treat
  a null as missing and then fail with `BAD_PARAMETERS`.
- **Replies with id `"00"` are terminal errors, not stray frames.** Jade uses that
  id when it rejects a message before recovering the real one, for example an
  oversize or malformed request. Discarding them would turn every such rejection
  into a full length timeout.

## Native transport contract

Jade advertises the Nordic UART Service:

| Role | UUID |
|---|---|
| Service | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` |
| Write (host to Jade) | `6e400002-b5a3-f393-e0a9-e50e24dcca9e` |
| Notify (Jade to host) | `6e400003-b5a3-f393-e0a9-e50e24dcca9e` |

Devices advertise as `Jade` or `Jade <serial>`.

Requirements on the native implementation:

1. **Write with response.** Write-without-response silently drops chunks on the
   ESP32 GATT stack.
2. **Do not pause between chunks of one request.** Firmware discards a partially
   received message after two seconds of silence, three on Jade v1, and answers
   with an unattributed error. A 30 KB PSBT is roughly 60 writes, so a UI thread
   stall mid send breaks signing.
3. **`get_chunk_size` returns `min(negotiated_mtu - 3, 509)`.** Rust clamps the
   answer to `1..=509`, so an unnegotiated `0` is not fatal.
4. **`read_chunk` returns promptly**, honouring the short `timeout_ms` it is
   given. Returning success with an empty vector means "nothing yet" and is the
   normal state while the user is deciding. The long per-operation deadline is
   enforced in Rust so the user can cancel.

Every callback invocation runs on the tokio blocking pool, so a slow
implementation costs a blocking thread rather than a runtime worker.

## Serial

115200 baud. Ports are matched on the USB descriptors Jade and its DIY bridge
chips present:

| VID:PID | Chip |
|---|---|
| `10c4:ea60` | Silicon Labs CP210x, Jade v1 |
| `1a86:55d4` | WCH CH9102 |
| `0403:6001` | FTDI FT232 |
| `1a86:7523` | WCH CH340 |
| `303a:4001` | Espressif native USB, Jade Plus |
| `303a:1001` | Espressif USB serial/JTAG |

DTR and RTS are cleared on open and close; leaving either asserted resets the
ESP32 on several of these bridges.

`serialport` is declared with `default-features = false` because its default
`libudev` feature links a C library that CI does not install, and
`build_android.sh` performs a host build.

## Connection flow

1. `jade_scan` collects devices from the transport callback and, on desktop, from
   serial enumeration. It returns `DeviceBusy` while a connection is open,
   because starting a Bluetooth scan during an active link drops it on Android.
2. `jade_connect` closes anything already open, opens the transport, reads
   `get_version_info`, and contributes 32 bytes of host entropy via `add_entropy`.
3. The returned `jade_state` decides what happens next: `Locked` means call
   `jade_unlock`, `Ready` means the device is usable, `Uninit` means the user has
   to create or restore a wallet on the device itself, which the host cannot
   drive.
4. `jade_unlock` sends `auth_user`. If the device answers with an `http_request`,
   the host performs it and feeds the reply back as the `pin` method's params.

## Unlock and the pinserver

Jade's PIN protection is backed by a blind pinserver. The exchange is end to end
encrypted between device and server, so the host never learns the PIN; it only
carries bytes. Two details matter:

- The HTTP response is JSON and must be decoded into a CBOR **map**. Firmware
  requires `params` to be a map with a text `data` member, so forwarding raw
  bytes fails every unlock.
- An HTTP failure must still send a `pin` message, with no `params`. The device
  blocks indefinitely waiting for one, so abandoning the exchange would leave it
  consuming the next unrelated request as the awaited reply, putting every later
  call one message out of step.

Because the URL list comes from the device, requests are constrained to https,
port 443, no credentials, no redirects, no onion hosts, a resolved address that
is not loopback, private, link local, CGNAT or unique local, and a 64 KiB body
cap. A non-default pinserver host is logged as a warning: a second hand or
tampered unit can carry a pinserver a previous owner configured.

## Signing

Jade returns a **signed PSBT**, so it follows the Passport path rather than the
Trezor one:

```
onchain_compose_transaction  ->  psbt (base64)
jade_sign_psbt               ->  signed psbt (base64)
finalize_psbt(original, signed) -> CompletedTransaction
onchain_broadcast_raw_tx
```

`jade_sign_psbt` checks the reply against what was sent before returning, so the
guarantee holds here even for a caller that does not go on to use
`finalize_psbt`: same unsigned transaction, same input and output counts,
unchanged previous output metadata, and at least one new signature.

Before the round trip it also rejects a PSBT larger than the device's input
buffer, an unsupported sighash type, and a PSBT whose BIP32 origins carry no
input for the connected device's master fingerprint. That last one is the most
likely integration failure: `WalletParams.fingerprint` must be set to the value
from `jade_get_master_fingerprint`, or `compose_transaction` produces a PSBT with
no key origins and the device signs nothing.

## Addresses

`jade_verify_address` takes the address the application is about to display and
asks the device to show its own derivation for the same path, failing with
`AddressMismatch` if they disagree. Jade always prompts on screen for this call,
so it is a verification step rather than a way to fetch an address. It catches
corruption and firmware bugs; a wholly malicious device is still caught by the
user reading the device screen.

## Cancellation

Jade has no cancel message, so `jade_cancel` and `jade_disconnect` close the
link. Both set an abort flag and close the transport **without** taking the I/O
lock, so a request blocked on a five minute confirmation returns
`UserCancelled` promptly instead of running out its deadline. UniFFI async
exports are detached onto the runtime, so a cancelled Swift or Kotlin task does
not cancel the Rust future by itself; this is the mechanism that does.

## Common issues

| Symptom | Cause |
|---|---|
| Every `sign_psbt` fails with `DeviceError` | Binary field encoded as a CBOR array rather than a byte string |
| Signing fails partway through a large PSBT | A pause longer than two seconds between chunks, or write-without-response |
| `FingerprintMismatch` | `WalletParams.fingerprint` was not set when composing |
| `UnsupportedFirmware` on a taproot address | Taproot addresses need firmware 1.0.34 or newer |
| `NetworkMismatch` | The device was unlocked for a different network |
| Unlock hangs, later calls report protocol errors | The `pin` follow-up was skipped after an HTTP failure |
| `DeviceUninitialized` | The wallet must be created or restored on the device itself |

## Constraints

- No `#[uniffi::export]` item in this module may be `cfg` gated. All three build
  scripts generate bindings from the **host** library, so a host only export
  would appear in the generated Swift and Kotlin while being absent from the
  device library: a link failure on iOS and a checksum mismatch on Android.
- No `u8` or `u16` in the FFI surface. `ping` returns `JadePingStatus` and
  `battery_status` is `u32`, keeping this module clear of the unsigned narrow
  return path that needed a binding generator fix for Android ARM32.
- Registering a transport callback twice replaces the first. This is deliberate,
  so an Android activity restart can re-register; the replacement is logged.

## Testing

```bash
cargo test modules::jade
```

Everything runs against a scripted mock device and a fake pinserver, so no
hardware or network access is needed. Covered: byte string encoding, frame
reassembly across reads, two frames in one read, log frame skipping, stale and
unattributed replies, error code mapping, multi fragment `sign_psbt` reassembly,
cancellation, connection poisoning, path validation, and the unlock exchange
including the HTTP failure path.
