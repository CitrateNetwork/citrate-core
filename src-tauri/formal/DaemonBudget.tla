----------------------------- MODULE DaemonBudget -----------------------------
(***************************************************************************)
(* HUP-S10.3 — daemons: scheduled Hermes runs inside a budget              *)
(* (src-tauri/src/daemons.rs DaemonBook, src/daemons/runner.ts,            *)
(* src/daemons/turn.ts).                                                   *)
(*                                                                         *)
(* What is modelled                                                        *)
(*   - Fire(d): a schedule minute of daemon d passes (marks it due).       *)
(*   - Claim(d): the runner's tick claims a due daemon: only when not      *)
(*     paused (one or all), no run in flight, and the day's run and token  *)
(*     budgets are not used up. It counts one run and grants an allowance  *)
(*     min(PerRun, MaxTokens - tokens). A due daemon that cannot be        *)
(*     claimed is skipped (the due mark is consumed, nothing runs).        *)
(*   - Finish(d, used): the run reports its tokens. The runner stops a run *)
(*     once its estimate passes the allowance, so `used` is at most the    *)
(*     allowance plus one model round (Over).                              *)
(*   - Abandon(d): a run never reported back is released and charged its   *)
(*     full allowance.                                                     *)
(*   - Pause/Resume one daemon, PauseAll/ResumeAll. Pausing does not stop  *)
(*     the bookkeeping of a run in flight; the runner stops the run and    *)
(*     Finish still records it.                                            *)
(*   - Propose(d): a running daemon's turn proposes an effectful tool call *)
(*     (a write or a signature). It is pending until the member Approves   *)
(*     or Declines it; only Approve executes it. There is no budget path   *)
(*     for an effect (HIC-required, never automatic).                      *)
(*   - Spend: there is no action that spends; SpendCap is 0.              *)
(*   - NewDay: the local day rolls over and today's counters reset.       *)
(*                                                                         *)
(* Abstractions: schedules are folded into Fire; time-of-day, the 24 h     *)
(* catch-up window and the stale-run timer are folded into the            *)
(* nondeterministic Fire/Abandon. Token amounts are small naturals.        *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Daemons,      \* the daemons
    MaxRuns,      \* runs per day
    MaxTokens,    \* tokens per day
    PerRun,       \* tokens per run (<= MaxTokens)
    Over,         \* the most one model round can overshoot an allowance
    MaxEffects,   \* bound on proposed effects (state space)
    MaxDays       \* bound on day rollovers (state space)

ASSUME PerRun <= MaxTokens /\ PerRun >= 1 /\ MaxRuns >= 1

VARIABLES
    due,          \* d -> a schedule minute passed and was not yet considered
    paused,       \* d -> paused by the member
    allPaused,    \* "pause all"
    running,      \* d -> number of runs in flight (0 or 1)
    allowance,    \* d -> the in-flight run's token allowance
    runs,         \* d -> runs claimed today
    tokens,       \* d -> tokens charged today
    spent,        \* d -> SALT spent today
    startedWhilePaused, \* ghost: a run was claimed while its daemon was paused
    effects,      \* set of [id, d, state] records: "pending" | "approved" | "declined"
    executed,     \* ids of effects that ran
    nextId, day

vars == <<due, paused, allPaused, running, allowance, runs, tokens, spent,
          startedWhilePaused, effects, executed, nextId, day>>

SpendCap == 0

Init ==
    /\ due = [d \in Daemons |-> FALSE]
    /\ paused = [d \in Daemons |-> FALSE]
    /\ allPaused = FALSE
    /\ running = [d \in Daemons |-> 0]
    /\ allowance = [d \in Daemons |-> 0]
    /\ runs = [d \in Daemons |-> 0]
    /\ tokens = [d \in Daemons |-> 0]
    /\ spent = [d \in Daemons |-> 0]
    /\ startedWhilePaused = FALSE
    /\ effects = {}
    /\ executed = {}
    /\ nextId = 0
    /\ day = 0

Fire(d) ==
    /\ ~due[d]
    /\ due' = [due EXCEPT ![d] = TRUE]
    /\ UNCHANGED <<paused, allPaused, running, allowance, runs, tokens, spent,
                   startedWhilePaused, effects, executed, nextId, day>>

Min(a, b) == IF a < b THEN a ELSE b

CanClaim(d) ==
    /\ ~paused[d] /\ ~allPaused
    /\ running[d] = 0
    /\ runs[d] < MaxRuns
    /\ tokens[d] < MaxTokens

Claim(d) ==
    /\ due[d]
    /\ CanClaim(d)
    /\ due' = [due EXCEPT ![d] = FALSE]
    /\ running' = [running EXCEPT ![d] = running[d] + 1]
    /\ allowance' = [allowance EXCEPT ![d] = Min(PerRun, MaxTokens - tokens[d])]
    /\ runs' = [runs EXCEPT ![d] = runs[d] + 1]
    /\ startedWhilePaused' = (startedWhilePaused \/ paused[d] \/ allPaused)
    /\ UNCHANGED <<paused, allPaused, tokens, spent, effects, executed, nextId, day>>

\* A due daemon that cannot run (paused, busy or out of budget) is skipped.
Skip(d) ==
    /\ due[d]
    /\ ~CanClaim(d)
    /\ due' = [due EXCEPT ![d] = FALSE]
    /\ UNCHANGED <<paused, allPaused, running, allowance, runs, tokens, spent,
                   startedWhilePaused, effects, executed, nextId, day>>

Finish(d, used) ==
    /\ running[d] = 1
    /\ used <= allowance[d] + Over
    /\ running' = [running EXCEPT ![d] = 0]
    /\ tokens' = [tokens EXCEPT ![d] = tokens[d] + used]
    /\ allowance' = [allowance EXCEPT ![d] = 0]
    /\ UNCHANGED <<due, paused, allPaused, runs, spent, startedWhilePaused,
                   effects, executed, nextId, day>>

Abandon(d) ==
    /\ running[d] = 1
    /\ running' = [running EXCEPT ![d] = 0]
    /\ tokens' = [tokens EXCEPT ![d] = tokens[d] + allowance[d]]
    /\ allowance' = [allowance EXCEPT ![d] = 0]
    /\ UNCHANGED <<due, paused, allPaused, runs, spent, startedWhilePaused,
                   effects, executed, nextId, day>>

SetPaused(d, p) ==
    /\ paused' = [paused EXCEPT ![d] = p]
    \* Resuming never replays missed minutes.
    /\ due' = IF p THEN due ELSE [due EXCEPT ![d] = FALSE]
    /\ UNCHANGED <<allPaused, running, allowance, runs, tokens, spent,
                   startedWhilePaused, effects, executed, nextId, day>>

SetAllPaused(p) ==
    /\ allPaused' = p
    /\ due' = IF p THEN due ELSE [d \in Daemons |-> FALSE]
    /\ UNCHANGED <<paused, running, allowance, runs, tokens, spent,
                   startedWhilePaused, effects, executed, nextId, day>>

Propose(d) ==
    /\ running[d] = 1
    /\ nextId < MaxEffects
    /\ effects' = effects \cup {[id |-> nextId, d |-> d, state |-> "pending"]}
    /\ nextId' = nextId + 1
    /\ UNCHANGED <<due, paused, allPaused, running, allowance, runs, tokens, spent,
                   startedWhilePaused, executed, day>>

Decide(e, verdict) ==
    /\ e \in effects
    /\ e.state = "pending"
    /\ effects' = (effects \ {e}) \cup {[e EXCEPT !.state = verdict]}
    /\ executed' = IF verdict = "approved" THEN executed \cup {e.id} ELSE executed
    /\ UNCHANGED <<due, paused, allPaused, running, allowance, runs, tokens, spent,
                   startedWhilePaused, nextId, day>>

NewDay ==
    /\ day < MaxDays
    /\ day' = day + 1
    /\ runs' = [d \in Daemons |-> 0]
    /\ tokens' = [d \in Daemons |-> 0]
    /\ spent' = [d \in Daemons |-> 0]
    /\ UNCHANGED <<due, paused, allPaused, running, allowance,
                   startedWhilePaused, effects, executed, nextId>>

Next ==
    \/ \E d \in Daemons :
         \/ Fire(d) \/ Claim(d) \/ Skip(d) \/ Abandon(d) \/ Propose(d)
         \/ \E u \in 0..(PerRun + Over) : Finish(d, u)
         \/ \E p \in BOOLEAN : SetPaused(d, p)
    \/ \E p \in BOOLEAN : SetAllPaused(p)
    \/ \E e \in effects : \E v \in {"approved", "declined"} : Decide(e, v)
    \/ NewDay

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
TypeOK ==
    /\ due \in [Daemons -> BOOLEAN]
    /\ paused \in [Daemons -> BOOLEAN]
    /\ allPaused \in BOOLEAN
    /\ running \in [Daemons -> 0..2]
    /\ runs \in [Daemons -> Nat]
    /\ tokens \in [Daemons -> Nat]
    /\ spent \in [Daemons -> Nat]
    /\ executed \subseteq 0..MaxEffects

\* The day's run budget is never exceeded.
RunsWithinCap == \A d \in Daemons : runs[d] <= MaxRuns

\* The day's token budget is exceeded by at most one model round.
TokensBounded == \A d \in Daemons : tokens[d] <= MaxTokens + Over

\* No run's allowance goes past what is left of the day.
AllowanceWithinDay == \A d \in Daemons : running[d] = 1 => tokens[d] + allowance[d] <= MaxTokens

\* A run never starts with nothing left to spend on tokens.
NoEmptyRun == \A d \in Daemons : running[d] = 1 => allowance[d] >= 1

\* A daemon never spends.
SpendZero == \A d \in Daemons : spent[d] <= SpendCap

\* One run in flight per daemon.
OneInFlight == \A d \in Daemons : running[d] <= 1

\* A paused daemon (or "pause all") never starts a run.
NoStartWhilePaused == ~startedWhilePaused

\* Nothing a daemon proposes runs without the member's approval.
EffectsOnlyApproved ==
    \A i \in executed : \E e \in effects : e.id = i /\ e.state = "approved"
=============================================================================
