---
created: 2026-10-04T00:00:00-07:00
branch: test/windows-memory-native-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-MEMORY-NATIVE-FIXTURE
---

# Windows native Memory test fixture

## Authority and tracking

- Issue: `CitrateNetwork/citrate-core#215`.
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`.
- Operating direction: tactical Windows bug discovery and bounded fixes under normal
  repository governance.

## Reproduction at `main` (`fc1d0b916524bfa5ea9f8058d6157184c64c9860`)

`cargo test --lib memory::tests:: -- --nocapture` reports four failures because the
supervisor directly executes the POSIX-only `stub_mem_mcp.sh` fixture on Windows:

```text
spawn failed: %1 is not a valid Win32 application. (os error 193)
```

## Scope

1. Replace the Bash/Python fixture with a dependency-free native Rust test fixture.
2. Compile the fixture once per unit-test process into Cargo's `OUT_DIR`.
3. Preserve the sidecar's positional arguments and `CITRATE_MEM_STORE_KEY` env contract.
4. Preserve encrypted fixture-file creation, stale-endpoint cleanup, and long-running
   supervisor lifecycle behavior.
5. Keep production Memory and supervisor code unchanged.

## Acceptance criteria

- All non-ignored `memory::tests` pass on Windows.
- The ciphertext tripwire still proves the plaintext sentinel is absent from `data.enc`.
- The fixture fails closed when its key is missing or invalid.
- Formatting and Clippy pass without warnings.
- No failing test is deleted, disabled, or weakened.
