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
