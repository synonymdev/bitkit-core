# Trezor binding migration checks

Regenerate the bindings with the repository scripts before running these checks.
Run platform builds sequentially because the Android script temporarily changes
the source/build configuration.

Python checks record defaults, explicit overrides, and the new response fields:

```sh
./build.sh python
PYTHONPATH=bindings/python python3 tests/bindings/trezor_records.py
```

Swift compiles and runs the same constructor and response checks against the
host library used for binding generation:

```sh
./build.sh ios
swiftc -I bindings/ios -L target/release -lbitkitcore \
  bindings/ios/bitkitcore.swift tests/bindings/trezor_records.swift \
  -o /tmp/trezor-records-smoke
DYLD_LIBRARY_PATH="$PWD/target/release" /tmp/trezor-records-smoke
```

Kotlin's record smoke source is included in the Android unit-test source set.
Compiling it checks the generated constructor defaults and response field types:

```sh
./build.sh android
cd bindings/android
./gradlew :lib:compileDebugUnitTestKotlin
```

The Kotlin smoke function is a compilation fixture, not an automated runtime
test. These checks do not connect to a device. Hardware confirmation, transport,
and native wallet reattachment checks remain part of release validation.
