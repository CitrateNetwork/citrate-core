---
created: 2026-10-05T18:30:00Z
branch: scl/s0.1 (private route; merges into release/0.5.0-hermes-upskill)
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: SCL-S0
---

# Sprint SCL-S0: Evidence

Close-with-proof records per WP. Private WPs (S0.1, S0.3, S0.4) are recorded here in generic
terms only; specifics are on federation #298.

## Rule-2 baseline

`cargo +1.98.1 test --workspace --locked` on `release/0.5.0-hermes-upskill` `6ab289e` (macOS
arm64): 2,314 passed, 0 failed, 18 ignored.

## S0.1: Startup cleanup matches exact owned executable paths only

- New module `src-tauri/src/owned_cleanup.rs`, tests in `owned_cleanup_tests.rs`, native
  fixture `src-tauri/tests/fixtures/cleanup_fixture.rs`. Owned paths come from the same
  resolvers the managers launch the sidecars with (`resolve_owned_sidecar` in `lib.rs`).
- Red: 12 new tests run against the previous matching rule behind the same seam; the three
  decoy fixtures (command-line mention, `argv[0]` naming our binary, owned binary under a live
  instance) failed; the positive control (a leftover at the exact owned path is stopped) passed.
- Green (macOS arm64): 12 of 12. Workspace after: 2,326 passed, 0 failed, 18 ignored (+12).
  The same module tests also pass 12 of 12 on Linux (aarch64 container, standalone harness).
- Gates: `cargo +1.98.1 fmt --all -- --check` clean; `cargo +1.98.1 clippy --workspace
  --all-targets --locked -- -D warnings` clean; `npm run typecheck` clean.
- Every fixture process is recorded and stopped by a guard; no fixture process remained after
  any run (process list checked).
- Open: native run on each OS's installed package (US-0.1 AC2), recorded on #298.
