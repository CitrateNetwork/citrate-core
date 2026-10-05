---
created: 2026-10-04T00:00:00-07:00
branch: test/windows-node-native-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-NODE-NATIVE-FIXTURE
---

# Verification evidence

## Pinned state

- Base branch: `main`
- Base SHA: `fc1d0b916524bfa5ea9f8058d6157184c64c9860`
- Host: Windows, `x86_64-pc-windows-msvc`
- Issue: `CitrateNetwork/citrate-core#188`

## Red evidence

```text
node::tests::start_reaches_running_then_stop_is_clean
node::tests::data_dir_is_ciphertext_at_rest

spawn failed: %1 is not a valid Win32 application. (os error 193)
```

The supervisor was attempting to execute `tests/fixtures/stub_node.sh` directly.

## Green evidence

```text
cargo test --lib node::tests::start_reaches_running_then_stop_is_clean -- --exact --nocapture
1 passed; 0 failed

cargo test --lib node::tests::data_dir_is_ciphertext_at_rest -- --exact --nocapture
1 passed; 0 failed

cargo test --lib node::tests:: -- --nocapture
25 passed; 0 failed; 1 ignored

cargo fmt --all -- --check
PASS

cargo clippy --lib --tests -- -D warnings
PASS
```

Rust language-server diagnostics are clean for the changed Rust files.

## Full-suite observation

`cargo test --lib` completed with `524 passed`, `7 failed`, and `5 ignored`. Every
node test passed. The remaining failures are outside this sprint's changed files:

- Two Comms named-pipe timeout failures tracked by PR `#180`.
- The node-agent Unix-only fixture failure tracked by issue `#189`.
- Four Memory tests whose Unix-only fixture cannot provide the expected files or
  long-lived process on Windows; these require separate tracking and a bounded fix.

No failing test was deleted, disabled, or modified.

## Current claim

The node fixture is locally Windows-clean: the lifecycle and ciphertext tests pass, as
does the complete node test module. Production process spawning is unchanged. The work
is not yet committed, reviewed, merged, released, or packaged-app tested.
