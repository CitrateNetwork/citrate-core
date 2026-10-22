---
created: 2026-10-19T01:10:00Z
branch: beta/k1a-gate0-tla
author: "Larry Klosowski + Claude Opus 5.5 (MAC lane)"
status: active
updated: 2026-10-22 (review round 3: Codex P2 at 59cecef, Probed credit must span healthy_after in time; review round 2: pysol-dev changes requested at 8ea9d53, vacuous MemberStart fixed; review round 1: DGX changes requested at 94d6384; B1, B2 and the (b), (c) conditions recorded below)
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
6. *(Superseded in review round 1, B2: only a member Start resets; see below.)* **An explicit Start gets a fresh retry budget** (`failures' = 0` in `RequestStart`). SPEC-001
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

## Review round 1: DGX changes requested at `94d6384` (S1.1, S1.7)

The DGX lane reproduced all 29 results with the same TLC rev (2.19, OpenJDK 1.8). It asked for two
blocking changes and agreed to decisions (b) and (c) on conditions. Both blocking items are fixed in
the model, not only in text.

### B1: Stopped owns nothing

Nothing tied Stopped to "no probe workers". A `StopComplete` that ignored workers passed every green
config. Added:

- **INV-16 `INV_StoppedOwnsNothing`**: `phase = "Stopped" => Cleanup = {}` (no child, no ticket, no
  probe worker). This is SPEC-001 line 92: Stopped has "no owned resources from the completed
  control". It is in all green configs.
- **Mutant `MutStopIgnoresWorkers`**: `StopComplete` checks only `~child /\ tickets = {}`. It must fail
  on INV-16.
- **Witnesses**: `ReachStopWaitsOnWorker` (green base: a stop has released the child and the tickets
  while a worker still runs, which is where the mutant fires) and `ReachStoppedWithWorker` (on the
  mutant: the cell reaches Stopped with a live worker).

### B2: the retry budget resets only on a member Start

Decision 6 is narrowed. The Start action is split:

- **`MemberStart`**: a person asks (a Start button, a toggle the member flips). New intent, fresh
  budget. It is the only action besides `ResetCredit` that resets `failures`. `RequestStart` is kept
  as an alias for it.
- **`AutoStart`**: no member asked (auto-start at launch, a lazy `ensure_started` from Hermes tools
  or another service). It carries the budget over and is refused while Failed or Quarantined.
- **`AutoRestart`**: an internal stop-then-start of a running service, as one new Run intent.
  Today's callers are `NodeManager::arm_mining` and `disarm_mining`
  (`self.stop(); let _ = self.start();`, `src-tauri/src/node.rs` 511-521 and 599-608). A watchdog
  restart has the same shape. It carries the budget over.
- **INV-17 `INV_CrashBoundBetweenMemberStarts`**: for a never-healthy child, the failures recorded
  since the last member Start stay at most `MaxRetries + 1` (history variable `sinceMember`).
- **New spec `AutoRestartSpec`** and green configs `_ForkBombAutoRestart` (LivenessOnly) and
  `_ProbedForkBombAutoRestart`. These are the fork bomb plus automated restarts and starts, with no
  member Start. Teardown and stop completion are fair. They check INV-17 and that the run still
  reaches Failed.
- **Mutant `MutAutoStartResets`**: the automated paths reset the budget, as every caller of
  `SidecarSupervisor::start` does today. It must violate INV-17. Its counterexample: three
  failures, an `AutoRestart` resets the budget, then two more crashes, so 5 > 4.
- **Witness `ReachAutoRestartAfterFailures`**: an automated restart can fire after failures were
  recorded.

**The crash bound (INV-2, INV-17) holds between member Starts, not over the app's lifetime.** A
member Start is a new budget by decision. No automated caller can give one. SCL-S3.2 implements
this split and S6.1 migrates the node's mining arm and disarm paths. Until then, today's code resets
on every `start()`, which is exactly the mutant.

### Conditions recorded on decisions (b) and (c)

- **(b), X-11 fairness.** Quit always ending is only as strong as the coordinator thread and the
  deadline publisher. The module header and the X-11 comment now say so. Conditions for S12.1 and
  S12.6:
  - the coordinator path never takes an owner or lifecycle lock, never joins, and never blocks on an
    owner;
  - `FinalActuation` only signals and never waits on a reap;
  - a test has an owner hold its lock forever and checks that Quit still exits by the deadline;
  - in the synchronous Terminate mode (inside `applicationWillTerminate`) the main thread is free of
    webview and IPC waits.
- **(c), logout and Dock Quit as Terminate.** RT-01 holds. Limits:
  - sudden termination (if `NSSupportsSuddenTermination` is ever enabled) is a straight kill;
  - at logout or shutdown, launchd may send SIGTERM then SIGKILL, and core installs no SIGTERM
    handler, so SIGTERM is ForceExit;
  - a 15 s main-thread drain during logout may outlast what loginwindow tolerates, which S12.6 must
    measure natively.
- **S12 requirement (not implemented here): a "close in progress" marker written before the drain
  starts.** In the model `ForceExit` sets `reported = Incomplete` in one step. A killed process
  writes nothing, so the next launch can only report Incomplete if the coordinator has already
  persisted a marker at `BeginClosing`, before any drain work. The marker is cleared only by a
  Complete exit. The module header notes this.

### Non-blocking review items

| # | Item | Disposition |
|---|---|---|
| 1 | INV-5's `~ENABLED AdmitSpawn` half is true by construction | Done: the comment says the teeth are in the ticket clause |
| 2 | INV-12 is true by construction; the real defect is ProbeOk accepting an old tag | Done: mutant `MutStaleSuccess` (fails on INV-13) with witness `ReachStaleWorker` |
| 4 | Failed with a timed-out worker never shows Quarantined | Done: `ProbeNeverEnds` includes Failed |
| 7 | RuntimeAdmit after the app gate and before RuntimeClosing | Done: the comment names the window as by design |
| 3 | No mixed case (some successes, then a crash before healthy_after) in the recovering configs | Open, for S3.2's re-run: add a partly healthy, early-crashing config |
| 5 | No single-cell liveness that a Stop ends in Stopped or Quarantined | Open, for S2.4's re-run |
| 6 | No owner-reported Incomplete in the coordinator, so Quit waits out the deadline | Open, for S12.1 (`OwnerIncomplete(o)`) |
| 8 | After an expired Restart or Install the only ways out are retry or Quit | S12.2 UX must say so |
| 9 | O-12 and 02 line 283 say logout is never Complete; X-10 allows Complete | Open: annotate O-12 in a planset docs pass |
| 10 | Reset exits at the deadline with owners Incomplete; a later wipe could race a live sidecar | S12.7: the wipe requires Complete or runs at next launch after cleanup |
| 11 | Pre-push hook skipped | Same reason as before (no PyYAML). Same manual gates run, for this PR only |

### TLC results after round 1

Same machine and toolchain as above, 2026-10-19. `tlc_expect.py` exit 0. It picks up every
`<Spec>_*.cfg`, so the new configs run without a change to the script. All 38 configs match: 7
green, 15 mutants, 16 witnesses (corrected in round 2; this line first said 16 mutants and 15 witnesses).

On `SidecarSupervisor.cfg` the distinct states go from 437 to 858, and on `_Probed` from 9415 to
10164. The cause is the new `sinceMember` history and the automated start actions. AppExitCoordinator
only had comments changed, and its green count is unchanged at 64904.

| Config | Expected | TLC result | Distinct states | Depth | Match |
|---|---|---|---|---|---|
| `SidecarSupervisor` | no error | no error | 858 | 35 | yes |
| `SidecarSupervisor_ForkBomb` | no error | no error | 24 | 14 | yes |
| `SidecarSupervisor_ForkBombAutoRestart` | no error | no error | 188 | 22 | yes |
| `SidecarSupervisor_MutAutoStartResets` | violated INV_CrashBoundBetweenMemberStarts | violated INV_CrashBoundBetweenMemberStarts | 413 | 20 | yes |
| `SidecarSupervisor_MutElapsedCredit` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 40 | 10 | yes |
| `SidecarSupervisor_MutStaleSuccess` | violated INV_ReadyIsCurrent | violated INV_ReadyIsCurrent | 2110 | 20 | yes |
| `SidecarSupervisor_MutStopIgnoresWorkers` | violated INV_StoppedOwnsNothing | violated INV_StoppedOwnsNothing | 286 | 12 | yes |
| `SidecarSupervisor_MutStopOff` | violated INV_StopNeverSpawns | violated INV_StopNeverSpawns | 158 | 13 | yes |
| `SidecarSupervisor_MutTimeoutFrees` | violated INV_ProbePermitBounded | violated INV_ProbePermitBounded | 236 | 10 | yes |
| `SidecarSupervisor_Probed` | no error | no error | 10164 | 50 | yes |
| `SidecarSupervisor_ProbedForkBomb` | no error | no error | 108 | 16 | yes |
| `SidecarSupervisor_ProbedForkBombAutoRestart` | no error | no error | 783 | 25 | yes |
| `SidecarSupervisor_ReachAutoRestartAfterFailures` | violated W_AutoRestartAfterFailures | violated W_AutoRestartAfterFailures | 98 | 24 | yes |
| `SidecarSupervisor_ReachStaleWorker` | violated W_StaleWorkerWhileAlive | violated W_StaleWorkerWhileAlive | 184 | 11 | yes |
| `SidecarSupervisor_ReachStopInStopped` | violated W_StopInStopped | violated W_StopInStopped | 98 | 12 | yes |
| `SidecarSupervisor_ReachStopWaitsOnWorker` | violated W_StopWaitsOnWorker | violated W_StopWaitsOnWorker | 196 | 10 | yes |
| `SidecarSupervisor_ReachStoppedWithWorker` | violated W_StoppedWithWorker | violated W_StoppedWithWorker | 342 | 12 | yes |
| `SidecarSupervisor_ReachTimedOutWorker` | violated W_TimedOutWorkerHeldWhileAlive | violated W_TimedOutWorkerHeldWhileAlive | 175 | 12 | yes |
| `SidecarSupervisor_ReachUnattestedTick` | violated W_UnattestedTickPastHealthy | violated W_UnattestedTickPastHealthy | 15 | 10 | yes |
| `AppExitCoordinator` | no error | no error | 64904 | 27 | yes |
| `AppExitCoordinator_MutAdmitWhileClosing` | violated X3_ClosingRejectsAdmission | violated X3_ClosingRejectsAdmission | 62 | 4 | yes |
| `AppExitCoordinator_MutFinalAfterExpiry` | violated X5_ExpiredNeverAuthorizes | violated X5_ExpiredNeverAuthorizes | 23459 | 16 | yes |
| `AppExitCoordinator_MutHermesCompleteEarly` | violated X7_HermesChildrenScope | violated X7_HermesChildrenScope | 284 | 6 | yes |
| `AppExitCoordinator_MutInstallWinNoClosing` | violated X1_EveryExitPathRoutes | violated X1_EveryExitPathRoutes | 120 | 6 | yes |
| `AppExitCoordinator_MutOthersFirst` | violated X9_HermesDrainsFirst | violated X9_HermesDrainsFirst | 118 | 6 | yes |
| `AppExitCoordinator_MutQuitNoActuation` | violated X12_QuitActuatesHeldScopes | violated X12_QuitActuatesHeldScopes | 3399 | 11 | yes |
| `AppExitCoordinator_MutQuitWaitsForever` | violated X11_QuitAlwaysTerminates | violated X11_QuitAlwaysTerminates | 43120 | - | yes |
| `AppExitCoordinator_MutReconnectAfterReport` | violated X7_HermesChildrenScope | violated X7_HermesChildrenScope | 1090 | 8 | yes |
| `AppExitCoordinator_MutTerminateNoDrain` | violated X10_UnpreventableExitDrains | violated X10_UnpreventableExitDrains | 64 | 5 | yes |
| `AppExitCoordinator_ReachExpiredQuit` | violated W_ExpiredQuitExits | violated W_ExpiredQuitExits | 12013 | 14 | yes |
| `AppExitCoordinator_ReachFinalActuation` | violated W_FinalActuationFires | violated W_FinalActuationFires | 3686 | 11 | yes |
| `AppExitCoordinator_ReachHermesComplete` | violated W_HermesCompleteWhileClosing | violated W_HermesCompleteWhileClosing | 522 | 7 | yes |
| `AppExitCoordinator_ReachHermesDraining` | violated W_HermesDrainingChildrenRun | violated W_HermesDrainingChildrenRun | 324 | 5 | yes |
| `AppExitCoordinator_ReachInstallWin` | violated W_InstallWinCompletes | violated W_InstallWinCompletes | 8851 | 13 | yes |
| `AppExitCoordinator_ReachLateCompletion` | violated W_LateCompletionAfterExpiry | violated W_LateCompletionAfterExpiry | 24344 | 16 | yes |
| `AppExitCoordinator_ReachOthersDrain` | violated W_OthersDrainWhileHermesDrains | violated W_OthersDrainWhileHermesDrains | 497 | 6 | yes |
| `AppExitCoordinator_ReachTerminate` | violated W_TerminateExits | violated W_TerminateExits | 8400 | 12 | yes |
| `AppExitCoordinator_ReachTicketWhileDraining` | violated W_TicketOwnedWhileDraining | violated W_TicketOwnedWhileDraining | 352 | 6 | yes |

Rule-2 record for round 1: TLC configs for these modules go from 29 to 38, and `SidecarSupervisor`
properties from 18 to 20 (INV-16, INV-17). There is still no Rust or TypeScript change.

## Review round 2: pysol-dev changes requested at `8ea9d53` (S1.1)

pysol-dev re-ran the suite on Linux with OpenJDK 21 and the TLC nightly 2026.10.15. Every green
count matched. The review found one modelling bug, one script gap and some stale counts.

### Blocking: `MemberStart` was vacuous after any failure

`MemberStart` set `sinceMember' = 0` and also listed `sinceMember` in its `UNCHANGED`. The
conjunction holds only when `sinceMember = 0`, so a member Start never fired from Failed, or after
any crash or spawn failure under the current intent. That left part of INV-15
(`INV_QuarantineNotRecycled`) and the B2 reading of INV-17 vacuous. Fix: `sinceMember` is
dropped from that `UNCHANGED`.

Red first. Commit `085f1af` adds the tests below; they fail against the old model:

| Test | At `085f1af` (old model) | After the fix |
|---|---|---|
| `SidecarSupervisor_ReachMemberStartFromFailed` (`W_MemberStartFromFailed`, never-healthy constants, full `Spec`) | **no error** (487 states): mismatch | violated (556) |
| `SidecarSupervisor_ReachMemberStartAfterFailures` (`W_MemberStartAfterFailures`) | **no error** (487): mismatch | violated (255) |
| `tla_frame_lint.py SidecarSupervisor` | `MemberStart: sinceMember is assigned and kept UNCHANGED by a sibling conjunct` | clean |

Revert check: with the fixed model restored the three tests pass. With the old `UNCHANGED` line put
back, all three fail again.

The fix opens the part of the state space the old model never explored. `SidecarSupervisor.cfg`
goes from 858 to 1002 distinct states and `_Probed` from 10164 to 16569, which is what pysol-dev
measured.

### Tripwires for the bug class

- **Frame lint** (`src-tauri/formal/tla_frame_lint.py`, run by `tlc_expect.py` before TLC). It flags
  any variable that one conjunct of an action assigns, directly or through a helper such as
  `FailStep`, while a sibling conjunct keeps it `UNCHANGED`. It respects IF arms and disjuncts and
  expands tuple aliases. It has eight self-tests (`--self-test`). On this tree it reports exactly
  the one finding above, and none in the other thirteen modules in `src-tauri/formal`.
- **Action coverage.** Green configs run with `-coverage`. Every named action TLC reports must be
  taken in at least one green config, unless the module marks it `\* COVERAGE-EXEMPT:`. The only
  exemption is `UpdaterDirectExit`, which exists only under its mutant. This catches an action
  that is wholly vacuous. The frame lint catches the partly vacuous case, which coverage alone
  missed here: `MemberStart` still fired from Stopped.
- **Action reachability witnesses.** `W_MemberStartFromFailed` and `W_MemberStartAfterFailures` are
  permanent configs.

### Should fix: temporal verdict wording (`tlc_expect.py`)

TLC 2.19 prints "Temporal properties were violated". Current nightlies print "Temporal property X
was violated". The classifier now matches the named form and checks the name against `EXPECT`. The
unnamed form is still accepted only when the config has a single `PROPERTY`. Seven classifier
self-tests run with `--self-test`, and the nightly form was red before the fix.

The script prints the TLC version and the jar's sha256. When `TLA_JAR_SHA256` is set, it refuses
any other jar. The jar used here is `TLC2 2.19 (rev 5a47802)`, sha256 `936a2620…050e88`. There is
no TLC CI job yet: S13.4 creates it and pins the jar with that variable.

### Counts

The round-1 line in this file had mutants and witnesses swapped; it is corrected above. The PR body
now says 40. The README no longer says "one witness per mutant", since `StopIgnoresWorkers` has two
and the two member-Start witnesses belong to no mutant.

### TLC results after round 2

TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802); jar sha256 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88. `tlc_expect.py` exit 0, frame lint clean, coverage check clean. All 40 configs match: 7
green, 15 mutants, 18 witnesses.

| Config | Expected | TLC result | Distinct states | Depth | Match |
|---|---|---|---|---|---|
| `SidecarSupervisor` | no error | no error | 1002 | 34 | yes |
| `SidecarSupervisor_ForkBomb` | no error | no error | 24 | 15 | yes |
| `SidecarSupervisor_ForkBombAutoRestart` | no error | no error | 188 | 22 | yes |
| `SidecarSupervisor_MutAutoStartResets` | violated INV_CrashBoundBetweenMemberStarts | violated INV_CrashBoundBetweenMemberStarts | 407 | 20 | yes |
| `SidecarSupervisor_MutElapsedCredit` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 64 | 14 | yes |
| `SidecarSupervisor_MutStaleSuccess` | violated INV_ReadyIsCurrent | violated INV_ReadyIsCurrent | 1988 | 22 | yes |
| `SidecarSupervisor_MutStopIgnoresWorkers` | violated INV_StoppedOwnsNothing | violated INV_StoppedOwnsNothing | 405 | 12 | yes |
| `SidecarSupervisor_MutStopOff` | violated INV_StopNeverSpawns | violated INV_StopNeverSpawns | 121 | 12 | yes |
| `SidecarSupervisor_MutTimeoutFrees` | violated INV_ProbePermitBounded | violated INV_ProbePermitBounded | 230 | 12 | yes |
| `SidecarSupervisor_Probed` | no error | no error | 16569 | 54 | yes |
| `SidecarSupervisor_ProbedForkBomb` | no error | no error | 108 | 17 | yes |
| `SidecarSupervisor_ProbedForkBombAutoRestart` | no error | no error | 783 | 24 | yes |
| `SidecarSupervisor_ReachAutoRestartAfterFailures` | violated W_AutoRestartAfterFailures | violated W_AutoRestartAfterFailures | 96 | 24 | yes |
| `SidecarSupervisor_ReachMemberStartAfterFailures` | violated W_MemberStartAfterFailures | violated W_MemberStartAfterFailures | 255 | 14 | yes |
| `SidecarSupervisor_ReachMemberStartFromFailed` | violated W_MemberStartFromFailed | violated W_MemberStartFromFailed | 556 | 20 | yes |
| `SidecarSupervisor_ReachStaleWorker` | violated W_StaleWorkerWhileAlive | violated W_StaleWorkerWhileAlive | 164 | 10 | yes |
| `SidecarSupervisor_ReachStopInStopped` | violated W_StopInStopped | violated W_StopInStopped | 107 | 10 | yes |
| `SidecarSupervisor_ReachStopWaitsOnWorker` | violated W_StopWaitsOnWorker | violated W_StopWaitsOnWorker | 262 | 13 | yes |
| `SidecarSupervisor_ReachStoppedWithWorker` | violated W_StoppedWithWorker | violated W_StoppedWithWorker | 545 | 15 | yes |
| `SidecarSupervisor_ReachTimedOutWorker` | violated W_TimedOutWorkerHeldWhileAlive | violated W_TimedOutWorkerHeldWhileAlive | 176 | 11 | yes |
| `SidecarSupervisor_ReachUnattestedTick` | violated W_UnattestedTickPastHealthy | violated W_UnattestedTickPastHealthy | 15 | 11 | yes |
| `AppExitCoordinator` | no error | no error | 64904 | 27 | yes |
| `AppExitCoordinator_MutAdmitWhileClosing` | violated X3_ClosingRejectsAdmission | violated X3_ClosingRejectsAdmission | 54 | 4 | yes |
| `AppExitCoordinator_MutFinalAfterExpiry` | violated X5_ExpiredNeverAuthorizes | violated X5_ExpiredNeverAuthorizes | 22838 | 16 | yes |
| `AppExitCoordinator_MutHermesCompleteEarly` | violated X7_HermesChildrenScope | violated X7_HermesChildrenScope | 280 | 6 | yes |
| `AppExitCoordinator_MutInstallWinNoClosing` | violated X1_EveryExitPathRoutes | violated X1_EveryExitPathRoutes | 199 | 6 | yes |
| `AppExitCoordinator_MutOthersFirst` | violated X9_HermesDrainsFirst | violated X9_HermesDrainsFirst | 90 | 5 | yes |
| `AppExitCoordinator_MutQuitNoActuation` | violated X12_QuitActuatesHeldScopes | violated X12_QuitActuatesHeldScopes | 2805 | 11 | yes |
| `AppExitCoordinator_MutQuitWaitsForever` | violated X11_QuitAlwaysTerminates | violated X11_QuitAlwaysTerminates | 63408 | - | yes |
| `AppExitCoordinator_MutReconnectAfterReport` | violated X7_HermesChildrenScope | violated X7_HermesChildrenScope | 856 | 9 | yes |
| `AppExitCoordinator_MutTerminateNoDrain` | violated X10_UnpreventableExitDrains | violated X10_UnpreventableExitDrains | 90 | 6 | yes |
| `AppExitCoordinator_ReachExpiredQuit` | violated W_ExpiredQuitExits | violated W_ExpiredQuitExits | 11407 | 14 | yes |
| `AppExitCoordinator_ReachFinalActuation` | violated W_FinalActuationFires | violated W_FinalActuationFires | 3298 | 10 | yes |
| `AppExitCoordinator_ReachHermesComplete` | violated W_HermesCompleteWhileClosing | violated W_HermesCompleteWhileClosing | 558 | 7 | yes |
| `AppExitCoordinator_ReachHermesDraining` | violated W_HermesDrainingChildrenRun | violated W_HermesDrainingChildrenRun | 267 | 6 | yes |
| `AppExitCoordinator_ReachInstallWin` | violated W_InstallWinCompletes | violated W_InstallWinCompletes | 9968 | 13 | yes |
| `AppExitCoordinator_ReachLateCompletion` | violated W_LateCompletionAfterExpiry | violated W_LateCompletionAfterExpiry | 23975 | 16 | yes |
| `AppExitCoordinator_ReachOthersDrain` | violated W_OthersDrainWhileHermesDrains | violated W_OthersDrainWhileHermesDrains | 396 | 6 | yes |
| `AppExitCoordinator_ReachTerminate` | violated W_TerminateExits | violated W_TerminateExits | 5491 | 12 | yes |
| `AppExitCoordinator_ReachTicketWhileDraining` | violated W_TicketOwnedWhileDraining | violated W_TicketOwnedWhileDraining | 355 | 6 | yes |

Rule-2 record for round 2: TLC configs go from 38 to 40. There are 16 self-tests for the scripts
(new). There is still no Rust or TypeScript change.

## Review round 3: Codex P2 at `59cecef` (S1.1)

Finding: `ProbeOk` added one to `okWindow` per result and left `age` unchanged, so with the Probed
constants `SpawnReturns` then two `ProbeBegin`/`ProbeOk` pairs enabled `ResetCredit` at age 0.
SPEC-001 line 80 asks for successful samples that span healthy_after. INV-7 checked the same sample
counter, so it accepted that trace.

- **Model.** New variables `okFirst` and `okLast` (age of the first and latest success of the
  current uninterrupted window, reset wherever the window ends). `OkSpan == okLast - okFirst`.
  Probed `ResetCredit` needs `okWindow > 0 /\ OkSpan >= HealthyAfterTicks`. `HealthyAfterTicks`
  now means ticks for both profiles.
- **INV-7 restated.** `credited => okWindow > 0 /\ OkSpan >= HealthyAfterTicks`: the invariant
  checks the time span, not the guard's sample count.
- **Negative control.** Mutant `SampleCredit` (the 59cecef rule) on the Probed constants: TLC
  reports `INV_CreditOnlyFromAttestedHealth` violated. Witness `W_SamplesWithoutSpan`: an
  uncredited window holds healthy_after samples spanning less than healthy_after ticks.
- **Positive control.** `W_CreditFromSpan` on the green Probed config: credit is reached, so the
  span rule can be met within `MaxAge` and INV-7 is not vacuous.
- **Red first.** 08f7c79 adds the tracking, the restated INV-7 and both witnesses with the old
  guard. TLC then reports `INV_CreditOnlyFromAttestedHealth` violated on `SidecarSupervisor_Probed`.
  The fix commit changes the guard. Revert check: with the old guard put back, `_Probed` fails on
  INV-7 again and passes once the fix is restored.

| Config | Expected | TLC result | Distinct states | Depth | Match |
|---|---|---|---|---|---|
| `SidecarSupervisor_Probed` | no error | no error | 18135 | - | yes |
| `SidecarSupervisor_MutSampleCredit` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 387 | 14 | yes |
| `SidecarSupervisor_ReachSamplesWithoutSpan` | violated W_SamplesWithoutSpan | violated W_SamplesWithoutSpan | 217 | 11 | yes |
| `SidecarSupervisor_ReachCreditFromSpan` | violated W_CreditFromSpan | violated W_CreditFromSpan | 694 | 15 | yes |

`tlc_expect.py SidecarSupervisor` exit 0 (TLC 2.19, same jar digest as round 2), frame lint clean,
coverage check clean, `--self-test` passes. All 24 SidecarSupervisor configs match; the
AppExitCoordinator configs are unchanged. Rule-2 record for round 3: TLC configs go from 40 to 43
(7 green, 16 mutants, 20 witnesses). Still no Rust or TypeScript change.

### Not done here

- S1.3 (harness), S1.4 (re-grounding), S1.5 (baseline) and S1.6 (CI lanes) are other lanes.
- TLC in CI, including the mutants, is S13.4. `tlc_expect.py` is the entry point it can call.
- `g0-formal` stays `met: false` in gates.yaml until this PR is reviewed and merged. The gates file
  links to this section.
