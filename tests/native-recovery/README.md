# Native USDT recovery protocol fixture

This loopback fixture exercises real native Core signing and the apps' real encrypted VSS clients without funds. It implements the RPC responses needed for one public test account, VSS protobuf put/get/list and a test LNURL token exchange. It validates operation and EIP-7702 signatures, records every submission attempt before validation, and persists an append-only event ledger and immutable prepare-phase oracle.

It does **not** execute deployed contracts, validate LNURL signatures, emulate a complete VSS server or prove a full app UI/Paykit journey. The public mnemonic is `test test test test test test test test test test test junk`. Never fund it. Use only a dedicated simulator/emulator installation without an application wallet; the native tests create separate SQLite files and do not reset a wallet.

## Start

From this directory, with Node 22 or newer:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm run check
npm test
FIXTURE_PORT=18557 node provider.mjs /tmp/bitkit-native-recovery-evidence
```

Keep the server running across native test processes. It binds only `127.0.0.1`. Its evidence directory contains encrypted VSS records plus **public fixture** signed operations and snapshots; it must never receive a real wallet. The test token expires after one day; use a fresh evidence directory for a later run.

## Native tests

The companion tests are `UsdtNativeRecoveryIntegrationTests.swift` in [iOS #1118](https://github.com/synonymdev/bitkit-ios/pull/1118) and `UsdtNativeRecoveryIntegrationTest.kt` in [Android #1681](https://github.com/synonymdev/bitkit-android/pull/1681). They skip when no configured fixture is available. Use the companion branches including those files. Build against Core containing [#179](https://github.com/synonymdev/bitkit-core/pull/179); the recovery apps must contain the independent recovery store implementation.

Select a fresh scenario and run **one native test process per phase**:

```sh
node control.mjs ios ios-loss-01 prepare commit-drop
# Run the selected iOS test, then:
node control.mjs ios ios-loss-01 resume
# Run it in a new test process, then:
node control.mjs ios ios-loss-01 restore
# Run it again in a new test process.
```

Repeat with a fresh name and `reject` instead of `commit-drop`. Prepare signs once, confirms three failed upload attempts and preserves the original operation independently of VSS. Resume reopens the same database. Restore uses a fresh database and reads encrypted recovery records. Both latter phases first retry while the fault remains active, then require a new recovery acknowledgement before submitting the original signed operation. Reject must leave encrypted records unchanged; commit/drop must leave decryptable records despite losing the response. Existing complete `WALLET` sentinel bytes must remain unchanged. This is byte-preservation evidence, not a complete-wallet migration journey.

### iOS

From the companion iOS checkout, build local Core bindings first, then use its existing local-build script with an available **dedicated** simulator:

```sh
scripts/build-usdt-local.sh /absolute/path/to/bitkit-core test \
  -configuration Debug -destination 'platform=iOS Simulator,id=YOUR_SIMULATOR_ID' \
  -derivedDataPath /tmp/bitkit-native-recovery-ios \
  -only-testing:BitkitTests/UsdtNativeRecoveryIntegrationTests \
  -parallel-testing-enabled NO ONLY_ACTIVE_ARCH=YES
```

Subsequent phases can use `test-without-building` with the same arguments. The test calls the actual `BackupService` recovery handler and checks that no unrelated backup category is exported.

### Android

From the companion Android checkout, build and install the app and instrumentation APK on a dedicated emulator. Set `ANDROID_SERIAL` explicitly; do not clear app data or stop another worker's device.

```sh
./gradlew :app:assembleDevDebug :app:assembleDevDebugAndroidTest \
  -PbitkitCoreAar=/absolute/path/to/bitkit-core/bindings/android/lib/build/outputs/aar/lib-release.aar
adb -s "$ANDROID_SERIAL" install -r app/build/outputs/apk/dev/debug/app-dev-arm64-v8a-debug.apk
adb -s "$ANDROID_SERIAL" install -r app/build/outputs/apk/androidTest/dev/debug/app-dev-debug-androidTest.apk
adb -s "$ANDROID_SERIAL" reverse tcp:18557 tcp:18557
```

Use `node control.mjs android android-loss-01 prepare commit-drop` in the fixture directory, then run this command once per configured phase:

```sh
adb -s "$ANDROID_SERIAL" shell am instrument -w -r \
  -e class to.bitkit.services.UsdtNativeRecoveryIntegrationTest \
  to.bitkit.dev.test/to.bitkit.test.HiltTestRunner
```

Require `OK (1 test)` and status code `0`; the shell exit code alone does not prove success. Android calls `UsdtRecoveryBackupStore` directly, so this does not replace `BackupRepo` orchestration tests.

## Cross-platform restore and evidence

After preparing and resuming on one platform, configure `restore` for that same scenario on the other platform, where no restored database exists yet. Do not run two phases against the same scenario concurrently: each phase deliberately changes the fault mode. Finish within the signed paymaster data's ten-minute validity window.

Save the native test reports, artifact/source hashes and `<scenario>.json` from the server directory. Each phase must show its own matching fault, zero pre-acknowledgement submission attempts, then a recovery acknowledgement followed by an identical signed payload. Earlier phases' events cannot satisfy later assertions. Repeated submissions of the **same** operation hash are allowed; a second operation is not.

The provider rejects a second distinct valid operation and records `broadcast-rejected`, including after its own restart. A plaintext guard failure records `backup-invalid` and leaves every record and version in that batch unchanged. `npm test` exercises both refusal paths and subsequent valid retries; these protocol controls do not replace the native app phases above.

The `/pubky` endpoint returns 503 for future integration, but these tests do not wire Paykit to it and must not be described as a complete Pubky-outage journey. Contact authorization, homeserver lease/auth recovery, full-wallet migration, concurrent app state changes and deployed-contract settlement require their separate tests.
