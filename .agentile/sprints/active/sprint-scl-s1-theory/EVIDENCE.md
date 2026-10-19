---
created: 2026-10-18
branch: beta/k1a-gate0-reground
author: "Larry Klosowski + Claude Opus 5.5 (MAC lane)"
status: active (SCL-S1 gate0 evidence; sections are owned by lane)
sprint: sprint-scl-s1-theory
planset: .agentile/planset/2026-10-05-sidecar-lifecycle/
---

# SCL-S1 (gate0) evidence

Evidence for `gate0` in [gates.yaml](../../../planset/2026-10-05-sidecar-lifecycle/gates.yaml).
Each lane writes under its own heading: the formal models (S1.1, S1.2, S1.7) under their own
section, the harness and baselines (S1.3, S1.5) under "Harness and baseline" below. The
re-grounding (S1.4) is in [REGROUND.md](REGROUND.md).

## Harness and baseline

### S1.5 baseline (Rule 2)

Commit pin: citrate-core `main` at `b521d98`. Host: macOS 15 arm64 (owner Mac, MAC lane),
rustc 1.96.0, Node 25.9.0. Recorded 2026-10-18.

| Measure | Command | Result at `b521d98` |
|---|---|---|
| Rust workspace tests | `cargo test --workspace --locked --no-fail-fast` | **2434 passed, 0 failed, 20 ignored** (wall time 3 min 20 s, warm cache) |
| of which `citrate_core_lib` (src-tauri) | same run | 1971 passed, 18 ignored |
| of which `citrate_core_kit` lib | same run | 347 passed, 1 ignored |
| of which `citrate-components` (all targets) | same run | 82 passed, 1 ignored |
| of which `citrate-templates` (all targets) | same run | 33 passed |
| of which doc tests | same run | 1 passed |
| Kit supervisor tests, Unix | `cargo test --locked -p citrate-core-kit --lib supervisor::tests` | **24** (all pass) |
| Kit supervisor tests, Windows | `supervisor_windows_tests.rs` | **3** (run on `windows-latest` by `lifecycle-platforms.yml`; not runnable on this host) |
| Frontend tests | `npx vitest run` | **2586 passed, 25 skipped** (2611 tests in 277 files: 275 passed, 2 skipped) |
| TLC state counts | (formal lane) | recorded by the S1.1 / S1.2 / S1.7 section |

The planset's S1.5 line says "kit supervisor tests (22 + 3 Windows)"; 22 predates SCL-S0.3, which
added two Unix tests. The 24 / 3 split above matches the counts `lifecycle-platforms.yml` asserts.

This is a macOS count. Linux CI (`ci.yml`, ubuntu-latest) runs the same workspace command; the
count there can differ where tests are `cfg`-gated by OS. Every later SCL PR compares against
this table: the workspace count never goes down (Rule 2).

### S1.3 fault-injection harness

PR #342 (`beta/k1a-gate0-harness`), files under `kit/tests/`:

| Piece | Where |
|---|---|
| Owned-process fixtures, marker-checked guard (kill and reap every recorded pid, from `Drop`) | `kit/tests/lifecycle/fixture.rs` |
| Monotonic timing helpers (`Deadline`, `wait_for`, `hold_for`, `sample_over`) | `kit/tests/lifecycle/timing.rs` |
| The suite | `kit/tests/lifecycle_faults.rs` |
| Suite manifest (kept in step with the suite by `manifest_lists_every_fixture`) | `kit/tests/lifecycle_faults.manifest` |
| CI job stub (ubuntu-latest, macos-14; `pgrep -f citrate-lcf-` before and after) | `.github/workflows/lifecycle-faults.yml` |

Run on this host at the harness commit `4b69bd8` (on top of `b521d98`):

- `cargo test --locked -p citrate-core-kit --test lifecycle_faults`: **10 passed, 0 failed**, three
  consecutive runs, about 2.1 s each. `pgrep -fl citrate-lcf-` afterwards: no process.
- Positive controls green: one incarnation per fresh start over 2 s; one Stop ends and reaps the
  child and stays Off over 2 s; an external kill gives one distinct new incarnation; a crash loop
  runs once plus `max_retries` and then stays Failed.
- Harness mutation check: with the guard's kill removed, 2 harness tests fail
  (`harness_guard_cleans_up_after_a_panicking_body`,
  `harness_sweep_finds_marked_processes_missing_from_the_record`) and marked processes are left
  behind for the job's pgrep step. Restored afterwards.

What this does not show: the CI job has not run on hosted runners yet at the time of writing
(it runs on the PR); there is no Windows lane (S13.1); the job is not a required check (S13.1).
