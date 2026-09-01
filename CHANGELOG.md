# Changelog

## Unreleased

## 0.5.13 - 2026-09-01

- Prevent Android ARM32 startup crashes by generating `Int` carriers for direct unsigned 8-bit and 16-bit UniFFI returns while preserving Kotlin unsigned APIs.

## 0.5.12 - 2026-08-28

- Add `get_activities_tags(wallet_id: Option<String>)` and `get_pre_activity_metadata_list(wallet_id: Option<String>)`, wallet-scoped reads of the two tag-backup tables where `None` returns every scope. Apps backing up a single wallet scope (for example a `trezor:{hash}` hardware wallet) can now name that scope in the call instead of fetching every scope and filtering client-side, so which records leave the device is visible in the FFI call. The unscoped `get_all_activities_tags()` and `get_all_pre_activity_metadata()` are unchanged and now delegate to the scoped versions.
- Drop the legacy `idx_onchain_id` and `idx_lightning_id` unique indexes on activity init. Older releases created them when an activity id was globally unique, which contradicts the `PRIMARY KEY (wallet_id, id)` the activity tables now use and blocked storing the same on-chain id once per wallet scope, for example a transaction visible to both the Bitkit wallet and a watched hardware wallet. `CREATE ... IF NOT EXISTS` never removed them from databases that already had them, so they are dropped unconditionally.

## 0.5.11 - 2026-08-27

- Add keep consumer rules for JNA types UniFFI needs under R8.

## 0.5.10 - 2026-08-27

- Watch-only watcher events now include the synchronized next unused external address and full derivation path.

## 0.5.9 - 2026-08-26

- Ship the initial Android R8 consumer keep rules for the UniFFI/JNA FFI surface.

## 0.5.8 - 2026-08-26

- Preserve UR decoder scan progress across invalid frames.
- Reject signed PSBT previous-output metadata that differs from the original transaction.
- Enforce encoded UR frame-size bounds.

## 0.5.7 - 2026-08-24

- Add a generic hardware-wallet catalog with Foundation Passport support, multipart UR QR encoding and decoding, Passport single-signature account export parsing, and signed PSBT finalization across the UniFFI bindings.

## 0.5.6 - 2026-08-14

- Stop writing secrets and user data to stdout. Library logs go through the `log` facade.

## 0.5.5 - 2026-08-03

- Add `serialized_extended_pubkey` for canonical 78-byte BIP32 xpub/tpub payloads, exposed as Swift `Data` and Kotlin `ByteArray`.

## 0.5.4 - 2026-07-30

- Persist the submarine-swap refund destination so `boltzGetSwap` and `boltzListSwaps` return it after completion.

## 0.5.3 - 2026-07-28

- Validate Boltz create responses, claim and refund addresses, and fee rates before swapping.
- Keep a settled reverse swap pending until its claim txid is recorded locally, and retry those claims on the updates stream.

## 0.5.2 - 2026-07-20

- Swap status updates now reconcile against Boltz's REST status whenever a swap is (re)subscribed, both on `boltz_start_swap_updates` and when `boltz_create_reverse_swap` adds a swap to a running stream. A confirmed reverse-swap lockup is therefore caught up and auto-claimed even when its live WebSocket event was missed (for example because the updates stream was down while the lockup confirmed), instead of the swap silently stalling until a manual claim. No FFI signature change.
- Add instant-claim and periodic reconcile for Boltz reverse swaps.

## 0.5.1 - 2026-07-15

- Upgrade `trezor-connect-rs` to 0.4.0.

## 0.5.0 - 2026-07-15

- Add a Boltz module for submarine (onchain → Lightning) and reverse (Lightning → onchain) swaps behind the UniFFI surface.

## 0.4.4 - 2026-07-28

- Add stable ELF build IDs to published Android libraries.

## 0.4.3 - 2026-07-23

- Publish the Android library with NDK r28c and 16 KB page-size validation on the AAR publication path.

## 0.4.2 - 2026-07-16

- Add `serialized_extended_pubkey` for converting Base58Check BIP32 xpub/tpub values into their canonical 78-byte payload, exposed as Swift `Data`, Kotlin `ByteArray`, and Python `bytes`.

## 0.4.1 - 2026-07-10

- `onchain_broadcast_raw_tx` now returns the transaction's canonical txid, computed locally in Rust, and treats Electrum "already known / already in mempool / already in block chain" responses as success (returning that same txid). This lets native apps complete Blocktank funding bookkeeping when they retry a broadcast after an ambiguous network failure, without relying on a signer-provided txid. Genuine connectivity failures and unrelated broadcast rejections remain typed `BroadcastError`s, and there is no FFI signature change.

## 0.4.0 - 2026-07-08

- Add Core-owned backup migration for wallet-scoped activity data, covering the backup-JSON boundary so apps can inject a default wallet id on legacy records.

## 0.3.9 - 2026-07-01

- Surface a locked Trezor during the THP handshake as the typed `TrezorError::DeviceBusy` instead of a generic connection error, so mobile clients back off and prompt the user to unlock rather than reconnecting in a loop. Backed by `trezor-connect-rs` 0.3.4, which classifies `DeviceLocked` as a distinct, non-retryable state: it no longer churns the transport (close/reopen loop) on a locked device and instead makes a single `try_to_unlock` handshake attempt so the device prompts for unlock.

## 0.3.8 - 2026-06-30

- Validate LNURL-pay amount.

## 0.3.7 - 2026-06-30

- Serialized activity records always include `walletId`.

## 0.3.6 - 2026-06-29

- Add `get_supported_hardware_wallets()` so apps render the supported-device catalog from core.
- Improve LNURL-pay payment validation.

## 0.3.4 - 2026-06-24

- Add `derive_wallet_id(device_type, xpubs)` so platforms derive the same hardware watch-only wallet id.
- Expose `get_default_gap_limit()` as the shared BIP44 gap-limit default.
- Watcher emits persistence-ready activities the app can store through the normal activity APIs.

## 0.3.3 - 2026-06-22

- Surface wrong/cancelled/expected Trezor PIN failures as typed `TrezorError` variants (`InvalidPin`, `PinCancelled`, `PinRequired`) instead of generic device errors, so mobile clients can clear the PIN spinner, prompt a deliberate retry, and avoid reconnecting while the device is mid-flow. Backed by `trezor-connect-rs` 0.3.3, which maps protocol `Failure` codes to typed errors; unknown failure codes remain generic `TrezorError::DeviceError`.

## 0.3.2 - 2026-06-22

- Expose Trezor lock state through `TrezorFeatures.unlocked` so mobile apps can distinguish PIN protection from the current locked/unlocked session state.
- Add `trezor_refresh_features()` as an explicit one-shot refresh for fresh Trezor feature state without background polling.
- Surface busy Trezor transport state as `TrezorError::DeviceBusy`, including structured native callback busy results, so mobile clients can back off while the device is busy or awaiting user action.
