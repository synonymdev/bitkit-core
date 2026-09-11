# Pubky swaps through the Boltz interface

New submarine and reverse swaps use [`pubky-swap-boltz`](https://github.com/coreyphillips/pubky-swap-boltz) as an embedded Rust library. Existing `boltz*` FFI names and result types remain available to Android. The library talks to a configured Pubky provider through encrypted messages and authenticated rendezvous. It does not open a localhost HTTP server.

The pinned `boltz-client` 0.4.1 dependency remains responsible for wallet key derivation, Taproot script validation and unilateral signing. New swaps do not use Boltz's REST service, WebSocket service, cooperative signatures or recovery service. Records created by older builds keep their original Boltz route for status, claims and refunds.

## Configuration

Call `initDb` as before, then configure the bridge before requesting limits or creating a swap:

```kotlin
boltzConfigurePubky(
    config = PubkySwapConfig(
        network = BoltzNetwork.MAINNET,
        provider = "q9x5sfjbpajdebk45b9jashgb86iem7rnwpmu16px3ens63xzwro",
        electrumUrl = "ssl://bitkit.to:9999",
        dataDir = "<private wallet directory>/pubky-swaps/<identity>",
        maxFeeBps = 1000u,
        maxAmountSat = 1000000u,
    ),
    secretKeyHex = registeredPubkySecret,
)
```

The example directory placeholder must be replaced with an absolute application-private path. The secret is the existing registered Pubky Ed25519 secret from the platform keychain, not a Bitcoin private key or the wallet mnemonic. The plain provider key and its `pubky`-prefixed form identify the same provider. A Ring session without a locally managed secret cannot sign encrypted swap messages.

Android calls `PubkySwapInit.nativeInit(applicationContext)` before configuration. This installs the device DNS and certificate-verifier context. The local build script bundles the certificate verifier's Java helper and consumer keep rules in the AAR.

The provider must run the upstream Taproot and status support, advertise `boltz-taproot-v1` and `swap-status-v1`, and accept authenticated iroh rendezvous. Configuration does not deploy or upgrade the provider. A mainnet provider cannot be used on testnet or regtest.

## Existing application flow

The app still creates the submarine Lightning invoice and funds the returned lockup address. For a reverse swap it supplies a destination address and pays the returned hold invoice. Bitkit keeps the claim/refund private keys and reverse preimage. Only public keys and the payment hash enter the bridge.

```text
boltzGetSubmarineLimits(network)
boltzGetReverseLimits(network)
boltzCreateSubmarineSwap(network, electrumUrl, invoice, mnemonic, bip39Passphrase)
boltzCreateReverseSwap(network, electrumUrl, amountSat, claimAddress, mnemonic, bip39Passphrase)
boltzListSwaps()
boltzListPendingSwaps()
boltzGetSwap(swapId)
boltzClaimReverseSwap(swapId, mnemonic, bip39Passphrase, feeRateSatPerVb)
boltzRefundSubmarineSwap(swapId, refundAddress, mnemonic, bip39Passphrase, feeRateSatPerVb)
boltzStartSwapUpdates(network, listener, mnemonic, bip39Passphrase, feeRateSatPerVb, acceptZeroConf)
boltzStopSwapUpdates()
boltzDisconnectPubky()
```

Pubky swaps are polled and produce the existing typed lifecycle events. Legacy records retain their Boltz WebSocket and REST reconciliation. `boltzDisconnectPubky` releases the messaging identity without stopping legacy recovery. Stop updates separately when the wallet stops or is wiped. Reconfigure after changing identity, provider, network or Electrum settings. Each identity needs its own state directory; recovering an older Pubky swap requires its original identity and provider.

Pubky reverse claims require a confirmed, independently observed unspent lockup and at least 18 blocks before refund. `acceptZeroConf` does not override this requirement. Pubky claims use the Taproot script path without transmitting the preimage to a cooperative signing endpoint. Submarine refunds use the timeout script path. The application supplies transaction fee estimates.

## Persistence and recovery

`boltz.db` preserves the existing BIP85 derivation index and adds the provider binding. Existing rows migrate as legacy Boltz records. Creation intents store the reserved index, exact public request, destination and idempotency identifier before negotiation. Restart recovery repeats the original request and checks the original wallet keys before completing the record.

The bridge separately persists quotes, accepted contracts and protocol identifiers in its private SQLite directory. Preserve both databases and the wallet seed/passphrase. A transaction-id journal lets recovery recognize a broadcast that occurred before the local completion record was saved. It contains no private key, reverse preimage or transaction witness.

A Pubky homeserver outage must not prevent spending an already accepted on-chain contract. Messaging signs in lazily; chain inspection and unilateral recovery use the persisted contract. Electrum access is still required. An interrupted quote that the provider never accepted may expire and require a new invoice or payment hash.

The wallet mnemonic and BIP39 passphrase must match the original wallet. Pubky swaps are not recoverable through Boltz's rescue website. Seed-only discovery of lost Pubky contract metadata is not implemented. Sign-out preserves recovery state. Coordinated swap-state cleanup during wallet wipe is not implemented. The Android wallet-wipe flow retains both `boltz.db` and the wrapper identity directories so their recovery records remain together. Preserve a successful logical recovery snapshot before wiping a wallet.

## Local recovery snapshots

`boltzExportBackup(pubkyDataRoot)` returns a versioned JSON snapshot of all wallet swap records, pending creation intents, the reserved BIP32 key counter, every pending spend transaction ID, and each identity's wrapper contracts and retry bindings. Supply the identity parent directory, such as `<private wallet directory>/pubky-swaps`. The API reads logical records from SQLite, including disconnected prior identities, and never copies a live database file. It exports no private keys, reverse preimages or transaction witnesses. The metadata still contains invoices and swap history, so keep the returned value in encrypted storage together with the wallet's existing recovery material.

`boltzRestoreBackup(snapshotJson, pubkyDataRoot)` validates the complete snapshot before merging it. Stop swap updates and disconnect Pubky first. Restore rejects an active swap operation promptly. It preserves newer local progress, retains the highest reserved key counter and refuses conflicting contracts or retry bindings. The merged key counter is reserved first, then wrapper contracts are restored before the Core records that reference them. If a filesystem write fails partway through, the API returns an error and the same snapshot can be retried safely. A durable incomplete-restore marker blocks export and new key reservations until the same snapshot is successfully retried against the same directory, including after restart. This prevents replacing a good backup with partially imported data. Reserved indexes remain consumed after failure so a later new swap cannot reuse a key from a partially restored contract.

Export briefly prevents new swap mutations while capturing Core records and the corresponding wrapper snapshots. It returns an error promptly if a swap operation is already active, without waiting for an offline provider request. Retry after swap creation or spending completes. Export fails if an identity database is corrupt, locked by another process, missing required recovery records or stored behind a symbolic link. Callers must keep their last successful backup when export fails. Limits are 16 MiB of JSON, 100 identities, 10,000 Core records/intents and 10,000 wrapper records across identities. Directory names must be canonical identity keys; snapshot input cannot select arbitrary filesystem paths.

These functions operate locally and do not select or upload to a backup destination. Automatic integration with the Android VSS backup is not included. The wallet seed and BIP39 passphrase remain necessary to reconstruct signing keys. Restoring prior Pubky identities also requires their original registered secrets from the platform's identity recovery flow.

## Local Android build

Use Rust 1.95 or later, the installed Android NDK, JDK 17 or 21, cargo-ndk and the repository's Gobley binding generator:

```sh
export ANDROID_NDK_HOME="<Android SDK>/ndk/<version>"
export JAVA_HOME="<JDK directory>"
./build_local_android.sh
```

The script builds all four Android ABIs, regenerates Kotlin bindings, archives native symbols, strips packaged libraries and publishes `com.synonym:bitkit-core-android:0.5.14-pubky-swap-boltz-local` to Maven Local. The Android integration branch resolves that exact local artifact. It does not publish a public release or change the source version.

The local artifact uses the Android TLS helper already bundled by Bitkit's pinned Paykit dependency, avoiding duplicate classes. Standard Core builds bundle the helper by default. For a local consumer without Paykit, set `LOCAL_BUNDLE_PUBKY_TLS_HELPER=true` when running the script. The helper must match the `rustls-platform-verifier-android` version in `Cargo.lock`.

Validate the core with `cargo test modules::boltz --lib` and `cargo clippy --lib`. The integration fixtures exercise the real bridge with the pinned Rust SDK, including response validation, unilateral signatures, restart intents, backend migration and recovery journals. Live mainnet swaps require separate operational validation against the upgraded provider.
