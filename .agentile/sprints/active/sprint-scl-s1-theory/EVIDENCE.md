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
harness. Counts on this host after the review rounds (#351 at `cca5a85`, #352 at `c39db6b`):

| Measure | Value |
|---|---|
| `cargo test --workspace --locked` | #351: 2463 passed; #352: 2488 passed; 0 failed, 20 ignored (baseline 2434) |
| kit `supervisor::tests` | #351: 32 Unix / 11 Windows; #352: 38 / 17 (baseline 24 / 3) |
| `lifecycle_faults` suite | #351: 21; #352: 40 |

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

## SCL-S3

SC2: readiness separate from liveness, and attested retry credit. Stacked PR on #352
(`beta/k1a-sc2-readiness`, base `beta/k1a-sc1-deadlines` at `c39db6b`). Host: macOS 15 arm64
(owner Mac, MAC lane), rustc 1.96.0, own target dir. Recorded 2026-10-19.

### Red first

| Commit | What | Result on the code before the fix |
|---|---|---|
| `150be77` | 8 fault fixtures on the existing API | 6 fail, 2 positive controls pass. Bug (b) reproduced: grace 120 ms, healthy_after 40 ms, max_retries 1, always-false probe, still Running after 3 s with **16 children**. Late probe: never Failed. Probe workers across incarnations: **3 at once**. Jumping wall clock: **22 children**, never Failed. Readiness: always `NotAssessed` |
| `2fcb8c0` | 6 unit tests + 2 fixtures on the new API (readiness DTO, split counters, member and automated starts) | Red by compile: the API did not exist |
| `727c051` | The fix | All green |
| `ac893a5` | Four fixtures made robust to a host at load average ~120 (they were reading timing, not the property) | All green |

### What the fix does

- **S3.1 readiness.** Profile from the spec: `Probed` with a `HealthCheck`, `LivenessOnly`
  without. Probed: `Awaiting` on spawn, `Ready` on a fresh success of the current incarnation,
  `Unavailable` after a failed or timed-out sample inside the grace, `Stale` once the last
  success is older than two probe intervals (SPEC-001's starting freshness). LivenessOnly:
  always `NotAssessed`. Outside `Alive` the snapshot shows the profile's not-ready value. A
  result for another incarnation, or one that arrives after its worker's timeout, is discarded.
  The D-12 snapshot adds `readinessProfile`, `readinessAgeMs` (monotonic) and `counters`, all
  counters as decimal strings. `Supervisor::is_ready()` reads the cached cell.
- **S3.2 attested credit (bug b).** Probed: the budget resets only when fresh successes span
  `healthy_after` with no gap over the freshness limit, once per window. Grace and elapsed time
  never reset it. LivenessOnly keeps the elapsed rule and its durations. Everything runs on
  `Instant`; the injected `Clock` stamps crash records only.
- **One probe at a time per service.** The monitor keeps one probe slot across incarnations. A
  worker that times out or outlives its incarnation keeps the slot until it really ends. If the
  slot is still taken a whole interval after a sample was due, that incarnation could not be
  sampled: inside the grace this ends the window, outside it the run is unhealthy (one counted
  failure). SC3's permit, quarantine and adapters (SCL-S4) are not built here; the slot is the
  seam they replace.
- **Member and automated starts (owner rule, #345 B2 and #351).** `request_start` is the member
  Start and the only control that gives a fresh budget. New `request_auto_start` and
  `request_auto_restart` carry the budget over and never revive a `Failed` service
  (`ControlResult::RequiresMemberStart`). An automated intent that arrives with a spent budget
  settles `Failed` without a spawn. Crash retries inside one intent never reset the budget.
- **S3.3 counters.** `attempts`, `respawns`, `initialFailures`, `consecutiveFailures`,
  `maxRetries`, `healthCredits`. `restarts` stays the legacy recorded-failure counter.

### Counts (Rule 2)

| Measure | Before (`c39db6b`, #352 head) | After (this branch) |
|---|---|---|
| `cargo test --workspace --locked` | 2488 passed, 0 failed, 20 ignored (re-run here) | **2504 passed, 0 failed, 20 ignored** |
| kit lib | 361 | **367** |
| kit `supervisor::tests` | 38 Unix / 17 Windows | **44 / 23** (the 6 new unit tests are platform-neutral; `lifecycle-platforms.yml` updated) |
| `lifecycle_faults` suite | 40 | **50** |

### Fixtures and what proves what

| Fixture or test | Acceptance it proves |
|---|---|
| `grace_longer_than_healthy_after_never_credits_a_failing_probe` | S3.2 red-first criterion and 04 E2 scenario: Failed within 3 s, at most 2 children (INV-7, INV-8) |
| `late_probe_answers_never_make_ready_or_credit` | late answers never Ready, never credit; never two workers at once inside the grace |
| `probe_workers_never_overlap_across_incarnations` | at most one probe worker per service across incarnations |
| `a_stale_probe_success_never_makes_the_new_incarnation_ready` | an earlier incarnation's success is inert (INV-12, INV-13) |
| `a_probed_service_is_awaiting_then_ready_then_unavailable` | readiness moves apart from liveness; no restart inside the grace |
| `a_liveness_only_service_is_alive_and_never_assessed` (positive control) | D-6, INV-9 |
| `a_forward_wall_clock_jump_never_grants_credit` | monotonic clock only |
| `sustained_fresh_success_resets_the_retry_budget` (positive control) | 04 E2 "Sustained health earns credit", one credit per window (INV-1) |
| `automated_restarts_never_reset_the_budget`, `an_automated_start_carries_the_budget_over` | INV-17: failures since the member Start stay within `1 + max_retries` |
| unit: `readiness_profile_follows_the_configured_probe`, `ready_is_shown_only_while_fresh` | S3.1 |
| unit: `retry_counters_split_attempts_respawns_and_initial_failures` | S3.3 |
| unit: `an_automated_start_never_revives_a_failed_service`, `an_automated_start_after_a_stop_carries_the_spent_budget`, `an_automated_restart_needs_a_running_service` | member versus automated starts |

### Mutation check

Each inverse of the fix, applied alone and reverted afterwards, fails the suite:

| Mutant | Fixtures that fail |
|---|---|
| Elapsed time credits a probed service (bug b back) | `grace_longer_than_healthy_after_never_credits_a_failing_probe`, `late_probe_answers_never_make_ready_or_credit` (and one timing-dependent fixture in that run) |
| Automated starts and restarts reset the budget | `automated_restarts_never_reset_the_budget`, `an_automated_start_carries_the_budget_over` |
| A probe timeout frees the slot | `probe_workers_never_overlap_across_incarnations`, `late_probe_answers_never_make_ready_or_credit` |
| Liveness credit read from the injected wall clock | `a_forward_wall_clock_jump_never_grants_credit` |

### Stability

12 parallel copies of the suite (3 rounds of 4) pass at a host load average of ~70, and 20 runs
of each automated-start fixture pass. No `citrate-lcf-` process of these runs was left.

### TLC re-run (INV-1, INV-2, INV-7, INV-8, INV-9, INV-17)

`tlc_expect.py SidecarSupervisor` on the model from #345 (`8ea9d53`, unchanged here), same
machine: **19 of 19 configs match** (green configs `SidecarSupervisor`, `_Probed`,
`_ForkBomb`, `_ProbedForkBomb`, `_ForkBombAutoRestart`, `_ProbedForkBombAutoRestart`; every
mutant fails on its named invariant, including `MutElapsedCredit` on INV-7 and
`MutAutoStartResets` on INV-17; every witness is violated).

Refinement notes for the formal lane (not changed here; the model lives on #345):

- `ResetCredit` (Probed) is now `supervise_running`'s attested window; `AutoStart` and
  `AutoRestart` are `request_auto_start` and `request_auto_restart`; `MemberStart` is
  `request_start`. `ProbeTimeout` keeps the slot, as the model does.
- **Divergence to model:** when the slot is held by a stale or timed-out worker a whole interval
  past a sample's due time, the code counts one failure outside the grace (a `FailStep`). The
  model has no such step (`ProbeBegin` needs a free permit; the only exit is `ProbeNeverEnds`,
  SC3). The step only adds counted failures, so INV-2, INV-7 and INV-17 are unaffected; it
  should be added to the model with SC3.
- The model's open item 3 (a partly healthy, early-crashing config) is still open for the
  formal lane; the code side is covered by `late_probe_answers_never_make_ready_or_credit` and
  the stale-success fixture.

### Not done in SC2

- **Consumers.** No `src-tauri` change. The managers still recreate their supervisor on every
  start, so each manager start is a fresh budget until the owner migrations: the node's mining
  arm and disarm move to `request_auto_restart` in S6.1, and chat routing reads readiness in
  S7.1 and S7.5.
- **Per-service readiness scope labels** (HTTP `/health` versus IPC connect) are not in the
  DTO; the profile is. They land with the owners (S7.1 to S7.3).
- **SC3 (SCL-S4):** the permit across generations, quarantine of a worker that never ends,
  INV-16 for probe workers on Stop, the literal-only HTTP adapter and the IPC adapters.
- Freshness (two intervals) is SPEC-001's starting value; S14 measures it on packaged builds.

### SCL-S3 review round 1 (DGX, changes requested at `56ecf82`)

- **Rebased onto the current #352 head**, `d539d1e`. This picks up `75272cc` (shutdown waits for
  readers; a quarantine reconciles without a monitor), `21ed683` (load hardening) and the #351
  round-3 fixes (tokens carry their supervisor instance; a pending Start is joined in every
  phase). Resolved conflicts:
  - `Cell`: both sides' fields are kept (`closed`, `closed_readers`, `instance`, plus the SC2
    readiness, counters and member-start fields).
  - `start_intent`: keeps #351's "join a pending Start in every phase" ahead of the
    member/automated split.
  - The lifecycle unit tests from both sides are all kept.
  - None of the SC2 tests build a `ControlToken` by hand, so the new `instance` field needed no
    test change.
- **A late probe answer no longer counts** (red `13dc35a`, fix `115e7ff`). The monitor collected a
  finished worker before it checked that worker's deadline, so an answer at 55 ms on a 50 ms
  interval, first seen in the same poll, was read as a fresh success (Ready at 435 ms). Now:
  - the worker records its own monotonic finish time;
  - an answer counts only for the current incarnation, if the monitor had not timed it out, and
    only if it finished within the interval;
  - a late answer collected before its timeout was noticed is treated as that timeout;
  - a worker that was already timed out is inert, so no sample is counted twice.
  - Fixture: `a_probe_answer_after_its_interval_never_counts`.
- **Load hardening** (`ab324e4`). The SC2 fixtures that wait for Failed now assert counts with a
  90 s bound (`SLOW_FAILED`) instead of 3 to 15 s windows. The sustained-credit fixture uses a
  250 ms interval and waits for each window's credit.

| Measure | #352 head `d539d1e` | This branch `ab324e4` |
|---|---|---|
| `cargo test --workspace --locked` | (2511 - 17) | **2511 passed, 0 failed, 20 ignored** |
| kit `supervisor::tests` Unix / Windows | 41 / 19 | **47 / 25** (`lifecycle-platforms.yml`) |
| `lifecycle_faults` | 43 | **54** |

**Repeated runs** (18 concurrent copies × 5 rounds, own fresh target dir, no `citrate-lcf-` process
left after any round):

| Build | Host load average | Result |
|---|---|---|
| this branch | 260 to 270 | 88/90. Each failure is in an SC1 fixture: `publication_ticket_released_before_stop_settles_is_in_the_complete_scope` (readers missed the 250 ms stop window) and `start_after_health_failures_with_a_held_reader_waits_for_cleanup` (no pid recorded in 15 s) |
| `d539d1e` control | 125 to 150 | 90/90 |
| this branch, run again | 95 to 115 | **90/90** |

At a load average of about 260 the second SC1 fixture can lose its first child before the shell
records a pid. Its probe answers healthy until then, and with the late-answer rule an answer
slower than its 50 ms interval now counts as a failed sample. That is the intended rule. If that
fixture flakes in CI, the fix belongs in the fixture (a longer probe interval), not in the rule.
