------------------------------ MODULE SpendBudget ------------------------------
(***************************************************************************)
(* HUP-S1.5 — the escalation spend budget (src-tauri/src/escalation.rs,    *)
(* `Book` + `Ledger`; the sidecar half is citrate-agent-runtime            *)
(* agent-escalation).                                                      *)
(*                                                                         *)
(* What is modelled                                                        *)
(*   - member endpoints: added (opted in) and removed. Removing voids its *)
(*     quotes (Book::remove_endpoint).                                     *)
(*   - quotes: Quote prices a request (a cost from Costs) for an added    *)
(*     endpoint; Show is the webview displaying that price. A quote id    *)
(*     is used once (authorize removes it).                                *)
(*   - the agent context's taint (Ingest / NewTask).                       *)
(*   - RunBudget (HIC-2, within today's cap, untainted) and RunConfirmed  *)
(*     (HIC-1, the member's explicit decision; outside the cap). Both     *)
(*     require the shown price to equal the quote and the endpoint to     *)
(*     still be added. RunBudget reserves write-ahead.                     *)
(*   - Settle charges 0..reserved; a reservation from an earlier period   *)
(*     never touches the current counters.                                 *)
(*   - the wall clock moving forward and backward; Roll advances the      *)
(*     period only to a LATER day. Every budget step runs after the roll  *)
(*     (authorize/settle call Ledger::roll first), modelled as the guard  *)
(*     period = now.                                                       *)
(*   - SetCap: the member changes the daily cap, never below today's use. *)
(*                                                                         *)
(* Abstractions                                                            *)
(*   - prices are abstract integers; the worst-case arithmetic and its    *)
(*     overflow checks are unit-tested in Rust.                            *)
(*   - persistence is assumed write-ahead and durable (a restart reloads  *)
(*     the same counters); an unreadable ledger is the tainted path (it   *)
(*     forces confirmation) and is unit-tested.                            *)
(*   - quote expiry only removes quotes (a subset of RemoveEndpoint's     *)
(*     effect on the quote set) and is unit-tested.                        *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Endpoints,  \* member endpoint ids
    QIds,       \* quote ids (bounds the number of escalations)
    Costs,      \* worst-case prices a quote can carry (positive)
    Caps,       \* daily caps the member can choose
    MaxDay,     \* the clock ranges over 0..MaxDay
    NoPrice     \* "no price shown yet"

ASSUME NoPrice \notin Costs /\ \A c \in Costs : c > 0

Modes == {"budget", "confirmed"}

VARIABLES
    now,            \* the wall clock's UTC day
    period,         \* the ledger's period (never decreases)
    cap,            \* today's cap
    committed,      \* settled budget spend this period
    reserved,       \* budget reservations in flight this period
    added,          \* endpoints the member opted in to
    quotes,         \* outstanding quotes: [id, ep, cost]
    shown,          \* shown[q] = the price the webview displayed for q, or NoPrice
    used,           \* quote ids already issued
    tainted,        \* the agent's context holds untrusted content
    res,            \* reservations in flight: [id, amt, per, mode]
    esc             \* every run: [id, ep, cost, shownAt, mode, optedIn, fit, taint]

vars == <<now, period, cap, committed, reserved, added, quotes, shown, used, tainted, res, esc>>

QuoteRec == [id : QIds, ep : Endpoints, cost : Costs]
ResRec == [id : QIds, amt : Costs, per : 0..MaxDay, mode : Modes]
EscRec == [id : QIds, ep : Endpoints, cost : Costs, shownAt : Costs \cup {NoPrice},
           mode : Modes, optedIn : BOOLEAN, fit : BOOLEAN, taint : BOOLEAN]

MaxSpend == Cardinality(QIds) * 10

TypeOK ==
    /\ now \in 0..MaxDay
    /\ period \in 0..MaxDay
    /\ cap \in Caps
    /\ committed \in 0..MaxSpend
    /\ reserved \in 0..MaxSpend
    /\ added \subseteq Endpoints
    /\ quotes \subseteq QuoteRec
    /\ shown \in [QIds -> Costs \cup {NoPrice}]
    /\ used \subseteq QIds
    /\ tainted \in BOOLEAN
    /\ res \subseteq ResRec
    /\ esc \subseteq EscRec

Init ==
    /\ now = 0
    /\ period = 0
    /\ cap \in Caps
    /\ committed = 0
    /\ reserved = 0
    /\ added = {}
    /\ quotes = {}
    /\ shown = [q \in QIds |-> NoPrice]
    /\ used = {}
    /\ tainted = FALSE
    /\ res = {}
    /\ esc = {}

-----------------------------------------------------------------------------
\* Endpoints and taint

AddEndpoint(e) ==
    /\ e \notin added
    /\ added' = added \cup {e}
    /\ UNCHANGED <<now, period, cap, committed, reserved, quotes, shown, used, tainted, res, esc>>

RemoveEndpoint(e) ==
    /\ e \in added
    /\ added' = added \ {e}
    /\ quotes' = {q \in quotes : q.ep # e}
    /\ UNCHANGED <<now, period, cap, committed, reserved, shown, used, tainted, res, esc>>

Ingest ==
    /\ tainted' = TRUE
    /\ UNCHANGED <<now, period, cap, committed, reserved, added, quotes, shown, used, res, esc>>

NewTask ==
    /\ tainted' = FALSE
    /\ UNCHANGED <<now, period, cap, committed, reserved, added, quotes, shown, used, res, esc>>

-----------------------------------------------------------------------------
\* Quotes and the shown price

Quote(id, e, c) ==
    /\ id \notin used
    /\ e \in added
    /\ quotes' = quotes \cup {[id |-> id, ep |-> e, cost |-> c]}
    /\ used' = used \cup {id}
    /\ UNCHANGED <<now, period, cap, committed, reserved, added, shown, tainted, res, esc>>

\* The webview displays the quote's price. (A buggy UI could display anything; the run's guard is
\* what binds the displayed price to the quote.)
Show(q, p) ==
    /\ q \in quotes
    /\ p \in Costs
    /\ shown' = [shown EXCEPT ![q.id] = p]
    /\ UNCHANGED <<now, period, cap, committed, reserved, added, quotes, used, tainted, res, esc>>

-----------------------------------------------------------------------------
\* Runs

Fits(q) == committed + reserved + q.cost <= cap

RunGuards(q) ==
    /\ q \in quotes
    /\ period = now                    \* Ledger::roll ran first
    /\ shown[q.id] = q.cost            \* NoEscalationWithoutShownPrice
    /\ q.ep \in added                  \* EgressOptInOnly

Record(q, m) ==
    [id |-> q.id, ep |-> q.ep, cost |-> q.cost, shownAt |-> shown[q.id], mode |-> m,
     optedIn |-> q.ep \in added, fit |-> Fits(q), taint |-> tainted]

RunBudget(q) ==
    /\ RunGuards(q)
    /\ ~tainted
    /\ Fits(q)
    /\ reserved' = reserved + q.cost
    /\ res' = res \cup {[id |-> q.id, amt |-> q.cost, per |-> period, mode |-> "budget"]}
    /\ esc' = esc \cup {Record(q, "budget")}
    /\ quotes' = quotes \ {q}
    /\ UNCHANGED <<now, period, cap, committed, added, shown, used, tainted>>

\* The member's explicit HIC-1 decision for this one request. Not counted against the cap.
RunConfirmed(q) ==
    /\ RunGuards(q)
    /\ res' = res \cup {[id |-> q.id, amt |-> q.cost, per |-> period, mode |-> "confirmed"]}
    /\ esc' = esc \cup {Record(q, "confirmed")}
    /\ quotes' = quotes \ {q}
    /\ UNCHANGED <<now, period, cap, committed, reserved, added, shown, used, tainted>>

\* Charge c (the provider's reported usage, or 0 for not sent, or the full amount), at most the
\* reservation.
Settle(r, c) ==
    /\ r \in res
    /\ period = now
    /\ c \in 0..r.amt
    /\ res' = res \ {r}
    /\ IF r.per = period /\ r.mode = "budget"
          THEN /\ reserved' = reserved - r.amt
               /\ committed' = committed + c
          ELSE UNCHANGED <<reserved, committed>>
    /\ UNCHANGED <<now, period, cap, added, quotes, shown, used, tainted, esc>>

-----------------------------------------------------------------------------
\* The clock, the period and the cap

ClockForward ==
    /\ now < MaxDay
    /\ now' = now + 1
    /\ UNCHANGED <<period, cap, committed, reserved, added, quotes, shown, used, tainted, res, esc>>

ClockBack ==
    /\ now > 0
    /\ now' = now - 1
    /\ UNCHANGED <<period, cap, committed, reserved, added, quotes, shown, used, tainted, res, esc>>

\* Ledger::roll: only a later day resets the counters.
Roll ==
    /\ now > period
    /\ period' = now
    /\ committed' = 0
    /\ reserved' = 0
    /\ UNCHANGED <<now, cap, added, quotes, shown, used, tainted, res, esc>>

SetCap(c) ==
    /\ period = now
    /\ c >= committed + reserved
    /\ cap' = c
    /\ UNCHANGED <<now, period, committed, reserved, added, quotes, shown, used, tainted, res, esc>>

-----------------------------------------------------------------------------

Next ==
    \/ \E e \in Endpoints : AddEndpoint(e) \/ RemoveEndpoint(e)
    \/ Ingest \/ NewTask
    \/ \E id \in QIds, e \in Endpoints, c \in Costs : Quote(id, e, c)
    \/ \E q \in quotes, p \in Costs : Show(q, p)
    \/ \E q \in quotes : RunBudget(q) \/ RunConfirmed(q)
    \/ \E r \in res, c \in 0..MaxSpend : Settle(r, c)
    \/ ClockForward \/ ClockBack \/ Roll
    \/ \E c \in Caps : SetCap(c)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
\* Safety

\* Budgeted spend this period never exceeds the cap.
SpendWithinCap == committed + reserved <= cap

\* Every run used exactly the price the member was shown for that quote.
NoEscalationWithoutShownPrice == \A x \in esc : x.shownAt = x.cost

\* Every run went to an endpoint the member had added at the time.
EgressOptInOnly == \A x \in esc : x.optedIn

\* A run that did not fit the budget, or ran with untrusted context, was the member's decision.
OverBudgetOrTaintedNeedsHic1 == \A x \in esc : (~x.fit \/ x.taint) => x.mode = "confirmed"

\* The in-flight counter is exactly the budget reservations of this period.
RECURSIVE SumAmt(_)
SumAmt(S) == IF S = {} THEN 0 ELSE LET r == CHOOSE r \in S : TRUE IN r.amt + SumAmt(S \ {r})
ReservedIsConsistent ==
    reserved = SumAmt({r \in res : r.mode = "budget" /\ r.per = period})

\* Committed spend drops only when the period advances to a later day.
ResetOnlyAtPeriodBoundary == [][committed' < committed => period' > period]_vars

\* The period never moves backwards, whatever the clock does.
PeriodMonotone == [][period' >= period]_vars
=============================================================================
