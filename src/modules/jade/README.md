# Jade Module - Technical Overview

Blockstream Jade support for bitkit-core, over Bluetooth (all platforms) and USB
CDC serial (desktop and Python). Bitcoin single signature only.

The protocol itself lives in
[`jade-client-rs`](https://github.com/coreyphillips/jade-client-rs). This module
is the FFI adapter. For the wire format, the pinserver exchange, PSBT checks and
the transport contract, read that crate's documentation; what follows is only
what is specific to bitkit-core.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                     bitkit-android / bitkit-ios                       │
│   implements JadeTransportCallback: BLE, and USB host on Android      │
└───────────────────────────────┬──────────────────────────────────────┘
                                │ UniFFI
┌───────────────────────────────▼──────────────────────────────────────┐
│                            bitkit-core                                │
│  lib.rs             jade_* exports over a global JadeManager          │
│  implementation.rs  session lock, device list, abort handle           │
│  callbacks.rs       JadeTransportCallback + bridge to JadeTransport   │
│  types.rs           #[uniffi::remote] scaffolding for crate types     │
└───────────────────────────────┬──────────────────────────────────────┘
                                │
┌───────────────────────────────▼──────────────────────────────────────┐
│                          jade-client-rs                               │
│  CBOR protocol, pinserver, PSBT checks, serial transport              │
└──────────────────────────────────────────────────────────────────────┘
```

## Why the types are declared with `#[uniffi::remote]`

The crate's types carry no binding framework. `types.rs` attaches UniFFI
scaffolding to them from here, which generates the same code a
`#[derive(uniffi::…)]` would without a mirrored set of structs.

This is the main way this module differs from `trezor`, which predates the
technique: that module maintains roughly 900 lines of parallel types and
hand-written `From` conversions in both directions against
`trezor-connect-rs`. The declarations here have to match upstream field for
field, and the compiler enforces it.

One consequence worth knowing: `#[uniffi::remote(Error)]` needs to match every
variant, so `jade_client_rs::JadeError` deliberately is not `#[non_exhaustive]`.

## Session state

`jade_client_rs::Jade` takes `&mut self` per operation, so the one request at a
time rule is a compile time property there. A free-function FFI surface needs a
process global, so `JadeManager` supplies the lock that implies.

The abort handle is kept outside that lock on purpose. Sharing one lock would
make `jade_disconnect` and every status read queue behind a five minute
confirmation, and UniFFI async exports are detached onto the runtime, so a
cancelled Swift or Kotlin task does not cancel the Rust future by itself.
`jade_cancel` and `jade_disconnect` therefore close the transport through a
`CancelHandle` without taking the session lock.

## Transport bridge

`JadeTransportCallback` is the `#[uniffi::export(with_foreign)]` trait the
application implements; `CallbackTransport` adapts it onto the crate's
`JadeTransport`. Every callback invocation runs on the tokio blocking pool, so a
slow implementation costs a blocking thread rather than a runtime worker.

The full Bluetooth contract, including the two second inter-chunk deadline and
the write-with-response requirement, is documented on the trait and in the
crate's README. Read it before writing a native implementation; each of those
rules fails only against real hardware.

Errors cross the boundary as a typed `JadeTransportErrorCode` rather than an
error string. The trezor adapter has to encode its code into a sentinel string
and parse it back out, because its upstream crate offers no typed channel.

## Signing

Jade returns a signed PSBT, so it follows the Passport path:

```
onchain_compose_transaction  ->  psbt (base64)
jade_sign_psbt               ->  signed psbt (base64)
finalize_psbt(original, signed) -> CompletedTransaction
onchain_broadcast_raw_tx
```

`WalletParams.fingerprint` must be set to the value from
`jade_get_master_fingerprint`, or the composed PSBT carries no BIP32 key origins
and the device signs nothing. The crate rejects that case before the round trip
with `FingerprintMismatch`.

## Constraints

- No `#[uniffi::export]` item here may be `cfg` gated. All three build scripts
  generate bindings from the host library, so a host only export would appear in
  the generated Swift and Kotlin while being absent from the device library.
- No `u8` or `u16` in the FFI surface. `ping` returns `JadePingStatus` and
  `battery_status` is `u32`, keeping this module clear of the narrow unsigned
  return path that needed a binding generator fix for Android ARM32.
- Registering a transport callback twice replaces the first, so an Android
  activity restart can re-register. The replacement is logged.

## Dependency

Pinned by git revision until the crate is published to crates.io, so bitkit-core
never depends on an unreleased version. Bumping it means updating both target
tables in `Cargo.toml`.

## Testing

```bash
cargo test modules::jade          # adapter only
```

Protocol level tests live in the crate and run with `cargo test` there, against
a scripted mock device and a fake pinserver.
