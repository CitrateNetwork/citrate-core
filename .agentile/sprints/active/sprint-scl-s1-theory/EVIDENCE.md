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

### SC1 (SCL-S2) status and deferrals

Work landed in the stacked PRs #351 (S2.1, S2.2) and #352 (S2.3, S2.4), built on the S1.3
harness. Counts on this host after the review rounds (#351 at `cca5a85`, #352 at `21ed683`):

| Measure | Value |
|---|---|
| `cargo test --workspace --locked` | #351: 2463 passed; #352 (`21ed683`): 2490 passed; 0 failed, 20 ignored (baseline 2434) |
| kit `supervisor::tests` | #351: 32 Unix / 11 Windows; #352: 38 / 17 (baseline 24 / 3) |
| `lifecycle_faults` suite | #351: 21; #352: 42 |
| Concurrency proof (#352 `21ed683`) | 180 of 180 runs green (10 rounds of 18 concurrent suite copies, load average 80 to 140, own target dir) |

**Deferred: using credential-publication tickets in the managers.** Reason: the kit
provides the ticket (`Supervisor::admit_publication`, `PublicationTicket::commit`,
`StaleTicket::cleaned`), and a Stop waits for it, with fault fixtures to prove that. But the
generation-specific bearer and seed files are written by the managers in `src-tauri`, not by the
kit. Routing those writes through tickets, and making the files generation-specific, is the
owner migration: SCL-S6.2 (node agent, memory), S7.2 (Comms, Cluster) and S7.3 (Hermes). It
lands with those owners.

Also not done in SC1:

- the app-close deadline record and deadline-bound Start (both belong to the coordinator, S12.1);
- the legacy `stop()`, which keeps its 10 s wait (consumers migrate in S6.4 / S7.4);
- replacement descriptors and generation bumps (no replacement API until SC5 / SC6);
- the INV-5, INV-6, INV-12 and INV-14 TLC re-runs, which belong to the formal lane.

**Follow-up for the formal model (#345, `SidecarSupervisor.tla`).** Review of #351 and #352 found
orderings the model could not express, because it folds log readers and credential files into
the `child` and `tickets` resources:

- a Start that supersedes a Stop mid-teardown, while a reader of the old incarnation is held;
- an explicit Start after `Failed`, where the earlier incarnation ended by crash or by health
  failures and its reader is still held;
- shutdown with an open publication ticket.

The model's `MemberStart` already waits for `StopComplete` with `Cleanup = {}`, and
`INV_StoppedOwnsNothing` holds. Proposed follow-up, not done in SC1:

- model `readers` as a resource of its own in `Cleanup`, released independently of `child`, on
  every way an incarnation ends (Stop teardown, Crash, health failure);
- require `Cleanup = {}` before the first spawn of a new Start intent, including from `Failed`;
- add a mutant that admits the spawn while `readers` is still held.

The fault suite covers these orderings today (`start_during_held_reader_teardown_waits_for_cleanup`,
`start_after_a_crash_with_a_held_reader_waits_for_cleanup`,
`start_after_health_failures_with_a_held_reader_waits_for_cleanup`,
`shutdown_with_an_open_publication_ticket_is_incomplete`). Control-result values
(`AlreadySatisfied`, `Quarantined`, `ObservationUnknown`) are outside the model by design.

**Also still open:** retries within one Start intent (crash, then backoff, then respawn) do not
wait for the previous incarnation's readers. That is reader handover, part of the bounded-capture
work (SC4, SCL-S5).

**Concurrency note.** Counts and stress runs must use a cargo target dir private to the
worktree. Path crates hash the same in every checkout, so a shared target dir can run another
checkout's stale test binary. "18 concurrent copies" means 18 started at once, not 6 at a time:
only the former exposed the shutdown-reader and timing issues fixed in #352 `75272cc` and
`21ed683`.
