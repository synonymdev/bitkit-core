# Changelog

## Unreleased

- Xpub watcher transaction events now include the synchronized wallet's next unused external receive address and full derivation path, allowing clients to render receive state without starting a second account scan.
- Android consumer keep rules now keep the JNA `Structure.FieldOrder` annotation type and annotated UniFFI struct classes through R8 full mode.
- The Android AAR now ships targeted R8 consumer keep rules for the UniFFI/JNA FFI surface, so consuming apps can enable R8 full mode without extra keep rules for this library.
- Add a generic hardware-wallet catalog with Foundation Passport support, multipart UR QR encoding and decoding, Passport single-signature account export parsing, and signed PSBT finalization across the UniFFI bindings.
- Swap status updates now reconcile against Boltz's REST status whenever a swap is (re)subscribed, both on `boltz_start_swap_updates` and when `boltz_create_reverse_swap` adds a swap to a running stream. A confirmed reverse-swap lockup is therefore caught up and auto-claimed even when its live WebSocket event was missed (for example because the updates stream was down while the lockup confirmed), instead of the swap silently stalling until a manual claim. No FFI signature change.
- `onchain_broadcast_raw_tx` now returns the transaction's canonical txid, computed locally in Rust, and treats Electrum "already known / already in mempool / already in block chain" responses as success (returning that same txid). This lets native apps complete Blocktank funding bookkeeping when they retry a broadcast after an ambiguous network failure, without relying on a signer-provided txid. Genuine connectivity failures and unrelated broadcast rejections remain typed `BroadcastError`s, and there is no FFI signature change.
- Surface a locked Trezor during the THP handshake as the typed `TrezorError::DeviceBusy` instead of a generic connection error, so mobile clients back off and prompt the user to unlock rather than reconnecting in a loop. Backed by `trezor-connect-rs` 0.3.4, which classifies `DeviceLocked` as a distinct, non-retryable state: it no longer churns the transport (close/reopen loop) on a locked device and instead makes a single `try_to_unlock` handshake attempt so the device prompts for unlock.

## 0.3.3 - 2026-06-22

- Surface wrong/cancelled/expected Trezor PIN failures as typed `TrezorError` variants (`InvalidPin`, `PinCancelled`, `PinRequired`) instead of generic device errors, so mobile clients can clear the PIN spinner, prompt a deliberate retry, and avoid reconnecting while the device is mid-flow. Backed by `trezor-connect-rs` 0.3.3, which maps protocol `Failure` codes to typed errors; unknown failure codes remain generic `TrezorError::DeviceError`.

## 0.3.2 - 2026-06-22

- Expose Trezor lock state through `TrezorFeatures.unlocked` so mobile apps can distinguish PIN protection from the current locked/unlocked session state.
- Add `trezor_refresh_features()` as an explicit one-shot refresh for fresh Trezor feature state without background polling.
- Surface busy Trezor transport state as `TrezorError::DeviceBusy`, including structured native callback busy results, so mobile clients can back off while the device is busy or awaiting user action.
