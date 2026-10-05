---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-1 draft)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core
companions: 02_ARCHITECTURE.md, 05_SPRINTS_AND_WPS.md, gates.yaml
---

# Formal Specs Plan

Modules live in `src-tauri/formal/` next to the existing ones and run through
`scripts/run-tlc.sh`. Rule 9: **extend** `SidecarSupervisor.tla` rather than writing a second
supervisor model. One new module, `AppExitCoordinator.tla`, holds what the supervisor model
cannot (several owners, the app gate, the final action). Each is TLC-green at small bounds
before gate0 closes, with the run cited in gates.yaml, and again at the close of the WP that
implements it.

## Lesson carried into the plan

The existing model was green while bugs (a) to (e) were present, because each bug sat outside
its abstraction (00_OVERVIEW, "How this was missed"). Two rules follow:

1. **Mutant configs that must fail.** For each reproduced bug the model can express, a
   mutant config re-introduces the defective transition, and TLC must report the matching
   invariant violated. A mutant that passes means the invariant has no teeth. Mutant runs
   are recorded with the green runs.
2. **Refinement notes.** Each action carries a comment naming the Rust function and arm it
   models (as the current file does for `run_monitor`). A WP that changes one of those arms
   updates the note in the same PR; the fault-injection suite (SCL-S13) is the
   code-level check that the note is still true.

## 1. `SidecarSupervisor.tla` (extended)

Current: states `{Off, Starting, Running, Backoff, Failed}`, variables `failures`, `child`,
`healthyRun`, `crashes`; INV-1 to INV-4; configs `SidecarSupervisor.cfg` (52 distinct
states, recovering child) and `SidecarSupervisor_ForkBomb.cfg` (12 states). The header
still names `src-tauri/src/supervisor.rs`; the module moved to `kit/src/supervisor.rs`, and
the extension fixes the reference.

**New variables**

| Variable | Models |
|---|---|
| `phase` | `{Stopped, Starting, Alive, Backoff, Failed, Stopping, Quarantined}` (replaces `state`; INV-1 to INV-4 are restated over it) |
| `desired` | `{Run, Stop}`, the latest committed intent |
| `intentSeq` | intent sequence, bounded by `MaxIntents` |
| `gen`, `inc` | service generation and child incarnation, bounded |
| `tickets` | set of admitted spawn or publication tickets `[seq, gen, inc]` |
| `probe` | `{Free, Busy}` plus `probeInc`, the incarnation the in-flight probe belongs to |
| `okWindow` | consecutive fresh successful samples of the current incarnation (abstract ticks) |
| `profile` | constant `{Probed, LivenessOnly}` |
| `cleanup` | owned resources still to release for a closed incarnation |

**New and changed actions** (each annotated with its Rust site when implemented)

`RequestStart`, `RequestStop` (commits `desired = Stop`, bumps `intentSeq`, closes ticket
admission), `AdmitSpawn` (only if the ticket's `seq = intentSeq` and `desired = Run`),
`SpawnReturns` (a late return after Stop goes to teardown), `ProbeBegin` (needs
`probe = Free`), `ProbeOk`, `ProbeFail`, `ProbeTimeout` (publishes `Unavailable`, keeps
`probe = Busy`), `ProbeWorkerEnds` (frees the permit), `ProbeNeverEnds` (leads to
`Quarantined`), `GraceTick`, `ElapsedTick` (time passes with no probe result),
`ResetCredit`, `ReplaceConfig` (bumps `gen` only after old `cleanup = {}`), `StaleCompletion`
(a result tagged with an old `gen` or `inc` arrives).

**Invariants**

| Id | Name | Statement | Reproduced bug it pins |
|---|---|---|---|
| INV-1 | `NeverFailedWhenRecovering` | kept for `LivenessOnly`; for `Probed`, a child that produces sustained fresh successes between crashes is never permanently `Failed` | (F-1, existing) |
| INV-2 | `CounterBounded` + `EventuallyFailedIfNeverHealthy` | kept; for `Probed`, "healthy" now means attested | (fork bomb, existing) |
| INV-3 | `StopIsNotACrash` | kept | |
| INV-4 | `NoOrphan` | restated: `phase \in {Stopped, Failed}` implies no live child and no admitted ticket | |
| INV-5 | `StopNeverSpawns` | `desired = Stop` implies no `AdmitSpawn` step is enabled, in **every** phase, including `Stopped` and `Backoff` | **bug (a)** |
| INV-6 | `RepeatedStopJoins` | a Stop with the same target as the pending one changes neither `inc` nor `tickets` | bug (a) |
| INV-7 | `CreditOnlyFromAttestedHealth` | for `Probed`, `failures` resets only when `okWindow >= HealthyAfterTicks`; `GraceTick` and `ElapsedTick` never reset it | **bug (b)** |
| INV-8 | `GraceNeverCredits` | no `ResetCredit` step while every sample since the spawn was a failure or a timeout | bug (b) |
| INV-9 | `LivenessOnlyResetUnchanged` | for `LivenessOnly`, the reset rule is today's elapsed-Running rule, and readiness stays `NotAssessed` | (D-6) |
| INV-10 | `ProbePermitBounded` | at most one probe worker in flight per service, across generations | **bug (c)** |
| INV-11 | `TimeoutNeverFreesPermit` | `ProbeTimeout` leaves `probe = Busy`; only `ProbeWorkerEnds` frees it | bug (c) |
| INV-12 | `StaleCompletionInert` | a result with an old `gen` or `inc` changes no readiness, counter, token or file | |
| INV-13 | `ReadyIsCurrent` | readiness `Ready` implies the last probe success belongs to the current `inc` and is fresh | |
| INV-14 | `ReplaceAfterCleanup` | `gen` advances only when the previous incarnation's `cleanup = {}` | |
| INV-15 | `QuarantineNotRecycled` | from `Quarantined`, only `ProbeWorkerEnds` plus an explicit Start leaves it; a timeout never does | |

Bugs (d) (bytes) and (e) (descendants) are not state-machine properties at this
abstraction. They are pinned by the fault-injection suite (SCL-S13) and, for (e), by the
`AppExitCoordinator` scope rule below.

**Configs**

| Config | Purpose | Expected |
|---|---|---|
| `SidecarSupervisor.cfg` | recovering child, `LivenessOnly` | no error |
| `SidecarSupervisor_Probed.cfg` | recovering child, `Probed`, grace longer than `HealthyAfterTicks` | no error |
| `SidecarSupervisor_ForkBomb.cfg` | crash-loop only | no error; reaches `Failed` |
| `SidecarSupervisor_ProbedForkBomb.cfg` | always-failing probe, grace longer than `HealthyAfterTicks` | no error; reaches `Failed` |
| `SidecarSupervisor_MutStopOff.cfg` | mutant: Stop in `Stopped` continues the spawn loop | **INV-5 violated** |
| `SidecarSupervisor_MutElapsedCredit.cfg` | mutant: `ElapsedTick` grants credit for `Probed` | **INV-7 violated** |
| `SidecarSupervisor_MutTimeoutFrees.cfg` | mutant: `ProbeTimeout` frees the permit | **INV-10 violated** |

The mutant configs select a mutant action through a constant (for example
`Mutant \in {"none", "StopOff", "ElapsedCredit", "TimeoutFrees"}`), so the mutants live in
the same module and cannot drift from it.

## 2. `AppExitCoordinator.tla` (new)

**Models** a fixed set of owners `Owners` (the 9 lifecycle cells, an abstract bespoke
registry, and the Hermes runtime as a nested owner), the app gate `{Open, Closing, Exited}`,
an absolute deadline as a bounded tick counter, exit triggers, and the final action.

**Variables:** `app`, `ownerState[o] \in {Running, Draining, Complete, Incomplete}`,
`tickets[o]`, `deadlineLeft`, `trigger \in {None, Quit, WindowClose, Restart,
InstallWin, InstallUnix, Reset, ForceExit}`, `final \in {None, Exit, Restart, Installer}`,
`reported \in {None, Complete, Incomplete}`, `hermesChildren \in {Running, Complete,
Incomplete}`.

**Actions:** `Trigger(t)` (any exit path), `BeginClosing`, `AdmitTicket(o)` (only while
`Open`), `TicketReturnsLate(o)`, `DrainHermesFirst`, `DrainOthers`, `OwnerComplete(o)`,
`OwnerStalls(o)`, `Tick`, `Expire`, `FinalAction`, `ForceExit` (models crash, logout and
external kill), `RepeatTrigger`.

**Invariants and properties**

| Id | Name | Statement |
|---|---|---|
| X-1 | `EveryExitPathRoutes` | every `trigger # None` other than `ForceExit` leads through `Closing`; no `final # None` without passing `Closing` |
| X-2 | `NoFinalBeforeComplete` | `final # None` implies every owner is `Complete` and `deadlineLeft > 0` at admission |
| X-3 | `ClosingRejectsAdmission` | `app = Closing` implies no `AdmitTicket` step is enabled |
| X-4 | `AdmittedTicketsDrained` | an owner is `Complete` only when its `tickets` set is empty |
| X-5 | `ExpiredNeverAuthorizes` | after `Expire`, `FinalAction` is disabled until an explicit retry trigger |
| X-6 | `NoRecursiveDrain` | `RepeatTrigger` while `Closing` changes nothing but the joined trigger |
| X-7 | `HermesChildrenScope` | the Hermes owner is `Complete` only if `hermesChildren = Complete` (the nested-ownership rule that pins bug (e) at this level) |
| X-8 | `ForceExitNeverComplete` | `ForceExit` implies `reported # Complete` |
| X-9 | `HermesDrainsFirst` | (O-11) no non-Hermes owner enters `Draining` before Hermes is `Complete` or `Incomplete` |
| L-1 | `CloseTerminates` | under weak fairness of owner progress and `Tick`, `<>(app = Exited \/ reported = Incomplete)` |

**Configs:** `AppExitCoordinator.cfg` (3 owners plus Hermes, all triggers, small deadline);
`AppExitCoordinator_Mut*.cfg` mutants: install on Windows calls `FinalAction` without
`Closing` (X-1 must fail), an admission while `Closing` (X-3), a final action after expiry
(X-5), Hermes `Complete` while children run (X-7).

## 3. Optional third module

If the retained-anchor ordering (no group signal after the leader is reaped) and the Windows
Job completion rule cannot be added to `SidecarSupervisor.tla` without a state explosion,
SCL-S1 adds `OwnedContainment.tla` with invariants `NoActuationAfterReap`,
`AbsenceOnlyFromObservation` and `JobCompleteOnlyAtZero`. The decision is recorded in
SCL-S1's EVIDENCE.

## Owning WPs

| Module / config | WP |
|---|---|
| `SidecarSupervisor.tla` extension, all configs and mutants | SCL-S1.1 (spec), re-run at SCL-S2.4, S3.3, S4.1 |
| `AppExitCoordinator.tla`, configs and mutants | SCL-S1.2 (spec), re-run at SCL-S12.1, S12.5 |
| `OwnedContainment.tla` (if needed) | SCL-S1.1 decision, then SCL-S9.2, S10.2 |
| TLC in CI including mutants | SCL-S13.4 |
