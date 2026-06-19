# Changelog

## 0.3.1 - 2026-06-19

- Expose Trezor lock state through `TrezorFeatures.unlocked` so mobile apps can distinguish PIN protection from the current locked/unlocked session state.
- Add `trezor_refresh_features()` as an explicit one-shot refresh for fresh Trezor feature state without background polling.
- Surface busy Trezor transport state as `TrezorError::DeviceBusy` so mobile clients can back off while the device is busy or awaiting user action.
