# Changelog

## Unreleased

- Surface a locked Trezor during the THP handshake as the typed `TrezorError::DeviceBusy` instead of a generic connection error, so mobile clients back off and prompt the user to unlock rather than reconnecting in a loop. Backed by `trezor-connect-rs` 0.3.4, which classifies `DeviceLocked` as a distinct, non-retryable state: it no longer churns the transport (close/reopen loop) on a locked device and instead makes a single `try_to_unlock` handshake attempt so the device prompts for unlock.

## 0.3.3 - 2026-06-22

- Surface wrong/cancelled/expected Trezor PIN failures as typed `TrezorError` variants (`InvalidPin`, `PinCancelled`, `PinRequired`) instead of generic device errors, so mobile clients can clear the PIN spinner, prompt a deliberate retry, and avoid reconnecting while the device is mid-flow. Backed by `trezor-connect-rs` 0.3.3, which maps protocol `Failure` codes to typed errors; unknown failure codes remain generic `TrezorError::DeviceError`.

## 0.3.2 - 2026-06-22

- Expose Trezor lock state through `TrezorFeatures.unlocked` so mobile apps can distinguish PIN protection from the current locked/unlocked session state.
- Add `trezor_refresh_features()` as an explicit one-shot refresh for fresh Trezor feature state without background polling.
- Surface busy Trezor transport state as `TrezorError::DeviceBusy`, including structured native callback busy results, so mobile clients can back off while the device is busy or awaiting user action.
