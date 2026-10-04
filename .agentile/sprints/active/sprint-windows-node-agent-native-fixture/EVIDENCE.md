---
created: 2026-10-04T00:00:00-07:00
branch: test/windows-node-agent-native-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-NODE-AGENT-NATIVE-FIXTURE
---

# Verification evidence

## Pinned state

- Base branch: `main`
- Base SHA: `fc1d0b916524bfa5ea9f8058d6157184c64c9860`
- Host: Windows, `x86_64-pc-windows-msvc`
- Issue: `CitrateNetwork/citrate-core#189`

## Red evidence

```text
agent::tests::start_spawns_stub_and_bearer_round_trips_over_loopback

spawn failed: %1 is not a valid Win32 application. (os error 193)
```

The supervisor exhausted its bounded retry policy attempting to execute
`tests/fixtures/stub_node_agent.sh` directly.

## Green evidence

```text
cargo test --lib agent::tests::start_spawns_stub_and_bearer_round_trips_over_loopback -- --exact --nocapture
1 passed; 0 failed

cargo test --lib agent::tests:: -- --nocapture
28 passed; 0 failed; 1 ignored

cargo fmt --all -- --check
PASS

cargo clippy --lib --tests -- -D warnings
PASS
```

Rust language-server diagnostics are clean for the changed Rust files.

## Full-suite observation

`cargo test --lib` completed with `523 passed`, `8 failed`, and `5 ignored`. Every
agent test passed. This PR is intentionally based directly on `main` and does not include
the independent node-fixture work from issue `#188`, so its two node failures remain in
this branch's full-suite result. The other failures are outside this sprint's changed
files:

- Two Comms named-pipe timeout failures tracked by PR `#180`.
- Four Memory tests whose Unix-only fixture cannot provide the expected files or
  long-lived process on Windows; these require separate tracking and a bounded fix.

No failing test was deleted, disabled, or modified.

## Current claim

The node-agent fixture is locally Windows-clean: the supervisor starts the native helper,
the minted bearer round-trips over a real loopback connection, and the complete agent
test module passes. Production process spawning and authentication are unchanged. The
work is not yet committed, reviewed, merged, released, or packaged-app tested.
