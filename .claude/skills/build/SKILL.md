---
name: build
description: Build platform bindings using build.sh. Pass a target as argument (ios, android, python, all). Use when you need to generate or test platform-specific bindings.
disable-model-invocation: true
---

Run `./build.sh $ARGUMENTS` from the project root.

If no arguments provided, ask the user which target to build (ios, android, python, all).

For release builds, remind the user to use `-r` with a version bump flag (`--patch`, `--minor`, or `--major`).
