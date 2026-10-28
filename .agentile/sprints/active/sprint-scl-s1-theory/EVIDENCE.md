---
created: 2026-10-19T01:10:00Z
branch: beta/k1a-gate0-tla
author: "Larry Klosowski + Claude Opus 5.5 (MAC lane)"
status: active
updated: 2026-10-22 (review round 5: Codex P2 at ebf978c, the exit deadline ticks independently of drain dispatch; review round 4: DGX changes requested at 5d1e255, INV-7 over an observer; review round 3: Codex P2 at 59cecef, Probed credit must span healthy_after in time; review round 2: pysol-dev changes requested at 8ea9d53, vacuous MemberStart fixed; review round 1: DGX changes requested at 94d6384; B1, B2 and the (b), (c) conditions recorded below)
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

## Review round 4: DGX changes requested at `5d1e255` (S1.1)

**B1. INV-7 restated the guard.** Round 3 wrote INV-7 over `okFirst`/`okLast`, the same values the
`ResetCredit` guard reads, so a defect in that bookkeeping passed every invariant. DGX showed two
one-line mutants that did.

- **Observer.** New variable `okAges`, the set of ages of the successes in the current
  uninterrupted window. `ProbeOk` adds the age. It is cleared wherever the window really ends:
  `ResetWindow`, a current spawn, a stale tick, and a grace failure or timeout. No action's guard
  reads it, and no mutant changes it.
- **INV-7.** `credited => okAges # {} /\ ObservedSpan >= HealthyAfterTicks`, with
  `ObservedSpan == Max(okAges) - Min(okAges)`.
- **Mutant M1 `FirstNotRecorded`** (DGX's M1 as written): `ProbeOk` never stamps the first
  success. TLC reports INV-7 violated (326 states). Witness `ReachFirstSuccessLate`: a first
  success can arrive after age 0 (151).
- **Mutant M2 `StaleKeepsWindow`** (DGX's M2, strengthened): a stale tick keeps `okWindow`,
  `okFirst` and `okLast`. TLC reports INV-7 violated (1050). Witness `ReachStaleWithOpenWindow`
  (295).
  - Why strengthened: DGX's literal M2 keeps only `okFirst`/`okLast`. That is an equivalent
    mutant. The stale tick still zeroes `okWindow`, `ProbeOk` re-stamps `okFirst` whenever
    `okWindow = 0`, and `ResetCredit` needs `okWindow > 0`. So a kept `okFirst`/`okLast` is never
    read before it is overwritten.
  - Checked: the literal M2 on the head reports no error on `_Probed` (23037 states), as it must.
    Keeping the whole window is the version that changes behaviour, and the observer catches it.
- **Red first.** 6947b99 adds both mutants and witnesses against the round-3 INV-7. `tlc_expect`
  reports MISMATCH for both (no error, 15269 and 18905 states).
- **Revert check.** With INV-7 put back to the guard expression, both mutants report no error
  (25069, 24919). With the fix, both are violated. DGX's literal M1 on the head: INV-7 violated on
  `_Probed`.
- **Non-blocking 1.** A success is stamped at the age its result is handled (completion), not at
  `ProbeBegin`. The `ProbeOk` comment records this, and S3.2 must use the same instant.

**B2. Committed bytecode.** `src-tauri/formal/__pycache__/tla_frame_lint.cpython-313.pyc` came in
with 08f7c79. It is now untracked. `__pycache__/` and `*.pyc` are in `.gitignore`. `tlc_expect.py`
sets `sys.dont_write_bytecode`, and a full run leaves no `__pycache__`. Rewriting history to drop
it from 08f7c79 needs the owner's OK for a force-push (Rule 10), so it is not done here.

### TLC results after round 4

TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802), jar sha256
936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88.
`tlc_expect.py SidecarSupervisor AppExitCoordinator` exit 0. Frame lint, coverage check and
`--self-test` are clean. All 47 configs match: 7 green, 18 mutants, 22 witnesses.

Green distinct states:

| Config | Round 3 | Round 4 |
|---|---|---|
| `SidecarSupervisor` | 1002 | 1002 |
| `SidecarSupervisor_ForkBomb` | 24 | 24 |
| `SidecarSupervisor_ForkBombAutoRestart` | 188 | 188 |
| `SidecarSupervisor_Probed` | 18135 | 22729 |
| `SidecarSupervisor_ProbedForkBomb` | 108 | 108 |
| `SidecarSupervisor_ProbedForkBombAutoRestart` | 783 | 783 |
| `AppExitCoordinator` | 64904 | 64904 |

`_Probed` grew because of `okAges`.

| Config | Expected | TLC result | Distinct states | Depth | Match |
|---|---|---|---|---|---|
| `SidecarSupervisor_MutFirstNotRecorded` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 326 | 13 | yes |
| `SidecarSupervisor_MutStaleKeepsWindow` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 1050 | 15 | yes |
| `SidecarSupervisor_ReachFirstSuccessLate` | violated W_FirstSuccessLate | violated W_FirstSuccessLate | 151 | 10 | yes |
| `SidecarSupervisor_ReachStaleWithOpenWindow` | violated W_StaleWithOpenWindow | violated W_StaleWithOpenWindow | 295 | 11 | yes |
| `SidecarSupervisor_MutSampleCredit` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 385 | 13 | yes |
| `SidecarSupervisor_MutElapsedCredit` | violated INV_CreditOnlyFromAttestedHealth | violated INV_CreditOnlyFromAttestedHealth | 69 | 13 | yes |

Rule-2 record for round 4: TLC configs go from 43 to 47. Still no Rust or TypeScript change.

## Review round 5: Codex P2 at `ebf978c` (S1.2)

**Finding.** In `AppExitCoordinator.tla`, `Tick` was enabled only after Hermes had been told to
drain, and stopped again at the sub-deadline until the other owners had been. So the absolute
deadline depended on coordinator progress. A drain dispatch delayed past the real deadline froze
`deadlineLeft`. Owners could then complete, and `FinalRestart` could authorize a restart or
installer with time apparently left. Weak fairness makes the dispatch happen eventually, but sets
no bound on the delay, so the green checks never explored this race. SPEC-001 arms the deadline at
admission and has its own publisher thread, which never waits on drain progress.

- **Model.** `Tick` needs only `app = "Closing" /\ deadlineLeft > 0`. The old dispatch gate is
  kept as mutant `TickWaitsOnDispatch`.
- **X-13 `X13_DeadlineIndependentOfDispatch`.** While Closing with time left, `Tick` is enabled.
  It is in the green config.
- **Mutant.** `MutTickWaitsOnDispatch` reports X-13 violated (62 states). Witness
  `ReachClosingBeforeDispatch`: Closing while Hermes is not yet draining (42).
- **The race.** Witness `ReachExpiredBeforeDispatch`: the deadline expires before Hermes is told
  to drain (848). On the green config the race is reachable, and every other property holds
  through it, unchanged:
  - X-5: no restart or install after expiry;
  - X-9: drain order, since the sub-deadline has passed;
  - X-11 and X-12: Quit ends with actuation and an Incomplete report;
  - L-1.
  The drain-order and termination properties needed no restatement.
- **Red first.** 56d22ed adds X-13, the mutant and both witnesses on the old `Tick`. TLC reports
  X-13 violated on the green `AppExitCoordinator` (43 states), and `ReachExpiredBeforeDispatch`
  reports no error, which is a mismatch.
- **Revert check.** The fixed model was copied aside and the red-commit model restored. The green
  config again fails on X-13, and the race witness again reports no error. With the fix put back,
  both match.

### TLC results after round 5

TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802), jar sha256
936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88.
`tlc_expect.py SidecarSupervisor AppExitCoordinator` exit 0. Frame lint, coverage check and
`--self-test` are clean. All 50 configs match: 7 green, 19 mutants, 24 witnesses.

Green `AppExitCoordinator` goes from 64904 to 104896 distinct states: ticks now interleave with
dispatch. The SidecarSupervisor green counts are unchanged from round 4.

| Config | Expected | TLC result | Distinct states | Depth | Match |
|---|---|---|---|---|---|
| `AppExitCoordinator` | no error | no error | 104896 | 26 | yes |
| `AppExitCoordinator_MutTickWaitsOnDispatch` | violated X13_DeadlineIndependentOfDispatch | violated X13_DeadlineIndependentOfDispatch | 62 | 4 | yes |
| `AppExitCoordinator_ReachClosingBeforeDispatch` | violated W_ClosingBeforeDispatch | violated W_ClosingBeforeDispatch | 42 | 4 | yes |
| `AppExitCoordinator_ReachExpiredBeforeDispatch` | violated W_ExpiredBeforeDispatch | violated W_ExpiredBeforeDispatch | 848 | 7 | yes |

Rule-2 record for round 5: TLC configs go from 47 to 50. Still no Rust or TypeScript change.

### Not done here

- S1.3 (harness), S1.4 (re-grounding), S1.5 (baseline) and S1.6 (CI lanes) are other lanes.
- TLC in CI, including the mutants, is S13.4. `tlc_expect.py` is the entry point it can call.
- `g0-formal` stays `met: false` in gates.yaml until this PR is reviewed and merged. The gates file
  links to this section.

## SCL-S13.4: TLC in CI (branch ci/k1a-tla-check, stacked on #345)

- Workflow `.github/workflows/tla-models.yml`: `tlc_expect.py --self-test`, the frame lint over all
  15 modules, then `tlc_expect.py --all` on tla2tools.jar v1.7.4 (`TLC2 Version 2.19 of 08 August
  2024`, rev 5a47802), sha256 `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`,
  checked by `sha256sum -c` at download and by `TLA_JAR_SHA256`. Java 21 (temurin).
- Red 0c67674 (four discovery/plan self-tests fail on assertion), fix ff299bd. Before the fix,
  `tlc_expect.py NoSuchSpec` exited 0 having checked nothing; now it exits 1 before TLC runs.
- Local run of the CI commands with the pinned jar (2026-10-23, host load ~128): 50/50 configs
  match, exit 0, 230 s. Red runs on a scratch copy: forcing `OthersFirst` on in the green
  AppExitCoordinator gives MISMATCH (violated X9_HermesDrainsFirst) plus 11 coverage findings,
  exit 1; an injected `sinceMember' = 0` beside `UNCHANGED sinceMember` in AutoStart fails the
  frame lint, exit 1; a wrong `TLA_JAR_SHA256` exits 2; a SidecarSupervisor config with no EXPECT
  line fails the plan, exit 1.
- The CI log on the PR head is the S13.4 close evidence ("CI log", 05_SPRINTS_AND_WPS.md).

## Harness and baseline

Sections for S1.3 (harness), S1.5 (baseline) and the SC1 status. The S1.4 re-grounding is in
[REGROUND.md](REGROUND.md).

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

### Numbers at merge time (2026-10-25)

The counts above are recorded at the SHAs they name and are kept as recorded. Later heads:

| PR | Now | `lifecycle_faults` | kit `supervisor::tests` (Unix / Windows) |
|---|---|---|---|
| #342 (S1.3 harness) | merged as `05462f5` | 11 (recorded above at `4b69bd8`: 10) | unchanged |
| #351 (SC1 S2.1, S2.2) | merged as `e10e57c` | 23 | 35 / 13 |
| #412, #414 (core#398, core#397) | merged (`1fc604e`, `b0a056f`) | 28 on `main` | 37 / 15 on `main` |
| #352 (SC1 S2.3, S2.4) | open, head `d6b6ef2` (main merged in) | 55 | 49 / 27 |

The current numbers for an open PR, with the run that produced them, are in its latest
`[MAC] Addressed` comment. Workspace totals are not refreshed here: the Rule 2 comparison is
against the S1.5 baseline table above.

The open item "retries within one Start intent do not wait for the previous incarnation's
readers" is closed by #414 (core#397): the reader reconcile now runs before every spawn,
automatic retries included.

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

### SCL-S3 review round 2 (rebase onto #352 `ccb9ed3`)

- **Rebase.** #352 moved to `ccb9ed3`: `enter_failed` now marks the service Failed and settles
  the failing Start under one lock, and a new `failed_barrier` test hook was added.
  - The only conflict was the fault suite's `use` list; both sides' imports are kept.
  - `supervisor.rs` merged cleanly. SC2's spent-budget path calls the new single-lock
    `enter_failed`.
- **Counts:**

  | Measure | Result |
  |---|---|
  | `cargo test --workspace --locked` | 2512 passed, 0 failed, 20 ignored |
  | kit `supervisor::tests` | 47 Unix (25 Windows, checked by `lifecycle-platforms.yml`) |
  | `lifecycle_faults` | 55 (#352's 44 plus 11 from SCL-S3) |

- **Repeated runs.** 90 runs (18 concurrent copies × 5 rounds) in a fresh target dir, at a host
  load average of 33 to 79: 90/90 green, and no `citrate-lcf-` process left.
  `start_after_health_failures_with_a_held_reader_waits_for_cleanup` did not flake, so its
  probe interval is unchanged.

### SCL-S3 review round 3 (2026-10-22)

- **Red:** `ebc2b20`. **Fix:** the commit after it. Merges #352 at `964aa51`, which brings #351
  and #342 (the reaped check) and `origin/main`.
- **Probe timing (Codex P1).** `Window::record_success` takes the worker's own finish time. A
  success older than the freshness limit when it is collected is `Stale`; a success after a gap
  starts a new window. Red: `a_success_collected_after_the_freshness_limit_is_stale`,
  `a_success_is_timed_when_the_probe_finished`, `a_success_after_a_freshness_gap_starts_a_new_window`.
- **Member Start joining an automated Start (pysol-dev 1).** It promotes the pending intent, and
  `enter_failed` refuses to settle Failed when a member Start arrived after the monitor read the
  intent. Red: `a_member_start_joining_an_automated_start_gives_a_fresh_budget` (held at the new
  `budget_barrier` seam).
- **Coverage (pysol-dev 2 and 3).** `successes_spanning_healthy_after_earn_one_credit`,
  `a_flapping_probe_never_earns_credit` and `a_hung_probe_slot_counts_as_a_failure_outside_the_grace`.
  They kill the mutants "credit on the first success" and "slot-taken arm disabled"; the
  `enter_failed` check mutant is killed by the member-join fixture.
- **Counts:** kit lib 373 -> 377; `supervisor::tests` 50 -> 54 Unix (lifecycle-platforms
  expects 54 and 32); `lifecycle_faults` 59 -> 62. 16 concurrent copies of the suite (two rounds
  of 8): 16/16 green, no `citrate-lcf-` process left.

### SCL-S3 review round 4 (2026-10-23)

- **Call-site pin (independent review 1).** `a_success_collected_late_by_the_monitor_never_reads_ready`
  holds the monitor at the new `collect_barrier` seam (before the turn reads the clock) while an
  on-time probe result waits, past the freshness limit. It fails when the call site passes the
  monitor's `now` instead of `done.finished_at` (Ready, age 0, one credit).
- **Flapping fixture (review 2).** It now alternates globally and counts spawns by the
  supervisor's incarnation counter instead of pid files. It still fails the "credit on the
  first success" mutant (incarnation 5, four credits).
- **#352 merged up** at `e4684e6` (an uncommitted ticket stays owned until commit or withdraw).
- **Counts:** kit lib 378; `supervisor::tests` 55 Unix / 33 Windows; `lifecycle_faults` 65.
  Load: 32 runs (4 rounds of 8 concurrent copies) at a load average of about 77: 32/32 green, no
  `citrate-lcf-` process left.

### SCL-S3 review round 5 (2026-10-25)

- **#352 merged up** at `d6b6ef2` (main through #351, #412, #414; only a control that armed a
  deadline can be `DeadlineExpired`). The reader reconcile now runs before every spawn; the
  core#398 tripwire scans `start_intent`, where SC2 moved the Start logic.
- **Member join with an unspent budget (Codex P2 at `44a68e0`).** The monitor adopts a member
  Start under the lock (`adopt_member_budget`) at the budget rule and in `publish_alive`, never
  from the count read at the top of its turn. Red `4d31b08`:
  `a_member_joining_an_unspent_automated_start_gets_exactly_its_budget` (2 incarnations, not 1).
  `a_member_joining_during_the_spawn_call_gets_exactly_its_budget` kills the mutant that drops
  the adoption in `publish_alive`. Tripwire: `the_monitor_adopts_member_budgets_under_the_lock_source_check`.
- **Ready during an unhealthy teardown (Codex P2 at `44a68e0`).** An unhealthy verdict publishes
  `Unavailable` before it returns, so the teardown never runs under a cached Ready. Red `4d31b08`:
  `a_failed_probe_outside_the_grace_clears_ready_before_teardown` (first non-Ready was `Stale`).
  Tripwire: `an_unhealthy_verdict_clears_ready_first_source_check`.
- **Counts:** kit lib 532 passed, 1 ignored; `supervisor::tests` 61 Unix / 39 Windows;
  `lifecycle_faults` 73. Load: 16 runs (2 rounds of 8 concurrent copies) at a load average of
  about 77 to 120: 16/16 green, no `citrate-lcf-` process left.
