------------------------------- MODULE AgentLoop -------------------------------
(***************************************************************************)
(* HUP-S1.3 — Hermes's verifier-judged agent loop (citrate-agent-loop        *)
(* `run_workflow` / `run_turn_with`, ADR loop-in-sidecar).                  *)
(*                                                                          *)
(* A workflow has NSteps steps; each step gets up to MaxAttempts attempts;  *)
(* each attempt is up to MaxTurns model calls. A model call either answers  *)
(* (then verifiers judge the attempt) or proposes an effect (a tool call).  *)
(* Effects pass a gate: HIC-1 (a human decides) or HIC-2 (an auto-approved  *)
(* budgeted kind). Reading untrusted content taints the task; after taint   *)
(* no effect may be auto-approved (red-team correction #3). A stop request  *)
(* may arrive at any time.                                                  *)
(*                                                                          *)
(* Properties:                                                              *)
(*   Bounded              model calls never exceed the static budget        *)
(*   OnlyVerifierSucceeds outcome = succeeded ⇒ every step was verified     *)
(*   NoEffectWithoutGate  every executed effect passed a gate, and a denied *)
(*                        effect never executes                             *)
(*   TaintDowngrade       no auto-approved effect after taint               *)
(*   StopIsLive           a stop request is always followed by halting      *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS NSteps, MaxAttempts, MaxTurns

VARIABLES
    step,        \* 1..NSteps (current step), NSteps+1 when all passed
    attempt,     \* 1..MaxAttempts
    turns,       \* model calls in the current attempt
    calls,       \* total model calls
    phase,       \* "model" | "effect" | "judge"
    outcome,     \* "running" | "succeeded" | "failed" | "stopped"
    verified,    \* [1..NSteps -> BOOLEAN]
    stopReq,     \* BOOLEAN — someone pressed Stop
    tainted,     \* BOOLEAN — untrusted content entered the context
    executedUngated,   \* count of effects executed without a gate (must stay 0)
    executedDenied,    \* count of denied effects that executed anyway (must stay 0)
    autoAfterTaint     \* count of auto-approved effects after taint (must stay 0)

vars == <<step, attempt, turns, calls, phase, outcome, verified, stopReq, tainted,
          executedUngated, executedDenied, autoAfterTaint>>

Steps == 1..NSteps

TypeOK ==
    /\ step \in 1..(NSteps + 1)
    /\ attempt \in 1..MaxAttempts
    /\ turns \in 0..MaxTurns
    /\ calls \in Nat
    /\ phase \in {"model", "effect", "judge"}
    /\ outcome \in {"running", "succeeded", "failed", "stopped"}
    /\ verified \in [Steps -> BOOLEAN]
    /\ stopReq \in BOOLEAN /\ tainted \in BOOLEAN
    /\ executedUngated \in Nat /\ executedDenied \in Nat /\ autoAfterTaint \in Nat

Init ==
    /\ step = 1 /\ attempt = 1 /\ turns = 0 /\ calls = 0 /\ phase = "model"
    /\ outcome = "running" /\ verified = [i \in Steps |-> FALSE]
    /\ stopReq = FALSE /\ tainted = FALSE
    /\ executedUngated = 0 /\ executedDenied = 0 /\ autoAfterTaint = 0

Running == outcome = "running"

\* The stop flag is checked before every model call and every effect (run_turn_with).
Halt ==
    /\ Running /\ stopReq
    /\ outcome' = "stopped"
    /\ UNCHANGED <<step, attempt, turns, calls, phase, verified, stopReq, tainted,
                   executedUngated, executedDenied, autoAfterTaint>>

RequestStop ==
    /\ ~stopReq
    /\ stopReq' = TRUE
    /\ UNCHANGED <<step, attempt, turns, calls, phase, outcome, verified, tainted,
                   executedUngated, executedDenied, autoAfterTaint>>

\* The model answers or proposes an effect.
ModelCall ==
    /\ Running /\ ~stopReq /\ phase = "model" /\ turns < MaxTurns
    /\ turns' = turns + 1 /\ calls' = calls + 1
    /\ phase' \in {"effect", "judge"}
    /\ UNCHANGED <<step, attempt, outcome, verified, stopReq, tainted,
                   executedUngated, executedDenied, autoAfterTaint>>

FailAttempt ==
    IF attempt < MaxAttempts
    THEN /\ attempt' = attempt + 1 /\ turns' = 0 /\ phase' = "model"
         /\ UNCHANGED <<step, calls, outcome, verified, stopReq, tainted,
                        executedUngated, executedDenied, autoAfterTaint>>
    ELSE /\ outcome' = "failed"
         /\ UNCHANGED <<step, attempt, turns, calls, phase, verified, stopReq, tainted,
                        executedUngated, executedDenied, autoAfterTaint>>

\* The attempt ran out of model calls without an answer: a failed attempt.
TurnLimit ==
    /\ Running /\ ~stopReq /\ phase = "model" /\ turns = MaxTurns
    /\ FailAttempt

\* An effect goes through a gate. Human-decided (HIC-1) effects may be approved or denied; a
\* denied effect is not executed. Auto (HIC-2) approval is only possible while untainted.
\* The tool result may carry untrusted content, which taints the task.
Effect ==
    /\ Running /\ ~stopReq /\ phase = "effect"
    /\ \E gate \in {"human-approve", "human-deny", "auto"}, untrusted \in BOOLEAN :
        /\ (gate = "auto") => ~tainted          \* TaintDowngrade: auto gate unavailable after taint
        /\ executedUngated' = executedUngated     \* every executed effect went through `gate`
        /\ executedDenied' = executedDenied       \* human-deny executes nothing
        /\ autoAfterTaint' = IF gate = "auto" /\ tainted THEN autoAfterTaint + 1 ELSE autoAfterTaint
        /\ tainted' = (tainted \/ untrusted)
    /\ phase' = "model"
    /\ UNCHANGED <<step, attempt, turns, calls, outcome, verified, stopReq>>

\* Verifiers judge the attempt (deterministic checks; modelled as either verdict).
Judge ==
    /\ Running /\ ~stopReq /\ phase = "judge"
    /\ \/ \* every verifier passed: the step is verified, move on
          /\ verified' = [verified EXCEPT ![step] = TRUE]
          /\ step' = step + 1 /\ attempt' = 1 /\ turns' = 0 /\ phase' = "model"
          /\ outcome' = IF step = NSteps THEN "succeeded" ELSE "running"
          /\ UNCHANGED <<calls, stopReq, tainted, executedUngated, executedDenied, autoAfterTaint>>
       \/ \* some verifier failed: retry with feedback, or fail the workflow
          FailAttempt

Done == ~Running /\ UNCHANGED vars

Next == Halt \/ RequestStop \/ ModelCall \/ TurnLimit \/ Effect \/ Judge \/ Done

Spec == Init /\ [][Next]_vars /\ WF_vars(Halt) /\ WF_vars(ModelCall) /\ WF_vars(TurnLimit)
                             /\ WF_vars(Effect) /\ WF_vars(Judge)

\* ---- properties ----
Bounded == calls <= NSteps * MaxAttempts * MaxTurns

OnlyVerifierSucceeds == (outcome = "succeeded") => \A i \in Steps : verified[i]

NoEffectWithoutGate == executedUngated = 0 /\ executedDenied = 0

TaintDowngrade == autoAfterTaint = 0

StopIsLive == stopReq ~> (outcome # "running")

Terminates == <>(outcome # "running")
================================================================================
