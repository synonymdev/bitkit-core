# GitHub Copilot Code Review Instructions

When performing a code review, respond in English.

## Architecture & Patterns

When performing a code review, ensure new public types are properly exported via `src/lib.rs` for UniFFI binding generation.

When performing a code review, verify that modules follow the established structure: `mod.rs`, `types.rs`, `errors.rs`, `implementation.rs`, and optional `tests.rs`.

When performing a code review, check that UniFFI-exported types follow existing patterns (derive macros, enum representations, error types).

## Error Handling & Safety

When performing a code review, flag any use of `unwrap()` or `expect()` in non-test code and suggest proper error propagation with `?` or `Result`.

When performing a code review, ensure error types implement proper `Display` and `Error` traits and are exported for UniFFI.

When performing a code review, flag any `unsafe` blocks and verify they are necessary and well-documented.

## Code Quality & Readability

When performing a code review, ensure `cargo clippy` warnings are addressed — the project treats clippy warnings as errors.

When performing a code review, verify that `cargo fmt` formatting is applied consistently.

When performing a code review, focus on readability and avoid deeply nested match arms, replacing with early returns or helper functions where possible.

When performing a code review, ensure unused code is removed after refactoring.

When performing a code review, verify that existing utilities and helper functions are reused rather than creating duplicates.

## Dependencies & Platform

When performing a code review, verify that platform-specific dependencies use correct `#[cfg(target_os)]` guards (especially Trezor: BLE-only on iOS, USB+BLE elsewhere).

When performing a code review, check that new dependencies are justified and don't introduce unnecessary bloat to the FFI binary.

## Testing

When performing a code review, suggest tests for new functionality covering the most important cases.

When performing a code review, verify that tests use the established patterns (test modules in `tests.rs`, `#[cfg(test)]` gating).

## Bitcoin & Lightning Specific

When performing a code review, verify that Bitcoin/Lightning operations use proper types from the `bitcoin` and `bdk` crates.

When performing a code review, verify that proper Bitcoin and Lightning technical terms are used when naming code components.

## Build & Version

When performing a code review, check that version changes are synchronized across `Cargo.toml`, `Package.swift`, and `bindings/android/gradle.properties`.

When performing a code review, verify that changes to public API types don't break existing UniFFI bindings without updating the binding generation.
