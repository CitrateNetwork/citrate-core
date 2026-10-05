---
created: 2026-10-02T00:00:00-07:00
branch: fix/windows-memory-cluster-ipc-timeouts
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-IPC-TIMEOUTS
---

# Windows Memory and Cluster IPC timeout compatibility

## Authority and tracking

- Issue: `CitrateNetwork/citrate-core#182`.
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`.
- Operating direction: tactical Windows bug discovery and bounded fixes under normal
  repository governance.
- This sprint does not authorize merging, release publication, signing, deployment, or
  changes to the open Comms fix in `citrate-core#180`.

## Reproduction at `main` (`b8d26e84cb12954d027b35bfadf5f63659e784ef`)

On Windows, `interprocess` uses named pipes and rejects socket-style I/O timeouts. The
Memory and Cluster clients propagate that unsupported-operation error before sending a
request, so otherwise healthy local IPC fails.

```text
memory::tests::unix_socket_transport_round_trips_a_real_socket
Transport("named pipes do not support I/O timeouts")

cluster::tests::cluster_ipc_authenticates_and_parses_status_defaulting_sharedfiles
Ipc("named pipes do not support I/O timeouts")
```

## Scope

1. Preserve mandatory receive/send timeout configuration on Unix.
2. Skip unsupported socket timeout configuration on Windows named pipes.
3. Keep request framing, authentication, endpoint naming, and daemon behavior unchanged.
4. Re-run both real local-socket integration tests on Windows.
5. Run formatting, Clippy, and the repository's applicable Rust test suite before making
   any completion claim.

## Acceptance criteria

- Memory's real local-socket round trip passes on Windows.
- Cluster's authenticated real local-socket round trip passes on Windows.
- Unix continues to require successful timeout configuration.
- No mock or production fallback is introduced.
- No unrelated behavior or dependency changes are included.

## Residual risk

Windows named pipes remain without a socket-level read/write timeout because the current
sync `interprocess` transport does not provide one. A fully bounded Windows operation
would require a separate, reviewed cancellation design. This fix removes the immediate
false failure without claiming that broader liveness work is complete.
