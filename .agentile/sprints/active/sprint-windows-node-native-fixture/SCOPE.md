---
created: 2026-10-04T00:00:00-07:00
branch: test/windows-node-native-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-NODE-NATIVE-FIXTURE
---

# Windows node supervisor native test fixture

## Authority and tracking

- Issue: `CitrateNetwork/citrate-core#188`.
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`.
- Operating direction: tactical Windows bug discovery and bounded fixes under normal
  repository governance; the prior three-PR queue limit has been lifted.

## Reproduction at `main` (`fc1d0b916524bfa5ea9f8058d6157184c64c9860`)

The node supervision tests pass `tests/fixtures/stub_node.sh` directly to
`std::process::Command`. Windows cannot execute that Unix shell script and records
`spawn failed: %1 is not a valid Win32 application. (os error 193)`.

## Scope

1. Replace the Unix-only shell fixture with a checked-in Rust fixture source.
2. Compile the fixture into Cargo's build output for `cargo test --lib` and spawn the
   resulting native executable directly on every platform.
3. Preserve the fixture's storage-key, ciphertext-at-rest, process-lifetime, and
   supervisor-shutdown behavior.
4. Leave production process spawning and sidecar resolution unchanged.

## Acceptance criteria

- The node lifecycle and ciphertext-at-rest tests pass on Windows.
- The complete node test module passes.
- Formatting and Clippy pass with warnings denied.
- The checkout remains clean except for the intentional branch changes.
- Production code is unchanged.
