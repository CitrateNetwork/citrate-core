---
created: 2026-10-19T01:10:00Z
branch: beta/k1a-gate0-tla
author: "Larry Klosowski + Claude Opus 5.5 (MAC lane)"
status: active
sprint: SCL-S1
planset: 2026-10-05-sidecar-lifecycle
code: SCL
gate: gate0 (g0-formal)
companions: ../../../planset/2026-10-05-sidecar-lifecycle/03_TLA_SPECS.md, ../../../planset/2026-10-05-sidecar-lifecycle/05_SPRINTS_AND_WPS.md, ../../../planset/2026-10-05-sidecar-lifecycle/gates.yaml
---

# SCL-S1 evidence

Evidence for the SCL-S1 (gate0) work packages, one section per WP or PR. Other lanes add their
own sections (S1.3 harness, S1.4 re-grounding, S1.5 baseline, S1.6 CI lanes). A recorded section
is not edited later except to fill in results that were pending.

## S1.1, S1.2, S1.7: TLA+ models (branch `beta/k1a-gate0-tla`)

Board item: "K1a · Services registry + Node › Services tab + SC1 + SC2 + S7.5a", gate0 theory half.
Base: citrate-core `origin/main` `b521d98`.

### Files

| File | WP | What |
|---|---|---|
| `src-tauri/formal/SidecarSupervisor.tla` | S1.1, S1.7 | Extended per 03 §1: phases `{Stopped, Starting, Alive, Backoff, Failed, Stopping, Quarantined}`, intent sequence, spawn tickets, generation and incarnation, probe permit plus in-flight workers, attested retry credit, readiness, quarantine. INV-1 to INV-15. Three mutants (`StopOff`, `ElapsedCredit`, `TimeoutFrees`) and three reachability witnesses. Refinement note on every action. Header path fixed to `kit/src/supervisor.rs` |
| `SidecarSupervisor.cfg`, `_ForkBomb.cfg` | S1.1 | The two existing configs, kept and restated over the new variables |
| `SidecarSupervisor_Probed.cfg`, `_ProbedForkBomb.cfg` | S1.1 | New green configs from 03 §1 (grace 3 longer than healthy_after 2) |
| `SidecarSupervisor_Mut{StopOff,ElapsedCredit,TimeoutFrees}.cfg` | S1.1 | The three named mutants |
| `SidecarSupervisor_Reach*.cfg` | S1.7 | One witness per mutant (RT-17) |
| `src-tauri/formal/AppExitCoordinator.tla` | S1.2, S1.7 | New, per 03 §2 with every red-team correction: runtime admission and `RuntimeClosing` (RT-08), X-2 and X-5 restated for Restart and Installer, X-9 with the Hermes sub-deadline (RT-26), X-10 (RT-01), X-11, X-12 (D-15) |
| `AppExitCoordinator.cfg` | S1.2 | Green: three owners plus Hermes, every trigger, deadline 3 ticks, Hermes sub-deadline 1 |
| `AppExitCoordinator_Mut*.cfg` (9) | S1.2, S1.7 | The four mutants from 03 §2, the four added by S1.7, and one more for X-9 |
| `AppExitCoordinator_Reach*.cfg` (9) | S1.7 | One witness per mutant |
| `src-tauri/formal/tlc_expect.py` | S1.1, S1.2 | Runs every config of a spec and checks it against the config's own `EXPECT:` line. This is the check S13.4 can put in CI |
| `src-tauri/formal/README.md` | | Section for the two models (how to run; results stay here) |

### How to reproduce

```sh
python3 src-tauri/formal/tlc_expect.py SidecarSupervisor AppExitCoordinator --markdown
```

Run on the owner's Mac (macOS, Apple silicon) with OpenJDK 27 (Homebrew) and TLC2 2.19
(`~/.tla/tla2tools.jar`), 2026-10-19T01:05Z. Exit status 0. Whole run: about 38 s wall.

### TLC results

Green configs report no error. Every mutant reports its named invariant or property violated. Every
witness is violated on its mutant's green constants. For the violated runs, the state count is the
count when TLC stopped at the first counterexample. With several workers it varies a little from run
to run. The verdict does not vary.

| Config | Expected | TLC result | Distinct states | Depth | Match |
|---|---|---|---|---|---|
| `SidecarSupervisor` | no error | no error | 437 | 34 | yes |
| `SidecarSupervisor_ForkBomb` | no error | no error | 24 | 14 | yes |
| `SidecarSupervisor_MutElapsedCredit` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 63 | 14 | yes |
| `SidecarSupervisor_MutStopOff` | violated INV_StopNeverSpawns | violated INV_StopNeverSpawns | 107 | 16 | yes |
| `SidecarSupervisor_MutTimeoutFrees` | violated INV_ProbePermitBounded | violated INV_ProbePermitBounded | 212 | 11 | yes |
| `SidecarSupervisor_Probed` | no error | no error | 9415 | 48 | yes |
| `SidecarSupervisor_ProbedForkBomb` | no error | no error | 108 | 17 | yes |
| `SidecarSupervisor_ReachStopInStopped` | violated W_StopInStopped | violated W_StopInStopped | 102 | 16 | yes |
| `SidecarSupervisor_ReachTimedOutWorker` | violated W_TimedOutWorkerHeldWhileAlive | violated W_TimedOutWorkerHeldWhileAlive | 142 | 13 | yes |
| `SidecarSupervisor_ReachUnattestedTick` | violated W_UnattestedTickPastHealthy | violated W_UnattestedTickPastHealthy | 15 | 10 | yes |
| `AppExitCoordinator` | no error | no error | 64904 | 27 | yes |
| `AppExitCoordinator_MutAdmitWhileClosing` | violated X3_ClosingRejectsAdmission | violated X3_ClosingRejectsAdmission | 54 | 4 | yes |
| `AppExitCoordinator_MutFinalAfterExpiry` | violated X5_ExpiredNeverAuthorizes | violated X5_ExpiredNeverAuthorizes | 23452 | 16 | yes |
| `AppExitCoordinator_MutHermesCompleteEarly` | violated X7_HermesChildrenScope | violated X7_HermesChildrenScope | 292 | 6 | yes |
| `AppExitCoordinator_MutInstallWinNoClosing` | violated X1_EveryExitPathRoutes | violated X1_EveryExitPathRoutes | 136 | 6 | yes |
| `AppExitCoordinator_MutOthersFirst` | violated X9_HermesDrainsFirst | violated X9_HermesDrainsFirst | 149 | 5 | yes |
| `AppExitCoordinator_MutQuitNoActuation` | violated X12_QuitActuatesHeldScopes | violated X12_QuitActuatesHeldScopes | 2690 | 10 | yes |
| `AppExitCoordinator_MutQuitWaitsForever` | violated X11_QuitAlwaysTerminates | violated X11_QuitAlwaysTerminates | 63408 | - | yes |
| `AppExitCoordinator_MutReconnectAfterReport` | violated X7_HermesChildrenScope | violated X7_HermesChildrenScope | 1033 | 9 | yes |
| `AppExitCoordinator_MutTerminateNoDrain` | violated X10_UnpreventableExitDrains | violated X10_UnpreventableExitDrains | 159 | 6 | yes |
| `AppExitCoordinator_ReachExpiredQuit` | violated W_ExpiredQuitExits | violated W_ExpiredQuitExits | 14218 | 14 | yes |
| `AppExitCoordinator_ReachFinalActuation` | violated W_FinalActuationFires | violated W_FinalActuationFires | 2117 | 11 | yes |
| `AppExitCoordinator_ReachHermesComplete` | violated W_HermesCompleteWhileClosing | violated W_HermesCompleteWhileClosing | 610 | 7 | yes |
| `AppExitCoordinator_ReachHermesDraining` | violated W_HermesDrainingChildrenRun | violated W_HermesDrainingChildrenRun | 222 | 5 | yes |
| `AppExitCoordinator_ReachInstallWin` | violated W_InstallWinCompletes | violated W_InstallWinCompletes | 10455 | 13 | yes |
| `AppExitCoordinator_ReachLateCompletion` | violated W_LateCompletionAfterExpiry | violated W_LateCompletionAfterExpiry | 20168 | 16 | yes |
| `AppExitCoordinator_ReachOthersDrain` | violated W_OthersDrainWhileHermesDrains | violated W_OthersDrainWhileHermesDrains | 444 | 6 | yes |
| `AppExitCoordinator_ReachTerminate` | violated W_TerminateExits | violated W_TerminateExits | 7900 | 13 | yes |
| `AppExitCoordinator_ReachTicketWhileDraining` | violated W_TicketOwnedWhileDraining | violated W_TicketOwnedWhileDraining | 262 | 5 | yes |

Checks beyond the table:

- **Action coverage.** `-coverage 1` on `SidecarSupervisor_Probed.cfg`: every action fires, including
  `ProbeNeverEnds`, `ReplaceConfig`, `RequestReplace` and both tick kinds. On `SidecarSupervisor.cfg`
  (LivenessOnly) the probe actions and `GraceTick` never fire, which is what that profile means. On
  `AppExitCoordinator.cfg` every action fires except `UpdaterDirectExit`, which exists only under its
  mutant. `RuntimeAdmit` adds no new states there, because in the green model it re-admits into a child
  set that is already Running. Its window is covered by `ReachHermesComplete` and by its mutant.
- **The counterexamples are the defects.** `MutStopOff` stops the service, completes the stop, then
  sends a second Stop in Stopped, and that Stop admits a ticket. `MutQuitWaitsForever` ends with
  every held owner actuated after expiry and the app stuck in Closing.
- **F-1 negative control (C1.0b) still bites.** In a scratch copy, removing `failures' = 0` from
  `ResetCredit` makes TLC report `INV_NeverFailedWhenRecovering` violated on both
  `SidecarSupervisor.cfg` and `SidecarSupervisor_Probed.cfg`.

### Rule-2 record

| Measure | Before (`b521d98`) | After |
|---|---|---|
| TLC configs for these two modules | 2 (SidecarSupervisor) | 29 (5 green, 12 mutants, 12 witnesses) |
| `SidecarSupervisor` invariants and properties | 5 (INV-1 to INV-4, one liveness) | 18 (INV-1 to INV-15, `StopNeverCounts`, the liveness property, `TypeOK`) |
| `cargo test --workspace --locked`, vitest | unchanged | unchanged: this PR touches no Rust or TypeScript |

The S1.5 baseline (full test counts on the release line) is its own WP and is not recorded here.

### Decisions taken while writing the models (for the lead)

1. **OwnedContainment.tla: needed, as a separate module, and not written in gate0.** In the
   supervisor, teardown is one step (`TeardownChild`), and the `Cleanup` set is what that step must
   empty. The retained-anchor ordering and the Windows Job rule add a sub-protocol inside that step:
   leader unreaped, group TERM, grace, one group KILL, actuation closed, then reap, then
   signal-zero absence; on Windows, `ActiveProcesses == 0`. Folding it in would multiply every
   supervisor state by the containment states, with no invariant that needs both at once. As a
   separate module it refines `TeardownChild` and `Cleanup = {}`, with `NoActuationAfterReap`,
   `AbsenceOnlyFromObservation` and `JobCompleteOnlyAtZero`. It is written with S9.2 (Unix) and
   S10.2 (Windows), which is where 05 already places it (v0.5.2, the "if S1.1 decides it is needed"
   row). Until then, `TeardownChild` is a stated abstraction: the header and the action comment
   both say so.
2. **Mutants use a `Mutant` constant in the module, as 03 asks.** Some earlier specs in this
   directory patch a temporary copy from a Python script instead. Here the configs select the
   mutant. Each mutant config checks only `TypeOK` plus its named invariant, so it cannot "fail" on
   a different one (RT-17).
3. **`cleanup` is a derived state function, not a variable.** `Cleanup` is computed from the child,
   the tickets and the probe workers, so it cannot drift from them. INV-14 is stated over it.
4. **The probe needs a set of workers, not just `probeInc`.** With one permit variable, two
   workers in flight cannot be expressed, so the `TimeoutFrees` mutant would pass vacuously. The
   model keeps the permit (`probe`) and the set of workers actually in flight, each tagged with its
   generation, incarnation and timed-out flag. INV-10 bounds the set. INV-11 ties the permit to it.
5. **Action properties where the statement is about a step.** INV-6, INV-12, INV-14, INV-15 (its
   step half), X-6 and X-9 are `[][A]_vars` properties. TLC reports them as "Action property ... is
   violated", and `tlc_expect.py` accepts that form.
6. **An explicit Start gets a fresh retry budget** (`failures' = 0` in `RequestStart`). SPEC-001
   says a new explicit Start requires complete old teardown. It does not say whether the budget
   resets. The model reads an explicit Start as a new intent, so INV-7 constrains only automatic
   credit. If the owner wants the budget to carry over, the model changes in one line. SCL-S3.2
   should pin whichever rule is chosen with a test.
7. **Quarantine trigger.** `ProbeNeverEnds` fires nondeterministically once a timed-out worker is
   outstanding, or while a Stop waits on any worker. It kills the child, keeps the permit and the
   slot, and does not consume retry budget. The numeric bound is SCL-S4.2's to choose.
8. **AppExitCoordinator fairness covers the coordinator's own steps, not only `Tick`.** X-11 is
   proved under weak fairness of `Tick`, `BeginClosing`, the two drain starts, `Expire`,
   `FinalActuation`, `FinalQuit`, `FinalRestart` and `ReportIncomplete`. Owners, tickets, the Hermes
   runtime and the environment have no fairness. That is how "an owner stalls" is modelled, in place
   of an explicit `OwnerStalls` action. 03 says "under weak fairness of `Tick`"; reading it as
   "and the coordinator itself makes progress" is the assumption S12.1 has to meet in code.
9. **Quit-like set.** X-11 and X-12 are checked for `Quit`, `WindowClose`, `Reset` and `Terminate`.
   This is a superset of the normative `Quit` and `Reset`. A `Quit`-like trigger that arrives while
   a restart or install is closing replaces it (D-15), and `Terminate` replaces any trigger,
   because it cannot be prevented.
10. **Logout and Dock Quit are `Terminate`, not `ForceExit`.** 03 §2 lists logout under
    `ForceExit`. RT-01 shows that they reach the app as an unpreventable `RunEvent::Exit` that can
    still drain synchronously, which is X-10. `ForceExit` is kept for crashes and external kills.
11. **`hermesChildren` is `{Running, Complete}`.** An incomplete child report is carried on the
    Hermes owner state (`Incomplete` after the final actuation). `RuntimeAdmit` can move the
    children back to `Running` only while the runtime gate is open (RT-08).
12. **One mutant beyond 03's list.** `MutOthersFirst` makes X-9 (Hermes first, with the sub-deadline)
    falsifiable too.
13. **The spawn-fail arm is split** into `SpawnFailCurrent` (counts) and `SpawnFailStale` (a late
    failure after Stop or replacement, counts nothing). This keeps INV-3 (`StopNeverCounts`) true
    for late returns.

### Not done here

- S1.3 (harness), S1.4 (re-grounding), S1.5 (baseline) and S1.6 (CI lanes) are other lanes.
- TLC in CI, including the mutants, is S13.4. `tlc_expect.py` is the entry point it can call.
- `g0-formal` stays `met: false` in gates.yaml until this PR is reviewed and merged. The gates file
  links to this section.
