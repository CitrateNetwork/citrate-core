---
created: 2026-10-03T00:00:00-07:00
branch: test/windows-ipc-name-portability
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-IPC-NAME-TESTS
---

# Windows IPC endpoint-name test portability

## Authority and tracking

- Issue: `CitrateNetwork/citrate-core#184`.
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`.
- Operating direction: tactical Windows bug discovery and bounded fixes under normal
  repository governance.

## Reproduction at `main` (`b8d26e84cb12954d027b35bfadf5f63659e784ef`)

`cargo test --lib ipc_name::tests:: -- --nocapture` fails the Unix-specific name test on
Windows and leaves these untracked files in `src-tauri`:

```text
.pipe-nonce
relative-name.sock.pipe-nonce
```

## Scope

1. Keep the Unix filesystem-name assertion on Unix.
2. Exercise stateful Windows endpoint naming under an absolute disposable directory.
3. Exercise empty and relative Windows edge cases through the pure name derivation helper.
4. Preserve the production naming algorithm and existing test count.

## Acceptance criteria

- All four `ipc_name` tests pass on Windows.
- The test run leaves the checkout clean.
- Windows compilation has no Unix-only unused-variable warning.
- Production code is unchanged.
