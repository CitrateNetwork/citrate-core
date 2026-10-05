---
created: 2026-10-03T00:00:00-07:00
branch: test/windows-hermes-refusal-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-HERMES-REFUSAL-FIXTURE
---

# Verification evidence

## Pinned state

- Base branch: `main`
- Base SHA: `b8d26e84cb12954d027b35bfadf5f63659e784ef`
- Host: Windows, `x86_64-pc-windows-msvc`
- Issue: `CitrateNetwork/citrate-core#186`

## Red evidence

Two consecutive `cargo test --lib` runs failed
`hermes::brief_tests::the_ureq_transport_keeps_the_body_of_a_refusal` while the
exact test passed alone. The full-suite failures were:

```text
Transport: connection aborted by local software (os error 10053)
Transport: connection forcibly closed by remote host (os error 10054)
```

Each red run completed with `518 passed`, `13 failed`, and `5 ignored`.

## Green evidence

```text
cargo fmt --all -- --check
PASS

cargo clippy --lib -- -D warnings
PASS

cargo test --lib hermes::brief_tests::the_ureq_transport_keeps_the_body_of_a_refusal -- --exact --nocapture
1 passed; 0 failed

cargo test --lib hermes::brief_tests:: -- --nocapture
11 passed; 0 failed

cargo test --lib
519 passed; 12 failed; 5 ignored
```

The Hermes refusal test passed under the full parallel-suite load. The 12 remaining
failures are outside `hermes_brief_tests.rs`; they are the existing Windows IPC and
sidecar-fixture failures tracked separately. The test count remains 536.

Rust language-server diagnostics are clean for `hermes_brief_tests.rs`.

## Clippy boundary

`cargo clippy --lib --tests -- -D warnings` reaches the unchanged `ipc_name.rs` test
and fails on its existing Windows-only unused `dbg` variable. That defect is tracked
by issue `#184` and fixed in PR `#185`; it is intentionally not duplicated in this
branch. The targeted tests compile and pass with this branch's changed test code.

## Current claim

The one-shot POST fixture now consumes the complete advertised request body before it
writes and closes the 422 response. The production `UreqControl` implementation is
unchanged. This work is not yet reviewed, merged, or released.
