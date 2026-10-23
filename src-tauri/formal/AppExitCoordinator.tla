--------------------------- MODULE AppExitCoordinator ---------------------------
(***************************************************************************)
(* Formal model of the app exit coordinator (SCL SC7, D-8), planset        *)
(* 2026-10-05-sidecar-lifecycle 03_TLA_SPECS.md section 2 as corrected by   *)
(* the red-team pass (RT-01, RT-02, RT-08, RT-17, RT-26) and owner decision *)
(* D-15 ("Quit always ends"). SCL-S1.2 wrote the module; SCL-S1.7 added     *)
(* the runtime Closing gate, X-10 to X-12, the restated X-2, X-5, X-9, the  *)
(* reachability witnesses and the new mutants.                              *)
(*                                                                         *)
(* What it holds that SidecarSupervisor.tla cannot: several owners, the    *)
(* app gate {Open, Closing, Exited}, one absolute deadline, the exit        *)
(* triggers, the final action, and the Hermes runtime as a nested owner    *)
(* with its own admission gate.                                             *)
(*                                                                         *)
(* Refinement notes. Nothing here exists in code yet; the coordinator is    *)
(* SCL-S12. Each action names the work package that implements it and,     *)
(* where one exists, today's exit path in src-tauri/src/lib.rs. The WP that *)
(* lands the code updates the note in the same PR.                          *)
(*                                                                         *)
(* Abstractions:                                                            *)
(*  - An owner is one lifecycle cell (a SidecarSupervisor instance) or the  *)
(*    abstract bespoke registry. Its internal teardown is that module's     *)
(*    concern; here it is Running, Draining, Complete or Incomplete.        *)
(*  - "An owner stalls" is the absence of its progress: owner steps carry   *)
(*    no fairness, so TLC explores owners that never finish.               *)
(*  - Fairness IS assumed for the coordinator's own steps and for Tick. So  *)
(*    "Quit always ends" (X-11) is only as strong as the coordinator thread *)
(*    and the deadline publisher: in code (S12.1, S12.6) the coordinator    *)
(*    path never takes an owner or lifecycle lock, never joins, never       *)
(*    blocks on an owner, FinalActuation only signals and never waits on a  *)
(*    reap, and in the synchronous Terminate mode the main thread is free   *)
(*    of webview and IPC waits.                                             *)
(*  - The async (preventable request) and synchronous (Exit callback) entry *)
(*    modes of RT-01 run the same actions over the same owners and the same *)
(*    deadline; the model does not distinguish them beyond the Terminate    *)
(*    trigger, which cannot be prevented or cancelled.                      *)
(*  - ForceExit is crash and external SIGKILL (and SIGTERM, which core does *)
(*    not handle). Logout and Dock Quit reach the app as an unpreventable   *)
(*    Exit, so they are Terminate (RT-01). ForceExit sets reported in one   *)
(*    step; a killed process writes nothing, so in code the report comes    *)
(*    from a close-in-progress marker written before the drain (S12).       *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    Owners,          \* non-Hermes owners (model values)
    Hermes,          \* the Hermes sidecar owner (a model value not in Owners)
    DeadlineTicks,   \* the absolute close deadline, in ticks (15 s proposed)
    HermesSubTicks,  \* the Hermes-first sub-deadline (7 s proposed, RT-26)
    MaxTickets,      \* bound on admitted tickets per owner
    MaxRetry,        \* bound on explicit retries of a restart or install
    Mutant           \* "none" or a mutant name (see MUTANTS below)

All         == Owners \cup {Hermes}
QuitLike    == {"Quit", "WindowClose", "Reset", "Terminate"}
RestartLike == {"Restart", "InstallWin", "InstallUnix"}
Triggers    == QuitLike \cup RestartLike
Mutants     == {"none", "InstallWinNoClosing", "AdmitWhileClosing", "FinalAfterExpiry",
                "HermesCompleteEarly", "ReconnectAfterReport", "OthersFirst",
                "QuitWaitsForever", "TerminateNoDrain", "QuitNoActuation", "TickWaitsOnDispatch"}

ASSUME Hermes \notin Owners
ASSUME DeadlineTicks \in Nat \ {0} /\ HermesSubTicks \in 1..DeadlineTicks
ASSUME MaxTickets \in Nat /\ MaxRetry \in Nat
ASSUME Mutant \in Mutants

VARIABLES
    app,              \* "Open", "Closing", "Exited"
    ownerState,       \* [All -> {"Running","Draining","Complete","Incomplete"}]
    tickets,          \* [All -> 0..MaxTickets] admitted, not yet returned
    deadlineLeft,     \* ticks left on the armed absolute deadline
    expired,          \* the deadline publisher marked the close expired
    trigger,          \* the (joined) exit trigger, or "None"
    final,            \* "None", "Exit", "Restart", "Installer"
    finalLeft,        \* deadlineLeft at final-action admission
    reported,         \* "None", "Complete", "Incomplete" (next-launch report)
    hermesChildren,   \* "Running", "Complete" (the runtime's child report)
    runtimeAdmission, \* the runtime's own gate: "Open", "Closing"
    passedClosing,    \* history: the app went through Closing
    forced,           \* history: the process was killed (ForceExit)
    actuated,         \* owners that received the final actuation (D-15)
    retries           \* explicit retries of a restart or install

vars == << app, ownerState, tickets, deadlineLeft, expired, trigger, final, finalLeft,
           reported, hermesChildren, runtimeAdmission, passedClosing, forced, actuated,
           retries >>

Elapsed        == DeadlineTicks - deadlineLeft
SubPassed      == Elapsed >= HermesSubTicks
HermesSettled  == ownerState[Hermes] \in {"Complete", "Incomplete"}
AllComplete    == \A o \in All : ownerState[o] = "Complete"

TypeOK ==
    /\ app \in {"Open", "Closing", "Exited"}
    /\ ownerState \in [All -> {"Running", "Draining", "Complete", "Incomplete"}]
    /\ tickets \in [All -> 0..MaxTickets]
    /\ deadlineLeft \in 0..DeadlineTicks
    /\ expired \in BOOLEAN
    /\ trigger \in Triggers \cup {"None"}
    /\ final \in {"None", "Exit", "Restart", "Installer"}
    /\ finalLeft \in 0..DeadlineTicks
    /\ reported \in {"None", "Complete", "Incomplete"}
    /\ hermesChildren \in {"Running", "Complete"}
    /\ runtimeAdmission \in {"Open", "Closing"}
    /\ passedClosing \in BOOLEAN
    /\ forced \in BOOLEAN
    /\ actuated \subseteq All
    /\ retries \in 0..MaxRetry

Init ==
    /\ app = "Open"
    /\ ownerState = [o \in All |-> "Running"]
    /\ tickets = [o \in All |-> 0]
    /\ deadlineLeft = DeadlineTicks
    /\ expired = FALSE
    /\ trigger = "None"
    /\ final = "None"
    /\ finalLeft = 0
    /\ reported = "None"
    /\ hermesChildren = "Running"
    /\ runtimeAdmission = "Open"
    /\ passedClosing = FALSE
    /\ forced = FALSE
    /\ actuated = {}
    /\ retries = 0

(***************************************************************************)
(* Exit triggers and the app gate.                                          *)
(***************************************************************************)

\* Any exit path: last window close, Cmd+Q and the menu Quit, updater
\* restart, install on Windows or Unix, factory reset, and the
\* unpreventable Terminate (Dock Quit, logout, applicationWillTerminate).
\* Today: RunEvent::ExitRequested / RunEvent::Exit arms in lib.rs, the
\* process plugin's restart, the updater's install. Target: SCL-S12.1, S12.6.
\* Mutant "TerminateNoDrain": Terminate exits at once, with no drain.
Trigger(t) ==
    /\ app = "Open"
    /\ trigger = "None"
    /\ trigger' = t
    /\ IF Mutant = "TerminateNoDrain" /\ t = "Terminate"
       THEN /\ app' = "Exited"
            /\ final' = "Exit"
            /\ reported' = "Complete"
            /\ UNCHANGED << ownerState, tickets, deadlineLeft, expired, finalLeft,
                            hermesChildren, runtimeAdmission, passedClosing, forced,
                            actuated, retries >>
       ELSE UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, final, finalLeft,
                         reported, hermesChildren, runtimeAdmission, passedClosing,
                         forced, actuated, retries >>

\* Atomically close admission, then arm the absolute deadline before any
\* drain work. Target: SCL-S12.1.
BeginClosing ==
    /\ app = "Open"
    /\ trigger # "None"
    /\ app' = "Closing"
    /\ passedClosing' = TRUE
    /\ deadlineLeft' = DeadlineTicks
    /\ UNCHANGED << ownerState, tickets, expired, trigger, final, finalLeft, reported,
                    hermesChildren, runtimeAdmission, forced, actuated, retries >>

\* A trigger while Closing joins the close intent; it never starts a second
\* drain (X-6). Terminate dominates (it cannot be prevented); a Quit-like
\* trigger replaces a pending restart or install (D-15: Quit always ends).
RepeatTrigger(t) ==
    /\ app = "Closing"
    /\ trigger' = IF t = "Terminate" THEN "Terminate"
                  ELSE IF t \in QuitLike /\ trigger \in RestartLike THEN t
                  ELSE trigger
    /\ UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

(***************************************************************************)
(* Admission tickets (spawns, probes, lazy singletons, foreground work).    *)
(* Mutant "AdmitWhileClosing": an owner admits new work after Closing.      *)
(***************************************************************************)
AdmitTicket(o) ==
    /\ \/ app = "Open" /\ ownerState[o] = "Running"
       \/ Mutant = "AdmitWhileClosing" /\ app = "Closing"
          /\ ownerState[o] \in {"Running", "Draining"}
    /\ tickets[o] < MaxTickets
    /\ tickets' = [tickets EXCEPT ![o] = @ + 1]
    /\ UNCHANGED << app, ownerState, deadlineLeft, expired, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

\* An admitted ticket finishes, possibly after Closing (it stays owned).
TicketReturnsLate(o) ==
    /\ tickets[o] > 0
    /\ tickets' = [tickets EXCEPT ![o] = @ - 1]
    /\ UNCHANGED << app, ownerState, deadlineLeft, expired, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

(***************************************************************************)
(* Drain order: Hermes first, the rest at Hermes settlement or the Hermes  *)
(* sub-deadline, whichever comes first (O-11, RT-26).                       *)
(***************************************************************************)

\* Target: SCL-S12.1 (order), S11.6 (nested-owner stop profile).
DrainHermesFirst ==
    /\ app = "Closing"
    /\ ownerState[Hermes] = "Running"
    /\ ownerState' = [ownerState EXCEPT ![Hermes] = "Draining"]
    /\ UNCHANGED << app, tickets, deadlineLeft, expired, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

\* Mutant "OthersFirst": the other owners start draining with Hermes.
DrainOthers ==
    /\ app = "Closing"
    /\ HermesSettled \/ SubPassed \/ Mutant = "OthersFirst"
    /\ \E o \in Owners : ownerState[o] = "Running"
    /\ ownerState' = [o \in All |->
                        IF o \in Owners /\ ownerState[o] = "Running"
                        THEN "Draining" ELSE ownerState[o]]
    /\ UNCHANGED << app, tickets, deadlineLeft, expired, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

(***************************************************************************)
(* The Hermes runtime as a nested owner (RT-08, Delta-5). The runtime      *)
(* closes its own admission (no child spawn, MCP reconnect, lazy browser or *)
(* search start) before it takes the child report.                          *)
(* Target: citrate-agent-runtime, SCL-S11.6.                                *)
(***************************************************************************)

RuntimeClosing ==
    /\ ownerState[Hermes] = "Draining"
    /\ runtimeAdmission = "Open"
    /\ runtimeAdmission' = "Closing"
    /\ UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, trigger, final,
                    finalLeft, reported, hermesChildren, passedClosing, forced, actuated,
                    retries >>

\* The runtime admits a child (an MCP reconnect, a turn reaching a shell,
\* a lazy browser start). This stays possible after the app gate closes and
\* until RuntimeClosing: the runtime is another process with its own gate,
\* and that window is by design; the child report is what must wait for it. Mutant "ReconnectAfterReport": admitted after the
\* runtime's Closing gate, so after the child report.
RuntimeAdmit ==
    /\ app # "Exited"
    /\ runtimeAdmission = "Open" \/ Mutant = "ReconnectAfterReport"
    /\ hermesChildren' = "Running"
    /\ UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, trigger, final,
                    finalLeft, reported, runtimeAdmission, passedClosing, forced, actuated,
                    retries >>

\* The runtime's children reach Complete (the child report).
ChildrenDrain ==
    /\ runtimeAdmission = "Closing"
    /\ hermesChildren = "Running"
    /\ hermesChildren' = "Complete"
    /\ UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, trigger, final,
                    finalLeft, reported, runtimeAdmission, passedClosing, forced, actuated,
                    retries >>

(***************************************************************************)
(* Owner completion. Complete only with no outstanding ticket (X-4), and   *)
(* for Hermes only with the runtime closed and its children Complete (X-7). *)
(* Mutant "HermesCompleteEarly": Hermes reports Complete while children run.*)
(* Target: SCL-S12.1 (owner receipts), S11.6 (Hermes child report).         *)
(***************************************************************************)
OwnerComplete(o) ==
    /\ app = "Closing"
    /\ ownerState[o] = "Draining"
    /\ tickets[o] = 0
    /\ o = Hermes =>
         /\ runtimeAdmission = "Closing"
         /\ hermesChildren = "Complete" \/ Mutant = "HermesCompleteEarly"
    /\ ownerState' = [ownerState EXCEPT ![o] = "Complete"]
    /\ UNCHANGED << app, tickets, deadlineLeft, expired, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

(***************************************************************************)
(* The deadline. Armed at BeginClosing, it ticks on its own for the whole  *)
(* close, whatever the coordinator has dispatched: a drain dispatch delayed *)
(* past the deadline does not stop it (X-13). The drain order (X-9) and the *)
(* final actions then handle an expiry that comes before any dispatch.     *)
(* Mutant "TickWaitsOnDispatch": ticks only once every owner whose turn has *)
(* come was told to drain (the model before the Codex review at ebf978c).   *)
(* Target: SCL-S2.4 (independent deadline publisher), S12.1.               *)
(***************************************************************************)
Tick ==
    /\ app = "Closing"
    /\ deadlineLeft > 0
    /\ Mutant = "TickWaitsOnDispatch" =>
         /\ ownerState[Hermes] # "Running"
         /\ ~(SubPassed /\ \E o \in Owners : ownerState[o] = "Running")
    /\ deadlineLeft' = deadlineLeft - 1
    /\ UNCHANGED << app, ownerState, tickets, expired, trigger, final, finalLeft, reported,
                    hermesChildren, runtimeAdmission, passedClosing, forced, actuated,
                    retries >>

\* The publisher marks the close expired at the deadline.
Expire ==
    /\ app = "Closing"
    /\ deadlineLeft = 0
    /\ ~expired
    /\ expired' = TRUE
    /\ UNCHANGED << app, ownerState, tickets, deadlineLeft, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

(***************************************************************************)
(* Restart and install: the final action needs every owner Complete inside  *)
(* the deadline, compared directly at admission (X-2, X-5). On expiry the  *)
(* app stays up and reports Incomplete; only an explicit retry re-arms.     *)
(* Mutant "FinalAfterExpiry": a late completion authorizes the action.      *)
(* Mutant "InstallWinNoClosing": the Windows updater exits directly.        *)
(* Today: the updater plugin's install, request_restart. Target: S12.2.     *)
(***************************************************************************)
FinalRestart ==
    /\ app = "Closing"
    /\ trigger \in RestartLike
    /\ reported = "None"
    /\ AllComplete
    /\ deadlineLeft > 0 \/ Mutant = "FinalAfterExpiry"
    /\ app' = "Exited"
    /\ final' = IF trigger = "Restart" THEN "Restart" ELSE "Installer"
    /\ finalLeft' = deadlineLeft
    /\ reported' = "Complete"
    /\ UNCHANGED << ownerState, tickets, deadlineLeft, expired, trigger, hermesChildren,
                    runtimeAdmission, passedClosing, forced, actuated, retries >>

\* COVERAGE-EXEMPT: UpdaterDirectExit (enabled only under its mutant)
UpdaterDirectExit ==
    /\ Mutant = "InstallWinNoClosing"
    /\ app = "Open"
    /\ trigger = "InstallWin"
    /\ app' = "Exited"
    /\ final' = "Installer"
    /\ finalLeft' = deadlineLeft
    /\ UNCHANGED << ownerState, tickets, deadlineLeft, expired, trigger, reported,
                    hermesChildren, runtimeAdmission, passedClosing, forced, actuated,
                    retries >>

ReportIncomplete ==
    /\ app = "Closing"
    /\ trigger \in RestartLike
    /\ expired
    /\ reported = "None"
    /\ reported' = "Incomplete"
    /\ UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, trigger, final,
                    finalLeft, hermesChildren, runtimeAdmission, passedClosing, forced,
                    actuated, retries >>

RetryFinal ==
    /\ app = "Closing"
    /\ trigger \in RestartLike
    /\ reported = "Incomplete"
    /\ retries < MaxRetry
    /\ reported' = "None"
    /\ expired' = FALSE
    /\ deadlineLeft' = DeadlineTicks
    /\ retries' = retries + 1
    /\ UNCHANGED << app, ownerState, tickets, trigger, final, finalLeft, hermesChildren,
                    runtimeAdmission, passedClosing, forced, actuated >>

(***************************************************************************)
(* Quit, window close, factory reset and Terminate always end (D-15): at    *)
(* the deadline every OS-process scope still held gets a final actuation,   *)
(* then the app exits and reports Incomplete. In-process threads never keep *)
(* it alive. Mutant "QuitNoActuation": exit at the deadline without the    *)
(* actuation. Mutant "QuitWaitsForever": Quit waits for Complete.           *)
(* Target: SCL-S12.7 (direct children in v0.5.1; group and Job with S9,    *)
(* S10).                                                                    *)
(***************************************************************************)
FinalActuation(o) ==
    /\ app = "Closing"
    /\ expired
    /\ trigger \in QuitLike
    /\ ownerState[o] \in {"Running", "Draining"}
    /\ actuated' = actuated \cup {o}
    /\ ownerState' = [ownerState EXCEPT ![o] = "Incomplete"]
    /\ UNCHANGED << app, tickets, deadlineLeft, expired, trigger, final, finalLeft,
                    reported, hermesChildren, runtimeAdmission, passedClosing, forced,
                    retries >>

FinalQuit ==
    /\ app = "Closing"
    /\ trigger \in QuitLike
    /\ \/ /\ AllComplete
          /\ (deadlineLeft > 0 /\ ~expired) \/ Mutant = "QuitWaitsForever"
          /\ reported' = "Complete"
       \/ /\ Mutant # "QuitWaitsForever"
          /\ expired
          /\ (\A o \in All : ownerState[o] \in {"Complete", "Incomplete"})
             \/ Mutant = "QuitNoActuation"
          /\ reported' = "Incomplete"
    /\ app' = "Exited"
    /\ final' = "Exit"
    /\ finalLeft' = deadlineLeft
    /\ UNCHANGED << ownerState, tickets, deadlineLeft, expired, trigger, hermesChildren,
                    runtimeAdmission, passedClosing, forced, actuated, retries >>

(***************************************************************************)
(* Crash or external kill: never Complete (X-8).                            *)
(***************************************************************************)
ForceExit ==
    /\ app # "Exited"
    /\ app' = "Exited"
    /\ forced' = TRUE
    /\ reported' = "Incomplete"
    /\ UNCHANGED << ownerState, tickets, deadlineLeft, expired, trigger, final, finalLeft,
                    hermesChildren, runtimeAdmission, passedClosing, actuated, retries >>

ExitedStutter ==
    /\ app = "Exited"
    /\ UNCHANGED vars

Next ==
    \/ \E t \in Triggers : Trigger(t) \/ RepeatTrigger(t)
    \/ BeginClosing
    \/ \E o \in All : AdmitTicket(o) \/ TicketReturnsLate(o) \/ OwnerComplete(o)
                      \/ FinalActuation(o)
    \/ DrainHermesFirst
    \/ DrainOthers
    \/ RuntimeClosing
    \/ RuntimeAdmit
    \/ ChildrenDrain
    \/ Tick
    \/ Expire
    \/ FinalRestart
    \/ UpdaterDirectExit
    \/ ReportIncomplete
    \/ RetryFinal
    \/ FinalQuit
    \/ ForceExit
    \/ ExitedStutter

\* Fairness only on the coordinator's own steps and on time (Tick). Owners,
\* tickets, the runtime and the environment have none: they may stall.
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(BeginClosing)
    /\ WF_vars(DrainHermesFirst)
    /\ WF_vars(DrainOthers)
    /\ WF_vars(Tick)
    /\ WF_vars(Expire)
    /\ WF_vars(\E o \in All : FinalActuation(o))
    /\ WF_vars(FinalQuit)
    /\ WF_vars(FinalRestart)
    /\ WF_vars(ReportIncomplete)

(***************************************************************************)
(* INVARIANTS AND PROPERTIES (03 section 2, normative restatements).       *)
(***************************************************************************)

\* X-1: no final action and no exit without passing Closing (ForceExit aside).
X1_EveryExitPathRoutes ==
    /\ final # "None" => passedClosing
    /\ (app = "Exited" /\ ~forced) => passedClosing

\* X-2 (restated, D-15): Restart and Installer need every owner Complete,
\* admitted with deadline left. It does not constrain final = Exit.
X2_NoFinalBeforeComplete ==
    final \in {"Restart", "Installer"} => (AllComplete /\ finalLeft > 0)

\* X-3: while Closing no owner can admit a ticket.
X3_ClosingRejectsAdmission ==
    app = "Closing" => ~(\E o \in All : ENABLED AdmitTicket(o))

\* X-4: an owner is Complete only with no outstanding ticket.
X4_AdmittedTicketsDrained ==
    \A o \in All : ownerState[o] = "Complete" => tickets[o] = 0

\* X-5 (restated, D-15): after expiry no Restart or Installer action can be
\* admitted until an explicit retry re-arms the deadline.
X5_ExpiredNeverAuthorizes == expired => ~ENABLED FinalRestart

\* X-7: Hermes is Complete only with its runtime closed and children Complete.
X7_HermesChildrenScope ==
    ownerState[Hermes] = "Complete" =>
        (hermesChildren = "Complete" /\ runtimeAdmission = "Closing")

\* X-8: a forced exit is never reported Complete.
X8_ForceExitNeverComplete == forced => reported # "Complete"

\* X-10 (RT-01): Terminate (Dock Quit, logout) cannot be prevented: it runs
\* the drain over the same owners and deadline, and never reports Complete
\* for an owner it did not observe Complete.
X10_UnpreventableExitDrains ==
    (app = "Exited" /\ trigger = "Terminate" /\ ~forced) =>
        /\ passedClosing
        /\ \A o \in All : ownerState[o] # "Running"
        /\ reported = "Complete" => AllComplete

\* X-11, safety half: Complete is reported only when every owner and the
\* Hermes children are Complete.
X11_CompleteIsHonest ==
    reported = "Complete" => (AllComplete /\ hermesChildren = "Complete")

\* X-13 (Codex review of core#345 at ebf978c): the absolute deadline runs on
\* its own. While Closing with time left, a tick is always possible, whatever
\* the coordinator has or has not dispatched (SPEC-001: the deadline is armed
\* at admission and published by its own thread; it never waits on drain
\* progress). Mutant "TickWaitsOnDispatch" gates the tick on drain dispatch.
X13_DeadlineIndependentOfDispatch ==
    (app = "Closing" /\ deadlineLeft > 0) => ENABLED Tick

\* X-12 (D-15): an exit after an expired Quit-like close actuated every
\* owner that was not Complete, and reports Incomplete.
X12_QuitActuatesHeldScopes ==
    (app = "Exited" /\ ~forced /\ trigger \in QuitLike /\ expired) =>
        /\ \A o \in All : ownerState[o] = "Complete" \/ o \in actuated
        /\ reported = "Incomplete"

\* X-6 (action): a repeated trigger while Closing changes only the joined
\* trigger.
X6_NoRecursiveDrain ==
    [][(\E t \in Triggers : RepeatTrigger(t)) =>
          UNCHANGED << app, ownerState, tickets, deadlineLeft, expired, final, finalLeft,
                       reported, hermesChildren, runtimeAdmission, actuated, retries >>]_vars

\* X-9 (action, RT-26): no non-Hermes owner enters Draining before Hermes is
\* Complete or Incomplete or the Hermes sub-deadline has passed.
X9_HermesDrainsFirst ==
    [][(\E o \in Owners : ownerState[o] = "Running" /\ ownerState'[o] = "Draining")
          => (HermesSettled \/ SubPassed)]_vars

\* X-11, liveness half (D-15): under fairness of Tick and the coordinator,
\* every Quit-like trigger ends with the app Exited and a report. No
\* fairness is assumed for owners or the runtime; the coordinator's own
\* progress is an obligation on the code (see the header).
X11_QuitAlwaysTerminates ==
    \A t \in QuitLike :
        (trigger = t) ~> (app = "Exited" /\ reported \in {"Complete", "Incomplete"})

\* L-1 (restated): a restart or install either exits or reports Incomplete.
L1_CloseTerminates ==
    \A t \in RestartLike : (trigger = t) ~> (app = "Exited" \/ reported = "Incomplete")

(***************************************************************************)
(* REACHABILITY WITNESSES (RT-17). Each must be reported violated on the   *)
(* green config, which shows its mutant's action site is reachable there.  *)
(***************************************************************************)

\* InstallWinNoClosing: the Windows install path runs and completes.
W_InstallWinCompletes == ~(trigger = "InstallWin" /\ final = "Installer")

\* AdmitWhileClosing: a ticket admitted while Open is still owned by a
\* Draining owner during Closing (where a new admission would land).
W_TicketOwnedWhileDraining ==
    ~(app = "Closing" /\ \E o \in All : tickets[o] > 0 /\ ownerState[o] = "Draining")

\* FinalAfterExpiry: every owner completes late, after expiry, on a restart.
W_LateCompletionAfterExpiry ==
    ~(app = "Closing" /\ expired /\ AllComplete /\ trigger \in RestartLike)

\* HermesCompleteEarly: Hermes is draining with its runtime closed, no
\* ticket, and children still running.
W_HermesDrainingChildrenRun ==
    ~(/\ ownerState[Hermes] = "Draining" /\ runtimeAdmission = "Closing"
      /\ tickets[Hermes] = 0 /\ hermesChildren = "Running")

\* ReconnectAfterReport: the report was taken and Hermes is Complete while
\* the app is still closing (the window a late reconnect would hit).
W_HermesCompleteWhileClosing ==
    ~(app = "Closing" /\ ownerState[Hermes] = "Complete" /\ hermesChildren = "Complete")

\* OthersFirst: the sub-deadline releases the others while Hermes drains.
W_OthersDrainWhileHermesDrains ==
    ~(ownerState[Hermes] = "Draining" /\ \E o \in Owners : ownerState[o] = "Draining")

\* QuitWaitsForever: an expired Quit or Reset exits with Incomplete.
W_ExpiredQuitExits ==
    ~(app = "Exited" /\ trigger \in {"Quit", "Reset"} /\ reported = "Incomplete" /\ ~forced)

\* TerminateNoDrain: a Terminate exits through the drain.
W_TerminateExits == ~(app = "Exited" /\ trigger = "Terminate" /\ ~forced)

\* QuitNoActuation: the final actuation fires.
W_FinalActuationFires == actuated = {}

\* TickWaitsOnDispatch: the app is Closing and Hermes has not yet been told
\* to drain (where a dispatch-gated tick would stop the deadline).
W_ClosingBeforeDispatch == ~(app = "Closing" /\ ownerState[Hermes] = "Running")

\* The race itself (Codex, ebf978c): the deadline expires before the drain
\* is dispatched. X-5 then refuses a restart or install, and X-11 / X-12
\* still end a Quit with actuation and an Incomplete report.
W_ExpiredBeforeDispatch ==
    ~(app = "Closing" /\ expired /\ ownerState[Hermes] = "Running")

=============================================================================
