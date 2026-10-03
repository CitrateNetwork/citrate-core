---
created: 2026-10-03T00:00:00-07:00
branch: test/windows-ipc-name-portability
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-IPC-NAME-TESTS
---

# Verification evidence

## Pinned state

- Base branch: `main`
- Base SHA: `b8d26e84cb12954d027b35bfadf5f63659e784ef`
- Host: Windows, `x86_64-pc-windows-msvc`
- Issue: `CitrateNetwork/citrate-core#184`

## Red evidence

```text
cargo test --lib ipc_name::tests:: -- --nocapture
3 passed; 1 failed

ipc_name::tests::unix_name_is_the_path_verbatim
fs name builds on unix: Os { code: 3, kind: NotFound, message: "The system cannot find the path specified." }
```

The run also emitted an unused-variable warning and left these untracked files:

```text
src-tauri/.pipe-nonce
src-tauri/relative-name.sock.pipe-nonce
```

## Green evidence

```text
cargo test --lib ipc_name::tests:: -- --nocapture
4 passed; 0 failed

cargo fmt --all -- --check
PASS

cargo clippy --lib --tests -- -D warnings
PASS
```

The targeted test and subsequent full-suite run left no nonce artifacts in the checkout.
Rust language-server diagnostics are clean for `ipc_name.rs`.

## Full-suite observation

`cargo test --lib` completed with `519 passed`, `12 failed`, and `5 ignored`. Every
`ipc_name` test passed. All 12 failures are outside this sprint's changed file; they
include the Memory and Cluster named-pipe timeout failures addressed by PR `#183`, the
Comms timeout failures addressed by PR `#180`, and other existing Windows fixture/process
failures. No failing test was deleted, disabled, or modified.

## Current claim

The IPC-name test module is locally Windows-clean: its four tests pass without warning or
working-tree artifacts. Production IPC behavior is unchanged. The work is not yet
committed, reviewed, merged, released, or packaged-app tested.
