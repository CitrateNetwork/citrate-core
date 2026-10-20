---------------------------- MODULE SidecarSupervisor ----------------------------
(***************************************************************************)
(* Formal model of the shared sidecar supervisor (kit/src/supervisor.rs,   *)
(* `run_monitor` and `supervise_running`), one lifecycle cell.             *)
(*                                                                         *)
(* History. CORE-C1.0b wrote the first version to pin F-1 (consecutive,   *)
(* not lifetime, failure counter) and the fork-bomb bound, over the states *)
(* {Off, Starting, Running, Backoff, Failed}. SCL-S1.1 extends it to the   *)
(* owned-lifecycle contract adopted in docs/adr/ADR-2026-10-05-owned-      *)
(* sidecar-lifecycle.md (base design: ADR-0001 / SPEC-001), per planset    *)
(* 2026-10-05-sidecar-lifecycle 03_TLA_SPECS.md section 1 and the S1.7     *)
(* corrections (RT-17 reachability witnesses).                             *)
(*                                                                         *)
(* The earlier model was green while defects (a) to (c) were present,      *)
(* because each sat outside its abstraction. This version adds the state   *)
(* those defects live in (intent, tickets, probe permit and workers,       *)
(* attested health) and carries one mutant per defect, selected by the     *)
(* constant Mutant, that must make TLC report the named invariant:         *)
(*   "StopOff"        Stop in Stopped continues the spawn loop  -> INV-5   *)
(*   "ElapsedCredit"  a tick grants retry credit for Probed     -> INV-7   *)
(*   "TimeoutFrees"   a probe timeout frees the permit          -> INV-10  *)
(*   "StopIgnoresWorkers" StopComplete ignores probe workers    -> INV-16  *)
(*   "AutoStartResets" an automated restart resets the budget   -> INV-17  *)
(*   "StaleSuccess"   ProbeOk accepts an older incarnation      -> INV-13  *)
(*                                                                         *)
(* Refinement notes. Every action names the Rust arm it models today, and  *)
(* the SCL work package that changes it. A WP that changes one of those    *)
(* arms updates the note in the same PR (03, "Lesson carried into the      *)
(* plan"); the lifecycle fault suite (SCL-S13) is the code-level check.    *)
(*                                                                         *)
(* Abstractions (stated so they can be checked, not hidden):               *)
(*  - One service. Several owners and the app gate are AppExitCoordinator. *)
(*  - Time is abstract ticks of one Alive incarnation (`age`).             *)
(*  - Child teardown (TERM, grace, KILL, reap) is one step, TeardownChild. *)
(*    The retained-anchor group and Job ordering is not modelled here; see *)
(*    the OwnedContainment decision in the SCL-S1 EVIDENCE.                *)
(*  - Defects (d) bytes and (e) descendants are not state-machine          *)
(*    properties at this level (fault suite SCL-S13; X-7 in                *)
(*    AppExitCoordinator for the nested-owner rule).                       *)
(*  - Log readers and credential files are folded into the child and the   *)
(*    ticket: they are released with them.                                *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxRetries,        \* backoff.max_retries (the fork-bomb bound)
    MaxCrashes,        \* model bound on recorded crashes (CONSTRAINT only)
    MaxInc,            \* model bound on child incarnations (CONSTRAINT only)
    MaxIntents,        \* bound on control intents the environment may commit
    MaxGen,            \* bound on committed replacement configurations
    CanHealthy,        \* TRUE: the child can produce a sustained healthy run
                       \*   (LivenessOnly: it outlives healthy_after; Probed: its
                       \*   probe can succeed). FALSE: crash-loop or always-failing
                       \*   probe (the fork-bomb configs).
    Profile,           \* "LivenessOnly" (no configured probe) or "Probed"
    HealthyAfterTicks, \* healthy_after, in ticks (LivenessOnly: elapsed; Probed:
                       \*   consecutive fresh successful samples)
    GraceTicks,        \* HealthCheck.grace, in ticks
    FreshTicks,        \* maximum success gap before Ready goes Stale
    MaxAge,            \* bound on ticks of one incarnation
    Mutant             \* "none" or one of the mutant names in the header

Profiles == {"LivenessOnly", "Probed"}
Mutants  == {"none", "StopOff", "ElapsedCredit", "TimeoutFrees", "StopIgnoresWorkers",
             "AutoStartResets", "StaleSuccess"}

ASSUME MaxRetries \in Nat /\ MaxCrashes \in Nat /\ MaxInc \in Nat
ASSUME MaxIntents \in Nat \ {0} /\ MaxGen \in Nat
ASSUME CanHealthy \in BOOLEAN
ASSUME Profile \in Profiles
ASSUME HealthyAfterTicks \in Nat \ {0} /\ GraceTicks \in Nat
ASSUME FreshTicks \in Nat \ {0} /\ MaxAge \in Nat
ASSUME Mutant \in Mutants

Phases == {"Stopped", "Starting", "Alive", "Backoff", "Failed", "Stopping", "Quarantined"}
ReadinessValues == {"NotAssessed", "Awaiting", "Ready", "Stale", "Unavailable"}

VARIABLES
    phase,         \* lifecycle phase (SPEC-001 "Process state, readiness and retries")
    desired,       \* latest committed intent: "Run" or "Stop"
    intentSeq,     \* controlIntentSequence
    gen,           \* serviceGeneration
    inc,           \* childIncarnation of the newest admitted spawn
    tickets,       \* admitted spawn tickets [s |-> seq, g |-> gen, i |-> inc]
    child,         \* TRUE iff an OS child is owned (of incarnation `inc`)
    replacing,     \* a replacement configuration waits for old teardown
    probe,         \* the service's single probe permit: "Free" or "Busy"
    workers,       \* probe workers actually in flight [g, i, t]; t = timed out
    okWindow,      \* consecutive fresh successful samples of this incarnation
    okSinceSpawn,  \* some sample of this incarnation succeeded
    credited,      \* retry credit granted for the current uninterrupted window
    age,           \* ticks since this incarnation became Alive
    sinceOk,       \* ticks since the last success (freshness)
    readiness,     \* readiness, independent of phase
    lastOk,        \* [g, i] of the last successful sample
    failures,      \* consecutive retry-budget counter
    crashes,       \* recorded failures (monotone; bounded by CONSTRAINT)
    sinceMember    \* recorded failures since the last member Start (INV-17)

ctrl    == << phase, desired, intentSeq, gen, inc, tickets, child, replacing >>
probing == << probe, workers >>
health  == << okWindow, okSinceSpawn, credited, age, sinceOk, readiness, lastOk >>
vars    == << ctrl, probing, health, failures, crashes, sinceMember >>

Min(a, b) == IF a < b THEN a ELSE b

Tag == [g |-> gen, i |-> inc]

\* What a teardown still has to release (03 calls this `cleanup`). Derived
\* from the owned resources so it cannot drift from them.
Cleanup ==
    (IF child THEN {"child"} ELSE {})
    \cup (IF tickets # {} THEN {"tickets"} ELSE {})
    \cup (IF workers # {} THEN {"probe"} ELSE {})

NotReady       == IF Profile = "Probed" THEN "Unavailable" ELSE "NotAssessed"
SpawnReadiness == IF Profile = "Probed" THEN "Awaiting" ELSE "NotAssessed"
InGrace        == age < GraceTicks

\* The health window of an incarnation ends.
ResetWindow ==
    /\ okWindow' = 0
    /\ okSinceSpawn' = FALSE
    /\ credited' = FALSE
    /\ age' = 0
    /\ sinceOk' = 0
    /\ readiness' = NotReady

TypeOK ==
    /\ phase \in Phases
    /\ desired \in {"Run", "Stop"}
    /\ intentSeq \in 0..MaxIntents
    /\ gen \in 0..MaxGen
    /\ inc \in Nat
    /\ \A t \in tickets : t.s \in 0..MaxIntents /\ t.g \in 0..MaxGen /\ t.i \in Nat
    /\ child \in BOOLEAN
    /\ replacing \in BOOLEAN
    /\ probe \in {"Free", "Busy"}
    /\ \A w \in workers : w.g \in 0..MaxGen /\ w.i \in Nat /\ w.t \in BOOLEAN
    /\ okWindow \in 0..HealthyAfterTicks
    /\ okSinceSpawn \in BOOLEAN
    /\ credited \in BOOLEAN
    /\ age \in 0..MaxAge
    /\ sinceOk \in 0..(FreshTicks + 1)
    /\ readiness \in ReadinessValues
    /\ lastOk.g \in 0..MaxGen /\ lastOk.i \in Nat
    /\ failures \in 0..(MaxRetries + 1)
    /\ crashes \in Nat
    /\ sinceMember \in Nat

\* Supervisor::start publishes Starting with a Run intent already committed.
Init ==
    /\ phase = "Starting"
    /\ desired = "Run"
    /\ intentSeq = 1
    /\ gen = 0
    /\ inc = 0
    /\ tickets = {}
    /\ child = FALSE
    /\ replacing = FALSE
    /\ probe = "Free"
    /\ workers = {}
    /\ okWindow = 0
    /\ okSinceSpawn = FALSE
    /\ credited = FALSE
    /\ age = 0
    /\ sinceOk = 0
    /\ readiness = NotReady
    /\ lastOk = [g |-> 0, i |-> 0]
    /\ failures = 0
    /\ crashes = 0
    /\ sinceMember = 0

(***************************************************************************)
(* A failed incarnation: record it, consume one unit of retry budget, and  *)
(* go to Backoff, or to Failed past the cap. Today: the three identical     *)
(* `consecutive_failures += 1; if > max_retries { Failed } else backoff`    *)
(* arms of run_monitor (spawn Err, RunOutcome::Crashed, ::Unhealthy).      *)
(***************************************************************************)
FailStep ==
    /\ child' = FALSE
    /\ crashes' = crashes + 1
    /\ sinceMember' = sinceMember + 1
    /\ failures' = failures + 1
    /\ phase' = IF failures + 1 > MaxRetries THEN "Failed" ELSE "Backoff"
    /\ ResetWindow

(***************************************************************************)
(* Control intents (SPEC-001 "Ownership, intent and generations").         *)
(***************************************************************************)

\* Member Start: a person asks for the service (a Start button, a settings
\* toggle the member flips). A new intent with a fresh retry budget, so the
\* fork-bomb bound (INV-2, INV-17) holds between member Starts, not over the
\* app's lifetime. Admitted from a settled phase only once old teardown is
\* complete, or queued behind a teardown in progress (it then waits for
\* StopComplete). Only this action resets `failures` outside ResetCredit.
\* Today: SidecarSupervisor::start creates a new monitor (every caller resets,
\* member or not); there is no intent cell. Target: SCL-S2.1, S2.3, S3.2.
MemberStart ==
    /\ intentSeq < MaxIntents
    /\ \/ phase \in {"Stopped", "Failed", "Quarantined"} /\ Cleanup = {}
       \/ phase = "Stopping" /\ desired = "Stop"
    /\ desired' = "Run"
    /\ intentSeq' = intentSeq + 1
    /\ phase' = IF phase = "Stopping" THEN "Stopping" ELSE "Starting"
    /\ failures' = 0
    /\ sinceMember' = 0
    /\ UNCHANGED << gen, inc, tickets, child, replacing, probing, health, crashes, sinceMember >>

\* Kept for readers of the first version: the explicit Start is the member's.
RequestStart == MemberStart

\* Automated Start: no member asked (auto-start at launch, a lazy
\* ensure_started from Hermes tools or another service). It carries the
\* retry budget over, and it is refused while Failed or Quarantined: only a
\* member Start revives those. Target: SCL-S2.3, S3.2.
\* Mutant "AutoStartResets": it resets the budget as a member Start does.
AutoStart ==
    /\ intentSeq < MaxIntents
    /\ \/ phase = "Stopped" /\ Cleanup = {}
       \/ phase = "Stopping" /\ desired = "Stop"
    /\ desired' = "Run"
    /\ intentSeq' = intentSeq + 1
    /\ phase' = IF phase = "Stopping" THEN "Stopping" ELSE "Starting"
    /\ failures' = IF Mutant = "AutoStartResets" THEN 0 ELSE failures
    /\ UNCHANGED << gen, inc, tickets, child, replacing, probing, health, crashes, sinceMember >>

\* Automated restart: an internal stop then start of a running service, as
\* one new Run intent (tickets of the old intent go to teardown, then
\* StopComplete re-enters Starting). Today: `NodeManager::arm_mining` and
\* `disarm_mining` (`self.stop(); let _ = self.start();`, src-tauri/src/node.rs
\* 511-521 and 599-608); a watchdog restart is the same shape. These must not
\* reset the budget: a crash-looping node under a flapping mining gate would
\* otherwise get a fresh budget on every flip. Target: SCL-S2.3, S3.2, S6.1.
\* Mutant "AutoStartResets": it resets the budget.
AutoRestart ==
    /\ intentSeq < MaxIntents
    /\ desired = "Run"
    /\ ~replacing
    /\ phase \in {"Starting", "Alive", "Backoff"}
    /\ intentSeq' = intentSeq + 1
    /\ phase' = "Stopping"
    /\ ResetWindow
    /\ failures' = IF Mutant = "AutoStartResets" THEN 0 ELSE failures
    /\ UNCHANGED << desired, gen, inc, tickets, child, replacing, probing, lastOk,
                    crashes, sinceMember >>

\* Stop commits desired = Stop under a new sequence, closes spawn admission
\* in every phase, and invalidates a pending replacement. A repeated
\* matching Stop joins the pending one (INV-6).
\* Today: SidecarSupervisor::stop sends ControlMsg::Stop; run_monitor's
\* Stopped arm answers a second Stop with `continue`, which re-enters the
\* spawn loop (defect (a)). Target: SCL-S2.2.
\* Mutant "StopOff": the join arm in Stopped re-enters the spawn loop and
\* admits a spawn, as today's `continue` does.
RequestStop ==
    IF desired = "Stop"
    THEN IF Mutant = "StopOff" /\ phase = "Stopped"
         THEN /\ phase' = "Starting"
              /\ tickets' = {[s |-> intentSeq, g |-> gen, i |-> inc + 1]}
              /\ inc' = inc + 1
              /\ UNCHANGED << desired, intentSeq, gen, child, replacing,
                              probing, health, failures, crashes, sinceMember >>
         ELSE UNCHANGED vars
    ELSE /\ intentSeq < MaxIntents
         /\ desired' = "Stop"
         /\ intentSeq' = intentSeq + 1
         /\ replacing' = FALSE
         /\ phase' = IF phase \in {"Starting", "Alive", "Backoff", "Stopping"}
                     THEN "Stopping" ELSE phase
         /\ ResetWindow
         /\ UNCHANGED << gen, inc, tickets, child, probing, lastOk, failures, crashes, sinceMember >>

\* Replacement configuration: keep old resources, tear the old incarnation
\* down, commit the new generation only after cleanup (ReplaceConfig).
\* Today: managers stop and re-create the supervisor. Target: SCL-S2.3.
RequestReplace ==
    /\ desired = "Run"
    /\ ~replacing
    /\ gen < MaxGen
    /\ intentSeq < MaxIntents
    /\ phase \in {"Starting", "Alive", "Backoff"}
    /\ intentSeq' = intentSeq + 1
    /\ replacing' = TRUE
    /\ phase' = "Stopping"
    /\ ResetWindow
    /\ UNCHANGED << desired, gen, inc, tickets, child, probing, lastOk, failures, crashes, sinceMember >>

(***************************************************************************)
(* Spawn admission and its ticket.                                         *)
(***************************************************************************)

\* Admission checks the latest intent under the lifecycle lock and
\* registers the ticket before the OS call (SPEC-001: "Spawn is an owned
\* admission ticket registered before releasing the locks").
\* Today: the top of run_monitor's loop calls spawn_child unconditionally.
\* Target: SCL-S2.1, S2.2.
AdmitSpawn ==
    /\ phase = "Starting"
    /\ desired = "Run"
    /\ ~replacing
    /\ tickets = {}
    /\ ~child
    /\ tickets' = {[s |-> intentSeq, g |-> gen, i |-> inc + 1]}
    /\ inc' = inc + 1
    /\ UNCHANGED << phase, desired, intentSeq, gen, child, replacing,
                    probing, health, failures, crashes, sinceMember >>

\* The OS spawn returns a child. A ticket of the current intent and
\* generation publishes Alive; a late return (Stop or replacement won the
\* race) goes straight to teardown and never Ready.
\* Today: `spawn_child` Ok arm, then Running is published. Target: SCL-S2.2.
SpawnReturns(t) ==
    /\ t \in tickets
    /\ tickets' = tickets \ {t}
    /\ child' = TRUE
    /\ IF t.s = intentSeq /\ t.g = gen /\ desired = "Run" /\ phase = "Starting"
       THEN /\ phase' = "Alive"
            /\ okWindow' = 0
            /\ okSinceSpawn' = FALSE
            /\ credited' = FALSE
            /\ age' = 0
            /\ sinceOk' = 0
            /\ readiness' = SpawnReadiness
       ELSE UNCHANGED << phase, okWindow, okSinceSpawn, credited, age, sinceOk, readiness >>
    /\ UNCHANGED << desired, intentSeq, gen, inc, replacing, probing, lastOk, failures, crashes, sinceMember >>

\* The OS spawn fails (bad path, EACCES). Counted only for the current
\* intent (SpawnFailCurrent); a late failure after Stop or replacement
\* counts nothing (SpawnFailStale, INV-3). Today: `spawn_child` Err arm.
\* Only in the crash-loop configs, as before.
SpawnFailCurrent(t) ==
    /\ ~CanHealthy
    /\ t \in tickets
    /\ t.s = intentSeq /\ t.g = gen /\ desired = "Run" /\ phase = "Starting"
    /\ tickets' = tickets \ {t}
    /\ FailStep
    /\ UNCHANGED << desired, intentSeq, gen, inc, replacing, probing, lastOk >>

SpawnFailStale(t) ==
    /\ ~CanHealthy
    /\ t \in tickets
    /\ ~(t.s = intentSeq /\ t.g = gen /\ desired = "Run" /\ phase = "Starting")
    /\ tickets' = tickets \ {t}
    /\ UNCHANGED << phase, desired, intentSeq, gen, inc, child, replacing,
                    probing, health, failures, crashes, sinceMember >>

(***************************************************************************)
(* Alive: crashes, retry credit, backoff.                                  *)
(***************************************************************************)

\* The child exits on its own (or try_wait errors). In the recovering
\* configs a crash comes only after a credited window, as before: a
\* long-lived daemon outlives healthy_after between crashes.
\* Today: RunOutcome::Crashed (and the try_wait Err arm).
Crash ==
    /\ phase = "Alive"
    /\ child
    /\ (CanHealthy => credited)
    /\ FailStep
    /\ UNCHANGED << desired, intentSeq, gen, inc, tickets, replacing, probing, lastOk >>

\* Retry credit. Probed: only after HealthyAfterTicks consecutive fresh
\* successes of this incarnation (INV-7). LivenessOnly: today's elapsed
\* Running rule, unchanged (INV-9). Granted once per window.
\* Today: `refresh_health!` plus `if report.sustained_healthy` (elapsed time
\* for every profile: defect (b) for Probed). Target: SCL-S3.2.
ResetCredit ==
    /\ phase = "Alive"
    /\ child
    /\ ~credited
    /\ IF Profile = "Probed"
       THEN okWindow >= HealthyAfterTicks
       ELSE CanHealthy /\ age >= HealthyAfterTicks
    /\ failures' = 0
    /\ credited' = TRUE
    /\ UNCHANGED << ctrl, probing, okWindow, okSinceSpawn, age, sinceOk, readiness, lastOk, crashes, sinceMember >>

\* Backoff elapsed: back to Starting; admission still checks the intent.
\* Today: `backoff_or_control` Timeout arm.
RetryFromBackoff ==
    /\ phase = "Backoff"
    /\ desired = "Run"
    /\ phase' = "Starting"
    /\ UNCHANGED << desired, intentSeq, gen, inc, tickets, child, replacing,
                    probing, health, failures, crashes, sinceMember >>

(***************************************************************************)
(* Time. Ticks advance one incarnation's age. A Ready readiness whose last *)
(* success is older than FreshTicks goes Stale and ends the window.        *)
(* Mutant "ElapsedCredit": a tick at age >= healthy_after grants retry      *)
(* credit for Probed with no attested success, as today's elapsed rule     *)
(* does (defect (b)).                                                      *)
(* Today: the 20 ms poll loop of supervise_running and `refresh_health!`.  *)
(***************************************************************************)
TickStep ==
    /\ phase = "Alive"
    /\ age < MaxAge
    /\ age' = age + 1
    /\ sinceOk' = Min(sinceOk + 1, FreshTicks + 1)
    /\ LET stale     == readiness = "Ready" /\ sinceOk + 1 > FreshTicks
           mutCredit == Mutant = "ElapsedCredit" /\ Profile = "Probed"
                        /\ age + 1 >= HealthyAfterTicks /\ ~credited
       IN  /\ readiness' = IF stale THEN "Stale" ELSE readiness
           /\ okWindow' = IF stale THEN 0 ELSE okWindow
           /\ credited' = IF mutCredit THEN TRUE ELSE IF stale THEN FALSE ELSE credited
           /\ failures' = IF mutCredit THEN 0 ELSE failures
    /\ UNCHANGED << ctrl, probing, okSinceSpawn, lastOk, crashes, sinceMember >>

GraceTick   == InGrace /\ TickStep
ElapsedTick == ~InGrace /\ TickStep

(***************************************************************************)
(* Probes. One permit per service, held across timeouts and generations    *)
(* until the worker actually ends (SPEC-001 "Finite actual workers").      *)
(***************************************************************************)

CurrentWorker(w) == w.g = gen /\ w.i = inc /\ ~w.t /\ phase = "Alive"

\* Today: `ProbeRunner::spawn` when no probe is in flight. Target: SCL-S4.1.
ProbeBegin ==
    /\ Profile = "Probed"
    /\ phase = "Alive"
    /\ child
    /\ desired = "Run"
    /\ probe = "Free"
    /\ probe' = "Busy"
    /\ workers' = workers \cup {[g |-> gen, i |-> inc, t |-> FALSE]}
    /\ UNCHANGED << ctrl, health, failures, crashes, sinceMember >>

\* A fresh success of the current incarnation: Ready at once, no credit by
\* itself (credit is ResetCredit). Today: `runner.poll()` returns true.
\* Mutant "StaleSuccess": a success of an older incarnation (or a timed-out
\* worker) is accepted, the stale-completion defect INV-12 is about.
ProbeOk(w) ==
    /\ CanHealthy
    /\ w \in workers
    /\ \/ CurrentWorker(w)
       \/ Mutant = "StaleSuccess" /\ phase = "Alive" /\ child
    /\ workers' = workers \ {w}
    /\ probe' = "Free"
    /\ okWindow' = Min(okWindow + 1, HealthyAfterTicks)
    /\ okSinceSpawn' = TRUE
    /\ readiness' = "Ready"
    /\ lastOk' = [g |-> w.g, i |-> w.i]
    /\ sinceOk' = 0
    /\ UNCHANGED << ctrl, credited, age, failures, crashes, sinceMember >>

\* A failed sample. Inside the grace it is tolerated but ends the window;
\* outside it the incarnation is unhealthy (teardown, one failure).
\* Today: `runner.poll()` returns false (RunOutcome::Unhealthy outside grace).
ProbeFail(w) ==
    /\ w \in workers
    /\ CurrentWorker(w)
    /\ InGrace \/ (CanHealthy => credited)
    /\ workers' = workers \ {w}
    /\ probe' = "Free"
    /\ IF InGrace
       THEN /\ okWindow' = 0
            /\ credited' = FALSE
            /\ readiness' = IF readiness = "Awaiting" THEN "Awaiting" ELSE "Unavailable"
            /\ UNCHANGED << ctrl, okSinceSpawn, age, sinceOk, lastOk, failures, crashes, sinceMember >>
       ELSE /\ FailStep
            /\ UNCHANGED << desired, intentSeq, gen, inc, tickets, replacing, lastOk >>

\* The observer deadline passes with no result: publish Unavailable, end the
\* window, keep the worker counted and the permit Busy (INV-11).
\* Today: `probe_elapsed >= probe_timeout` drops the runner and a new one may
\* start at the next interval (defect (c)). Target: SCL-S4.1.
\* Mutant "TimeoutFrees": the timeout frees the permit while the worker runs.
ProbeTimeout(w) ==
    /\ w \in workers
    /\ CurrentWorker(w)
    /\ InGrace \/ (CanHealthy => credited)
    /\ workers' = (workers \ {w}) \cup {[w EXCEPT !.t = TRUE]}
    /\ probe' = IF Mutant = "TimeoutFrees" THEN "Free" ELSE "Busy"
    /\ IF InGrace
       THEN /\ okWindow' = 0
            /\ credited' = FALSE
            /\ readiness' = IF readiness = "Awaiting" THEN "Awaiting" ELSE "Unavailable"
            /\ UNCHANGED << ctrl, okSinceSpawn, age, sinceOk, lastOk, failures, crashes, sinceMember >>
       ELSE /\ FailStep
            /\ UNCHANGED << desired, intentSeq, gen, inc, tickets, replacing, lastOk >>

\* A worker whose result no longer counts (timed out, older generation or
\* incarnation, or the incarnation closed) actually ends. It frees the permit
\* and changes nothing else: this is also StaleCompletion (INV-12).
\* Today: the dropped ProbeRunner's detached thread ends unobserved.
ProbeWorkerEnds(w) ==
    /\ w \in workers
    /\ ~CurrentWorker(w)
    /\ workers' = workers \ {w}
    /\ probe' = IF workers \ {w} = {} THEN "Free" ELSE probe
    /\ UNCHANGED << ctrl, health, failures, crashes, sinceMember >>

\* A worker that never ends: once it is past its observer deadline (or a
\* Stop is waiting on it), the owner kills the child, keeps the permit and
\* the slot, and reports Quarantined. Leaving needs ProbeWorkerEnds plus an
\* explicit Start (INV-15). Failed is included: a Failed cell whose timed-out
\* worker never ends is Quarantined too (SPEC-001), not Failed with the
\* permit silently held. Today: no such state. Target: SCL-S4.2.
ProbeNeverEnds ==
    /\ \E w \in workers : w.t \/ phase = "Stopping"
    /\ phase \in {"Alive", "Starting", "Backoff", "Stopping", "Failed"}
    /\ tickets = {}
    /\ phase' = "Quarantined"
    /\ child' = FALSE
    /\ replacing' = FALSE
    /\ ResetWindow
    /\ UNCHANGED << desired, intentSeq, gen, inc, tickets, probing, lastOk, failures, crashes, sinceMember >>

(***************************************************************************)
(* Teardown.                                                               *)
(***************************************************************************)

\* Terminate and reap the owned child (SIGTERM, stop_grace, SIGKILL, reap).
\* Today: `terminate_child` in the Stopped / Shutdown / Unhealthy arms.
TeardownChild ==
    /\ phase = "Stopping"
    /\ child
    /\ child' = FALSE
    /\ UNCHANGED << phase, desired, intentSeq, gen, inc, tickets, replacing,
                    probing, health, failures, crashes, sinceMember >>

\* Every owned resource is released: Stopped, or Starting for a Start queued
\* behind the teardown. Stopped attests "no owned resources from the
\* completed control" (SPEC-001 line 92), probe workers included (INV-16).
\* Today: `shared.set_state(Off)` after terminate_child.
\* Mutant "StopIgnoresWorkers": completes with a probe worker still running.
StopComplete ==
    /\ phase = "Stopping"
    /\ ~replacing
    /\ IF Mutant = "StopIgnoresWorkers" THEN ~child /\ tickets = {} ELSE Cleanup = {}
    /\ phase' = IF desired = "Run" THEN "Starting" ELSE "Stopped"
    /\ UNCHANGED << desired, intentSeq, gen, inc, tickets, child, replacing,
                    probing, health, failures, crashes, sinceMember >>

\* The replacement commits its generation only after cleanup (INV-14).
\* Target: SCL-S2.3.
ReplaceConfig ==
    /\ phase = "Stopping"
    /\ replacing
    /\ Cleanup = {}
    /\ gen' = gen + 1
    /\ replacing' = FALSE
    /\ phase' = "Starting"
    /\ UNCHANGED << desired, intentSeq, inc, tickets, child, probing, health, failures, crashes, sinceMember >>

\* Settled phases may idle (behaviours are infinite, no deadlock artefact).
TerminalStutter ==
    /\ phase \in {"Stopped", "Failed", "Quarantined"}
    /\ UNCHANGED vars

Next ==
    \/ MemberStart
    \/ AutoStart
    \/ AutoRestart
    \/ RequestStop
    \/ RequestReplace
    \/ AdmitSpawn
    \/ \E t \in tickets : SpawnReturns(t) \/ SpawnFailCurrent(t) \/ SpawnFailStale(t)
    \/ Crash
    \/ ResetCredit
    \/ RetryFromBackoff
    \/ GraceTick
    \/ ElapsedTick
    \/ ProbeBegin
    \/ \E w \in workers : ProbeOk(w) \/ ProbeFail(w) \/ ProbeTimeout(w) \/ ProbeWorkerEnds(w)
    \/ ProbeNeverEnds
    \/ TeardownChild
    \/ StopComplete
    \/ ReplaceConfig
    \/ TerminalStutter

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Fork-bomb specification: a child that never earns credit and is never   *)
(* stopped. Only the spawn, crash, probe, tick and retry steps run, under   *)
(* weak fairness, so TLC can show the run reaches Failed (INV-2's teeth).   *)
(***************************************************************************)
ForkBombNext ==
    \/ AdmitSpawn
    \/ \E t \in tickets : SpawnReturns(t) \/ SpawnFailCurrent(t) \/ SpawnFailStale(t)
    \/ Crash
    \/ ResetCredit
    \/ RetryFromBackoff
    \/ GraceTick
    \/ ElapsedTick
    \/ ProbeBegin
    \/ \E w \in workers : ProbeOk(w) \/ ProbeFail(w) \/ ProbeTimeout(w) \/ ProbeWorkerEnds(w)
    \/ TerminalStutter

ForkBombFairness ==
    /\ WF_vars(AdmitSpawn)
    /\ WF_vars(\E t \in tickets : SpawnReturns(t) \/ SpawnFailCurrent(t))
    /\ WF_vars(Crash)
    /\ WF_vars(RetryFromBackoff)
    /\ WF_vars(GraceTick \/ ElapsedTick)
    /\ WF_vars(ProbeBegin)
    /\ WF_vars(\E w \in workers : ProbeFail(w) \/ ProbeTimeout(w))
    /\ WF_vars(\E w \in workers : ProbeWorkerEnds(w))

ForkBombSpec == Init /\ [][ForkBombNext]_vars /\ ForkBombFairness

(***************************************************************************)
(* Fork bomb under automated restarts (B2): the never-healthy child also   *)
(* gets automated stop-then-start (a flapping mining gate) and automated   *)
(* starts, but no member Start. The run must still reach Failed, and       *)
(* INV-17 bounds the crashes. Automated intents are bounded by MaxIntents. *)
(***************************************************************************)
AutoRestartNext ==
    \/ ForkBombNext
    \/ AutoRestart
    \/ AutoStart
    \/ TeardownChild
    \/ StopComplete

AutoRestartSpec ==
    /\ Init
    /\ [][AutoRestartNext]_vars
    /\ ForkBombFairness
    /\ WF_vars(TeardownChild)
    /\ WF_vars(StopComplete)

(***************************************************************************)
(* INVARIANTS (03 section 1). State invariants unless marked "action".     *)
(***************************************************************************)

\* INV-1 (F-1): a recovering child is never permanently Failed. Checked in
\* the CanHealthy configs, where every failure follows a credited window:
\* LivenessOnly by elapsed time, Probed by attested fresh successes.
INV_NeverFailedWhenRecovering == phase # "Failed"

\* INV-2 (fork-bomb bound): the counter is bounded; with
\* EventuallyFailedIfNeverHealthy, a never-healthy child reaches Failed.
INV_CounterBounded == failures <= MaxRetries + 1

\* INV-3: an intentional stop is not a crash. State part (kept): Stopped
\* owns no child. Action part: StopNeverCounts below.
INV_StopIsNotACrash == (phase = "Stopped") => ~child

\* INV-4 (restated): Stopped or Failed owns no live child and no ticket.
INV_NoOrphan == (phase \in {"Stopped", "Failed"}) => (~child /\ tickets = {})

\* INV-5 (defect (a)): under a Stop intent no spawn can be admitted, in
\* every phase, and every outstanding ticket predates that intent. The
\* `~ENABLED AdmitSpawn` half is true by construction (AdmitSpawn checks
\* desired = "Run"); the teeth are in the ticket clause, which MutStopOff
\* breaks.
INV_StopNeverSpawns ==
    desired = "Stop" => (~ENABLED AdmitSpawn /\ \A t \in tickets : t.s < intentSeq)

\* INV-7 (defect (b)): for Probed, credit only from an attested window.
INV_CreditOnlyFromAttestedHealth ==
    (Profile = "Probed" /\ credited) => okWindow >= HealthyAfterTicks

\* INV-8 (defect (b)): no credit while every sample since the spawn failed
\* or timed out.
INV_GraceNeverCredits ==
    (Profile = "Probed" /\ credited) => okSinceSpawn

\* INV-9 (D-6): LivenessOnly keeps today's elapsed rule and NotAssessed.
INV_LivenessOnlyResetUnchanged ==
    Profile = "LivenessOnly" =>
        /\ readiness = "NotAssessed"
        /\ workers = {}
        /\ (credited => age >= HealthyAfterTicks)

\* INV-10 (defect (c)): at most one probe worker in flight, across
\* generations and incarnations.
INV_ProbePermitBounded == Cardinality(workers) <= 1

\* INV-11 (defect (c)): the permit stays Busy while any worker runs; only
\* ProbeWorkerEnds frees it.
INV_TimeoutNeverFreesPermit == (workers # {}) => (probe = "Busy")

\* INV-13: Ready belongs to the current, live incarnation and is fresh.
INV_ReadyIsCurrent ==
    (readiness = "Ready") =>
        /\ phase = "Alive"
        /\ child
        /\ lastOk = Tag
        /\ sinceOk <= FreshTicks

\* INV-16 (B1): Stopped owns nothing: no child, no ticket and no probe
\* worker (SPEC-001 line 92, "no owned resources from the completed
\* control"). The supervisor form of "Complete attests only its named scope".
INV_StoppedOwnsNothing == (phase = "Stopped") => (Cleanup = {})

\* INV-17 (B2): for a never-healthy child the recorded failures since the
\* last member Start stay within the fork-bomb bound, whatever automated
\* starts and restarts happen. The bound holds between member Starts; a
\* member Start is a new budget by decision.
INV_CrashBoundBetweenMemberStarts == ~CanHealthy => (sinceMember <= MaxRetries + 1)

\* INV-15 (state part): Quarantined holds no child and no ticket.
INV_QuarantineHoldsNoChild == (phase = "Quarantined") => (~child /\ tickets = {})

\* INV-6 (action): a Stop with the same target as the pending one changes
\* neither the incarnation, the tickets nor the intent sequence.
INV_RepeatedStopJoins ==
    [][(desired = "Stop" /\ RequestStop) => UNCHANGED << inc, tickets, intentSeq >>]_vars

\* INV-3 (action part): nothing counts a failure under a Stop intent.
StopNeverCounts ==
    [][(desired' = "Stop") => (crashes' = crashes /\ failures' <= failures)]_vars

\* INV-12 (action): a stale completion changes no readiness, counter,
\* ticket, child or phase. True by construction of ProbeWorkerEnds; the real
\* stale-result defect (ProbeOk accepting an old tag) is MutStaleSuccess,
\* which INV-13 catches.
INV_StaleCompletionInert ==
    [][\A w \in workers : ProbeWorkerEnds(w) =>
          UNCHANGED << phase, child, tickets, health, failures, crashes, sinceMember >>]_vars

\* INV-14 (action): the generation advances only when cleanup is empty.
INV_ReplaceAfterCleanup == [][(gen' # gen) => (Cleanup = {})]_vars

\* INV-15 (action): Quarantined is left only after every worker ended, and
\* only by a fresh explicit Start; a timeout never leaves it.
INV_QuarantineNotRecycled ==
    [][(phase = "Quarantined" /\ phase' # "Quarantined") =>
          (workers = {} /\ desired' = "Run" /\ intentSeq' = intentSeq + 1)]_vars

(***************************************************************************)
(* LIVENESS (fork-bomb configs): a never-healthy child reaches Failed.     *)
(***************************************************************************)
EventuallyFailedIfNeverHealthy == <>(phase = "Failed")

(***************************************************************************)
(* REACHABILITY WITNESSES (RT-17). Each is an invariant TLC must report    *)
(* violated on the green config the matching mutant is built on, which     *)
(* shows the mutated action is reachable there.                            *)
(***************************************************************************)

\* StopOff: a Stop arrives while Stopped (the join arm fires).
W_StopInStopped == ~(phase = "Stopped" /\ desired = "Stop" /\ ENABLED RequestStop)

\* ElapsedCredit: an Alive incarnation with no attested window ticks to
\* healthy_after (where the elapsed rule would grant credit).
W_UnattestedTickPastHealthy ==
    ~(/\ Profile = "Probed"
      /\ ~credited
      /\ okWindow < HealthyAfterTicks
      /\ age + 1 >= HealthyAfterTicks
      /\ ENABLED (GraceTick \/ ElapsedTick))

\* TimeoutFrees: a timed-out worker is still in flight while the service is
\* Alive (where a freed permit would admit a second worker).
W_TimedOutWorkerHeldWhileAlive ==
    ~(phase = "Alive" /\ \E w \in workers : w.t)

\* Action reachability (review round 2): a member Start can fire from Failed,
\* and after failures were recorded under the current member Start. Both are
\* required by INV-15 and INV-17 being non-vacuous.
W_MemberStartFromFailed    == ~(phase = "Failed" /\ ENABLED MemberStart)
W_MemberStartAfterFailures == ~(sinceMember > 0 /\ ENABLED MemberStart)

\* StopIgnoresWorkers (on the green base): a stop has released the child
\* and the tickets but a probe worker is still running.
W_StopWaitsOnWorker ==
    ~(phase = "Stopping" /\ ~replacing /\ ~child /\ tickets = {} /\ workers # {})

\* StopIgnoresWorkers (on the mutant): the cell reaches Stopped with a live
\* worker. Shows the mutant's counterexample is the defect itself.
W_StoppedWithWorker == ~(phase = "Stopped" /\ workers # {})

\* AutoStartResets: an automated restart or start can fire after failures
\* were recorded under the current member Start.
W_AutoRestartAfterFailures ==
    ~(failures > 0 /\ ENABLED (AutoRestart \/ AutoStart))

\* StaleSuccess: an Alive incarnation has a worker of an older incarnation,
\* or a timed-out one, still in flight.
W_StaleWorkerWhileAlive ==
    ~(phase = "Alive" /\ child /\ \E w \in workers : ~(w.g = gen /\ w.i = inc /\ ~w.t))

(***************************************************************************)
(* State constraint (model checking only) for the recovering configs,      *)
(* whose crash and restart cycles are otherwise unbounded. Intents and     *)
(* generations are bounded by guards on the environment's requests.        *)
(***************************************************************************)
Bounds == crashes <= MaxCrashes /\ inc <= MaxInc

\* Kept for the existing cfg name.
CrashesBounded == Bounds

=============================================================================
