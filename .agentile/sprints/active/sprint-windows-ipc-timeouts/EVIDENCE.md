---
created: 2026-10-02T00:00:00-07:00
branch: fix/windows-memory-cluster-ipc-timeouts
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-IPC-TIMEOUTS
---

# Verification evidence

## Pinned state

- Base branch: `main`
- Base SHA: `b8d26e84cb12954d027b35bfadf5f63659e784ef`
- Host: Windows, `x86_64-pc-windows-msvc`
- Issue: `CitrateNetwork/citrate-core#182`
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`

## Red evidence

Both existing real local-socket integration tests failed on the base SHA before the
request was sent:

```text
cargo test --lib unix_socket_transport_round_trips_a_real_socket -- --nocapture
Transport("named pipes do not support I/O timeouts")

cargo test --lib cluster_ipc_authenticates_and_parses_status_defaulting_sharedfiles -- --nocapture
Ipc("named pipes do not support I/O timeouts")
```

## Green evidence

After preserving required timeout setup on Unix and skipping the unsupported operation on
Windows:

```text
cargo test --lib unix_socket_transport_round_trips_a_real_socket -- --nocapture
1 passed; 0 failed

cargo test --lib cluster_ipc_authenticates_and_parses_status_defaulting_sharedfiles -- --nocapture
1 passed; 0 failed

cargo fmt --all -- --check
PASS

cargo clippy --lib -- -D warnings
PASS
```

Rust language-server diagnostics are clean for `memory.rs` and `cluster.rs`.

## Full-suite observation

`cargo test --lib` completed with `521 passed`, `10 failed`, and `5 ignored`. The two
Comms failures reproduce the separate named-pipe timeout defect already addressed by open
PR `citrate-core#180`. The remaining eight failures are pre-existing Windows test/path or
stub-process failures outside this sprint. No failing test was deleted, disabled, or
changed.

## Current claim

The Memory and Cluster regression tests are fixed locally and the changed production code
passes formatting, Clippy, and language-server checks. The work is not committed,
published, reviewed, merged, released, or runtime-proven in a packaged application.
