---
created: 2026-07-13
branch: feat/core-c1-0b-supervisor-formal
author: Claude Fable 5
status: draft
---

# SidecarSupervisor formal model (CORE-C1.0b)

TLA+ model of the `SidecarSupervisor` state machine
(`src-tauri/src/supervisor.rs`), written for CORE-C1.0b to pin the C1.0b fixes
(F-1 consecutive-not-lifetime counter, the fork-bomb bound) and to cover the
transitions the Rule-8 review (`citrate-security`
`reviews/2026-07-13-rule8-c1-0-supervisor`) flagged as untested. The supervisor
is the substrate for every Phase-C sidecar, and the example-based tests MISSED
F-1 — so this model checks the counter-reset semantics exhaustively.

## Files

- `SidecarSupervisor.tla` — the model (states, transitions, invariants).
- `SidecarSupervisor.cfg` — RECOVERING-child config (`CanHealthy = TRUE`):
  pins INV-1 (never permanently Failed while recovering) + the safety invariants.
- `SidecarSupervisor_ForkBomb.cfg` — CRASH-LOOP-ONLY config
  (`CanHealthy = FALSE`, never stopped): pins INV-2 (the fork-bomb bound) as both
  a safety invariant AND a liveness property (the run actually reaches Failed).

## States and transitions

States `{Off, Starting, Running, Backoff, Failed}` mirror `SupervisorState`.
Transitions map 1:1 to `run_monitor` arms: `SpawnOk`, `SpawnFail`,
`BecomeHealthy` (the F-1 reset), `Crash`, `Unhealthy`, `TryWaitError`,
`RetryFromBackoff`, `Stop`, `StopInBackoff` (cancels a pending restart),
`Shutdown`.

Modeling fidelity for F-1: in the recovering config a `Crash` can only occur
AFTER the episode became sustained-healthy (`healthyRun` = TRUE), which is the
real distinction — a long-lived daemon runs past `healthy_after` between crashes
(resetting the counter), while a fast crash-loop exits before it (never resets).

## Invariants and the Rust tests they mirror

| Invariant | Meaning | Rust test(s) (`supervisor_tests.rs`) |
|---|---|---|
| **INV-1** `INV_NeverFailedWhenRecovering` | An intermittently-crashing-but-recovering child is NEVER permanently Failed (the F-1 property). | `intermittent_crashes_with_healthy_runs_never_permanently_fail` |
| **INV-2** `INV_CounterBounded` + `EventuallyFailedIfNeverHealthy` | A crash-loop-only child (no healthy interval) reaches Failed within `max_retries` (the fork-bomb bound). | `crash_loop_without_healthy_interval_still_hits_cap_after_reset_fix`, `fork_bomb_is_bounded_and_backoff_increases`, `fork_bomb_bound_is_load_bearing`, `spawn_failure_reaches_failed_within_bound` |
| **INV-3** `INV_StopIsNotACrash` (+ Stop/StopInBackoff/Shutdown keep `crashes` UNCHANGED) | An intentional stop never triggers a restart and never records a crash. | `graceful_stop_kills_child_and_does_not_restart`, `stop_during_backoff_cancels_pending_restart` |
| **INV-4** `INV_NoOrphan` | Off/Failed implies no live child (no orphan). | `drop_kills_child_no_orphan`, `drop_with_bounded_join_returns_promptly` |

F-2 (the non-blocking health probe + bounded teardown) is a liveness property of
the implementation rather than a state-machine invariant; it is pinned by the
Rust tests `hanging_health_probe_does_not_stall_crash_detection_or_stop` and
`drop_with_bounded_join_returns_promptly`. The model captures the related
transition faithfully: the probe is an event that cannot pre-empt the crash/stop
transitions.

## Running TLC

Requires Java + `tla2tools.jar`. On the build machine these were at
`/opt/homebrew/opt/openjdk/bin/java` and `~/.local/share/tla/tla2tools.jar`.

```sh
JAR=~/.local/share/tla/tla2tools.jar
JAVA=/opt/homebrew/opt/openjdk/bin/java   # or `java` if on PATH
cd src-tauri/formal

# INV-1 + safety (recovering child)
"$JAVA" -cp "$JAR" tlc2.TLC -config SidecarSupervisor.cfg SidecarSupervisor.tla

# INV-2 fork-bomb bound (safety + liveness, crash-loop-only child)
"$JAVA" -cp "$JAR" tlc2.TLC -config SidecarSupervisor_ForkBomb.cfg SidecarSupervisor.tla
```

## Run result (2026-07-13)

Both configs were run headless with TLC 2026.05.26:

- `SidecarSupervisor.cfg` — **No error found** (52 distinct states; INV-1..4 +
  TypeOK hold).
- `SidecarSupervisor_ForkBomb.cfg` — **No error found** (12 distinct states;
  INV-2 + INV-4 + the `<>Failed` liveness property hold).

Negative control (model has teeth): removing the counter reset in `BecomeHealthy`
(re-introducing the F-1 lifetime-counter bug) makes TLC report
`Invariant INV_NeverFailedWhenRecovering is violated` with a concrete
counterexample trace reaching Failed — the exact bug the example tests missed.

Note: the `crashes` counter is bounded by `MaxCrashes` (a `CONSTRAINT` in the
primary cfg) purely to keep the state space finite; it does not weaken the
invariants (INV-1..4 are inductive on `state`/`failures`).

---

# MemoryPack formal model (Hermes P1 / WP1.1)

TLA+ model of the docs-corpus packer (`docs_ingest::ingest_docs_incremental` +
`memory::ingest_docs_corpus`). Pins the "monotone, no dupes" property the
content-hash seen-set provides, so a corpus that GROWS across app versions
(WP1.2's reference packs added to the Almanac docs) re-packs only the new chunks.

## Files
- `MemoryPack.tla` — states (`corpus`, `packed`, `seen`, `lastRun`) + actions
  (`Ingest`, `GrowCorpus`, `WipeStore`).
- `MemoryPack.cfg` — a 4-hash universe; checks TypeOK + NoDupes + Integrity and
  the `MonotoneUnderIngest` action property.

## Invariants and the code they mirror
- **INV-Pack-1 (monotone)** — `MonotoneUnderIngest`: an `Ingest` step only grows
  `packed`. Mirrors `ingest_docs_incremental` unioning `corpus \ seen`. `WipeStore`
  is the one intentional exception (a wiped store), modeled explicitly.
- **INV-Pack-2 (no dupes)** — `packed`/`seen` are sets and `Ingest` only adds
  `corpus \ seen`, disjoint from `seen`. Mirrors the sha256 seen-set skip.
- **INV-Pack-3 (integrity)** — `Integrity`: `packed \subseteq corpus`; the packer
  never invents a node (Rule 1).

## Run result (2026-09-11)
Run headless with TLC (`tla2tools.jar`, same harness as above):
- `MemoryPack.cfg` — **No error found** (221 distinct states; TypeOK + NoDupes +
  Integrity + the MonotoneUnderIngest property hold).

Also present: `ModelRouter.tla`/`.cfg` (Hermes P0 / WP0.1 — the model-router
selection invariants).

---

# ConsentGate formal model (Telemetry WP-T.1)

TLA+ model of the telemetry consent gate — the property `telemetry_send` (the one pinned
HTTPS POST, `telemetry.rs`) must satisfy: **nothing egresses except a bundle the member
reviewed AND consented to, and only while the telemetry toggle is on.**

## Files
- `ConsentGate.tla` — states (`toggle`, `reviewed`, `consented`, `egressed`, `reviewedBody`,
  `sentBody`) + actions (`ToggleOn`/`ToggleOff`, `Review`, `Consent`, `Egress`).
- `ConsentGate.cfg` — a 2-bundle universe; checks TypeOK + INV_Consent_1 + INV_Consent_3 and
  the INV_Consent_2 action property.

## Invariants
- **INV-Consent-1** — every egressed bundle was consented (`egressed ⊆ consented`). Mirrors
  `telemetry_send` being called only from the consented review flow (WP-T.4).
- **INV-Consent-2** — no egress step occurs while the toggle is off (`Egress` guards on
  `toggle = TRUE`). Toggle-off keeps the historical consent record; it doesn't erase it.
- **INV-Consent-3** — what was sent equals what was reviewed (`sentBody = reviewedBody`, nonzero)
  — the UI sends exactly the bundle it displayed.

## Run result (2026-09-11)
Run headless with TLC (`tla2tools.jar`): **No error found** (32 distinct states; TypeOK +
INV_Consent_1 + INV_Consent_3 + the INV_Consent_2 property hold). Negative control (the model
has teeth): an earlier draft cleared `consented` on `ToggleOff`, and TLC produced a concrete
counterexample violating INV-Consent-1 (a legitimately-sent bundle no longer showed as
consented) — fixed by keeping consent as a historical record.
