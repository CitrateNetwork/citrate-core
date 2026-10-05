---
created: 2026-10-04T00:00:00-07:00
branch: test/windows-node-agent-native-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-NODE-AGENT-NATIVE-FIXTURE
---

# Windows node-agent native test fixture

## Authority and tracking

- Issue: `CitrateNetwork/citrate-core#189`.
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`.
- Operating direction: tactical Windows bug discovery and bounded fixes under normal
  repository governance; the prior three-PR queue limit has been lifted.

## Reproduction at `main` (`fc1d0b916524bfa5ea9f8058d6157184c64c9860`)

The node-agent supervision test passes `tests/fixtures/stub_node_agent.sh` directly to
`std::process::Command`. Windows cannot execute that Unix shell script, retries the spawn
nine times, and records `spawn failed: %1 is not a valid Win32 application. (os error 193)`.

## Scope

1. Replace the Bash/Python fixture with a checked-in native Rust fixture source.
2. Compile the fixture into Cargo's build output for `cargo test --lib` and spawn the
   resulting native executable directly on every platform.
3. Preserve the bearer-file handoff, loopback binding, authenticated `/status` response,
   long-lived process behavior, and clean shutdown.
4. Leave production process spawning, bearer handling, and sidecar resolution unchanged.

## Acceptance criteria

- The node-agent start and bearer round-trip test passes on Windows.
- The complete agent test module passes.
- Formatting and Clippy pass with warnings denied.
- The checkout remains clean except for the intentional branch changes.
- Production code is unchanged.
