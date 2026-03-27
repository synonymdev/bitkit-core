---
name: verify
description: Run clippy and tests to verify the codebase compiles cleanly and all tests pass. Use after making changes or before committing.
---

Run the following checks in sequence, stopping on first failure:

1. `cargo clippy -- -D warnings` — ensure no lint warnings
2. `cargo test` — run all tests

Report results concisely. If clippy or tests fail, show the relevant errors and suggest fixes.
