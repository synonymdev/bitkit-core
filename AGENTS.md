# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust FFI library (`bitkitcore`) providing Bitcoin & Lightning functionality with UniFFI-generated bindings for iOS (Swift), Android (Kotlin), and Python.

## Build

```bash
cargo build                                    # Rust library
./build.sh <ios|android|python|all>            # Platform bindings
./build.sh -r --patch <target>                 # Bump version + build (--minor, --major)
```

## Test

```bash
cargo test                                     # All tests
cargo test modules::<module>                   # Single module (scanner, lnurl, onchain, activity, blocktank, trezor, pubky)
```

## Lint & Format

```bash
cargo clippy                                   # Lint
cargo fmt                                      # Format Rust
```

Android bindings use ktlint via Gradle plugin (`org.jlleitschuh.gradle.ktlint`), excluding generated code.

## Architecture

- `src/lib.rs` — UniFFI exports and module re-exports
- `src/modules/` — Core modules: scanner, lnurl, onchain, activity, blocktank, trezor, pubky
- `bindings/` — Platform-specific binding outputs (ios/, android/, python/)
- `build.sh`, `build_ios.sh`, `build_android.sh`, `build_python.sh` — Build scripts

## Key Constraints

- **Version sync**: Version must match across `Cargo.toml`, `Package.swift`, and `bindings/android/gradle.properties`. Use `build.sh -r` to bump all three.
- **UniFFI**: Public types exposed to bindings are declared in `src/lib.rs`. Follow existing UniFFI patterns when adding new types.
- **Platform-specific deps**: Trezor uses Bluetooth-only on iOS, USB+Bluetooth on other platforms (see `Cargo.toml` target-specific dependencies).
- **Android build**: `build_android.sh` temporarily modifies `Cargo.toml` crate-type and removes `example/main.rs` during build — don't run concurrent builds.

## Conventions

- Branch naming: `feat/*`, `fix/*`, `chore/*`
