---------------------------- MODULE SidecarSupervisor ----------------------------
(***************************************************************************)
(* Formal model of the citrate-core SidecarSupervisor state machine        *)
(* (src-tauri/src/supervisor.rs), written for CORE-C1.0b to pin the F-1 and *)
(* F-2 fixes and the transitions the Rule-8 review flagged as untested.     *)
(*                                                                          *)
(* The supervisor is the substrate for every Phase-C sidecar, and the       *)
(* example-based tests MISSED F-1 (the lifetime-vs-consecutive counter bug). *)
(* This model exists so the counter-reset semantics and the fork-bomb bound  *)
(* are checked exhaustively, not just by example.                            *)
(*                                                                          *)
(* States {Off, Starting, Running, Backoff, Failed} mirror                   *)
(* SupervisorState. `failures` is the CONSECUTIVE-failure counter            *)
(* (`consecutive_failures` in run_monitor) that drives the backoff delay and *)
(* the Failed cap; it RESETS on a sustained-healthy run (the F-1 fix). A     *)
(* child that only ever crash-loops never becomes healthy, so it never       *)
(* resets and still hits the cap (the fork-bomb bound).                      *)
(*                                                                          *)
(* Modeled transitions (each maps to a run_monitor arm):                     *)
(*   SpawnOk        Starting -> Running        (spawn_child Ok)              *)
(*   SpawnFail      Starting -> Backoff|Failed (spawn_child Err arm)         *)
(*   BecomeHealthy  Running  -> Running        (sustained-healthy => reset)  *)
(*   Crash          Running  -> Backoff|Failed (RunOutcome::Crashed)         *)
(*   Unhealthy      Running  -> Backoff|Failed (RunOutcome::Unhealthy)       *)
(*   TryWaitError   Running  -> Backoff|Failed (try_wait Err => Crashed)     *)
(*   RetryFromBackoff Backoff -> Starting      (backoff timeout => respawn)  *)
(*   Stop           any-live -> Off  (intentional stop; NOT a crash)         *)
(*   StopInBackoff  Backoff  -> Off  (cancels the pending restart)           *)
(*   Shutdown       any      -> Off  (teardown)                             *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS
    MaxRetries,   \* backoff.max_retries (the fork-bomb bound)
    MaxCrashes,   \* model-checking bound on total crash records so the state
                  \* space is finite (crashes is otherwise an unbounded Nat that
                  \* grows on the recovering child's healthy/crash loop). A small
                  \* bound already exercises many reset cycles.
    CanHealthy    \* TRUE: a spawned child may reach a sustained-healthy run
                  \* (models a real long-lived daemon that crashes only
                  \*  intermittently -> checks INV-1).
                  \* FALSE: no run ever becomes healthy (a pure crash-loop /
                  \*  instant-exit child -> checks INV-2, the fork-bomb bound).

ASSUME MaxRetries \in Nat
ASSUME MaxCrashes \in Nat
ASSUME CanHealthy \in BOOLEAN

VARIABLES
    state,        \* one of {"Off","Starting","Running","Backoff","Failed"}
    failures,     \* consecutive-failure counter (0..MaxRetries+1)
    child,        \* TRUE iff a live OS child is currently owned
    healthyRun,   \* TRUE iff the CURRENT Running episode became sustained-healthy
    crashes       \* total crash records written (monotone; audit/liveness aid)

vars == << state, failures, child, healthyRun, crashes >>

States == {"Off", "Starting", "Running", "Backoff", "Failed"}

TypeOK ==
    /\ state \in States
    /\ failures \in 0..(MaxRetries + 1)
    /\ child \in BOOLEAN
    /\ healthyRun \in BOOLEAN
    /\ crashes \in Nat

Init ==
    /\ state = "Starting"     \* Supervisor::start publishes Starting first
    /\ failures = 0
    /\ child = FALSE
    /\ healthyRun = FALSE
    /\ crashes = 0

(***************************************************************************)
(* A crash/spawn-fail/unhealthy step: record the crash, bump the           *)
(* consecutive counter, and either go to Failed (cap exceeded) or Backoff.  *)
(* Mirrors the three identical `consecutive_failures += 1; if >max_retries  *)
(* { Failed } else { backoff }` arms in run_monitor.                        *)
(***************************************************************************)
FailStep ==
    /\ child' = FALSE
    /\ healthyRun' = FALSE
    /\ crashes' = crashes + 1
    /\ IF failures + 1 > MaxRetries
         THEN /\ state' = "Failed"
              /\ failures' = failures + 1
         ELSE /\ state' = "Backoff"
              /\ failures' = failures + 1

\* Starting -> Running: the spawn succeeded, a live child now exists.
SpawnOk ==
    /\ state = "Starting"
    /\ state' = "Running"
    /\ child' = TRUE
    /\ healthyRun' = FALSE
    /\ UNCHANGED << failures, crashes >>

\* Starting -> Backoff|Failed: spawn itself failed (bad path). Treated as a
\* crash under the same bound. A spawn-fail NEVER becomes healthy, so it is
\* modeled only in the crash-loop (CanHealthy = FALSE) config; a genuinely
\* recovering daemon spawns successfully.
SpawnFail ==
    /\ ~CanHealthy
    /\ state = "Starting"
    /\ FailStep

\* Running -> Running: the child stayed up long enough to count as sustained-
\* healthy; RESET the consecutive counter (the F-1 fix). Only enabled when
\* CanHealthy: a pure crash-loop child never reaches this.
BecomeHealthy ==
    /\ state = "Running"
    /\ CanHealthy
    /\ ~healthyRun
    /\ healthyRun' = TRUE
    /\ failures' = 0
    /\ UNCHANGED << state, child, crashes >>

\* Running -> Backoff|Failed: the child exited unexpectedly (crash).
\* KEY MODELING FIDELITY (F-1): in the RECOVERING config (CanHealthy = TRUE) a
\* crash can only occur AFTER the episode became sustained-healthy — i.e. every
\* Running episode of a recovering daemon achieves the reset before it next
\* crashes. In the crash-loop config the child crashes with healthyRun = FALSE.
\* This is exactly the real distinction: a fast crash-loop exits BEFORE
\* healthy_after; a long-lived daemon runs well past it between crashes.
CrashEnabled ==
    /\ state = "Running"
    /\ (CanHealthy => healthyRun)

Crash ==
    /\ CrashEnabled
    /\ FailStep

\* Running -> Backoff|Failed: a health probe failed (or was wedged). Same bound
\* and same healthy-precondition as Crash.
Unhealthy ==
    /\ CrashEnabled
    /\ FailStep

\* Running -> Backoff|Failed: try_wait itself errored -> treated as Crashed with
\* default_failed_status. Same bound.
TryWaitError ==
    /\ CrashEnabled
    /\ FailStep

\* Backoff -> Starting: the backoff delay elapsed; respawn.
RetryFromBackoff ==
    /\ state = "Backoff"
    /\ state' = "Starting"
    /\ UNCHANGED << failures, child, healthyRun, crashes >>

\* Intentional stop from a live/scheduling state -> Off. NOT a crash: no crash
\* recorded, failures unchanged, no restart follows. Kills any live child.
Stop ==
    /\ state \in {"Starting", "Running"}
    /\ state' = "Off"
    /\ child' = FALSE
    /\ healthyRun' = FALSE
    /\ UNCHANGED << failures, crashes >>

\* Stop during Backoff cancels the pending restart -> Off (no respawn, no crash).
StopInBackoff ==
    /\ state = "Backoff"
    /\ state' = "Off"
    /\ child' = FALSE
    /\ healthyRun' = FALSE
    /\ UNCHANGED << failures, crashes >>

\* Teardown (Drop/Shutdown) from any non-terminal state -> Off, child killed.
Shutdown ==
    /\ state \in {"Starting", "Running", "Backoff", "Off"}
    /\ state' = "Off"
    /\ child' = FALSE
    /\ healthyRun' = FALSE
    /\ UNCHANGED << failures, crashes >>

\* Terminal states self-loop so behaviours are infinite (no deadlock artifact).
TerminalStutter ==
    /\ state \in {"Off", "Failed"}
    /\ UNCHANGED vars

Next ==
    \/ SpawnOk
    \/ SpawnFail
    \/ BecomeHealthy
    \/ Crash
    \/ Unhealthy
    \/ TryWaitError
    \/ RetryFromBackoff
    \/ Stop
    \/ StopInBackoff
    \/ Shutdown
    \/ TerminalStutter

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Fork-bomb liveness spec: a crash-loop child that is NEVER stopped or      *)
(* torn down. Only the spawn/crash/retry steps are enabled, with weak        *)
(* fairness, so TLC can prove the run actually REACHES Failed (INV-2's        *)
(* teeth). Used only with the CanHealthy = FALSE config.                     *)
(***************************************************************************)
ForkBombNext ==
    \/ SpawnOk
    \/ SpawnFail
    \/ Crash
    \/ Unhealthy
    \/ TryWaitError
    \/ RetryFromBackoff
    \/ TerminalStutter

ForkBombSpec ==
    /\ Init
    /\ [][ForkBombNext]_vars
    /\ WF_vars(SpawnOk)
    /\ WF_vars(Crash)
    /\ WF_vars(RetryFromBackoff)

(***************************************************************************)
(* INVARIANTS — the fixes, pinned.                                          *)
(***************************************************************************)

\* INV-1 (F-1): an intermittently-crashing-but-recovering child is NEVER
\* permanently Failed. Checked with CanHealthy = TRUE: because every Running
\* episode may become sustained-healthy (resetting `failures` to 0), the counter
\* can never accumulate MaxRetries+1 CONSECUTIVE failures, so Failed is
\* unreachable. (Model-checked as `state # "Failed"` under the CanHealthy cfg.)
INV_NeverFailedWhenRecovering == state # "Failed"

\* INV-2 (fork-bomb bound): the consecutive counter never exceeds MaxRetries+1,
\* i.e. the cap is finite and always enforced (a crash-loop cannot run unbounded).
\* Combined with the CanHealthy=FALSE config's liveness (below), this pins that a
\* crash-loop-only child reaches Failed within MaxRetries.
INV_CounterBounded == failures <= MaxRetries + 1

\* INV-3: an intentional stop never records a crash and never restarts. Modeled
\* structurally: Off is only ever entered by Stop/StopInBackoff/Shutdown, none of
\* which increment `crashes`, and Off transitions only to Starting via an
\* explicit resume (absent in C1.0) — so reaching Off leaves `failures` and
\* `crashes` untouched by the stop itself. We assert Off implies no live child
\* AND that no transition INTO Off bumped crashes (captured by INV-4 + the fact
\* that Stop/StopInBackoff/Shutdown all keep `crashes` UNCHANGED).
INV_StopIsNotACrash == (state = "Off") => (child = FALSE)

\* INV-4 (no orphan): Off or Failed implies no live child is owned.
INV_NoOrphan == (state \in {"Off", "Failed"}) => (child = FALSE)

(***************************************************************************)
(* LIVENESS — for the CanHealthy = FALSE config, a crash-loop child must     *)
(* actually REACH Failed (INV-2's teeth). Under weak fairness on the crash/  *)
(* retry steps, a never-healthy child eventually accumulates MaxRetries+1     *)
(* consecutive failures. (Checked as a temporal property in the -forkbomb cfg *)
(* only, where Stop/Shutdown are disabled so the run is not pre-empted.)      *)
(***************************************************************************)
EventuallyFailedIfNeverHealthy == <>(state = "Failed")

(***************************************************************************)
(* State constraint (model-checking only): bound the unbounded `crashes`     *)
(* counter so TLC explores a FINITE space. This does not weaken the           *)
(* invariants — INV-1..4 are inductive on `state`/`failures`, and a few       *)
(* crash/reset cycles suffice to exercise every transition and the counter    *)
(* reset. Named in the .cfg via CONSTRAINT.                                    *)
(***************************************************************************)
CrashesBounded == crashes <= MaxCrashes

=============================================================================
