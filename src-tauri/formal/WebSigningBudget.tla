--------------------------- MODULE WebSigningBudget ---------------------------
(***************************************************************************)
(* HUP-S2.3 (formal half) — the signing side of the Rule-3 amendment,      *)
(* ADR-2026-09-30-rule3-budgetable-signatures.md, section D9.              *)
(*                                                                         *)
(* STATUS: a model of a PROPOSED design. Nothing here is wired; the ADR is *)
(* not accepted and every signature in the tree is still HIC-1.           *)
(*                                                                         *)
(* What is modelled                                                        *)
(*   - the closed list (D1): only B-1 hardened SIWE and B-2 capped x402    *)
(*     may be auto-signed; every other kind (transactions, permits, other  *)
(*     typed data, eth_sign) always falls through to an HIC-1 ceremony.    *)
(*   - the SIWE provenance and message checks of D2 that carry a safety    *)
(*     property: core-attested top-frame origin, managed browser only,     *)
(*     origin allowlist, domain/URI binding, nonce ledger, no Resources.   *)
(*   - the x402 cap model of D3: per-signature max, recipient pinning,     *)
(*     per-recipient rolling-window max, global rolling-window max,        *)
(*     max_count, budget expiry. Reservation is write-ahead, never refunded*)
(*   - the dedicated budget lock of D4: one critical section covers        *)
(*     check, reserve, record and sign. Revoke and grant take the same lock*)
(*   - write-ahead decision records (D4) and crash recovery (D8): a record *)
(*     whose outcome is not known after a crash becomes outcome_unknown.   *)
(*   - the taint rule (D2 #19, red-team correction 3), with the O-3 same-  *)
(*     origin exemption as a constant that defaults to OFF (strict rule).  *)
(*   - a bounded clock (Tick) and rolling windows of W ticks.              *)
(*                                                                         *)
(* Abstractions (see README "WebSigningBudget" for the full list)          *)
(*   - deterministic parse checks with no cross-request state (strict      *)
(*     EIP-4361 parse, Version, Chain ID, address, Issued At, statement    *)
(*     length) are folded into the per-request booleans that ARE modelled; *)
(*     any failing check takes the same HIC-1 branch.                      *)
(*   - the x402 authorization nonce is core-generated (unique by           *)
(*     construction) and is not modelled.                                  *)
(*   - one member, one principal. Cross-principal isolation is not here.   *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Origins,          \* web origins a managed-browser tab can be on
    Allowlist,        \* origins the member allowlisted for SIWE budgets (subset of Origins)
    Recipients,       \* x402 payees (escalation settlement addresses)
    Nonces,           \* SIWE nonces a site may put in a message
    Values,           \* x402 amounts, in base units of the one allowlisted asset
    OtherKinds,       \* intent kinds that are NOT on the closed list
    MaxCount,         \* max_count of every budget (one value for the model)
    PerSigMax,        \* x402 per_signature_max
    PerRecipMax,      \* x402 per_recipient_window_max
    GlobalMax,        \* x402 global_window_max (GlobalSpendCap)
    SiweWindowMax,    \* SIWE auto-signs per origin per rolling window (D2 #21)
    SiweMinGap,       \* minimum ticks between SIWE auto-signs to one origin (D2 #21)
    W,                \* rolling window length, in ticks (the model's "24 h")
    TTL,              \* budget lifetime at grant, in ticks
    MaxClock,         \* bound on the clock
    MaxReq,           \* bound on requests raised by the sidecar
    MaxGrants,        \* bound on budget grants (HIC-1 member decisions)
    MaxCrashes,       \* bound on process crashes
    O3Exempt,         \* owner decision O-3; FALSE = strict red-team rule (the ADR default)
    ReqKinds,         \* which request kinds the environment raises (lets a cfg focus a class)
    TaintSources      \* which taint sources the environment produces (subset of Sources)

ASSUME Allowlist \subseteq Origins
ASSUME /\ MaxCount \in Nat /\ PerSigMax \in Nat /\ PerRecipMax \in Nat /\ GlobalMax \in Nat
       /\ W \in Nat \ {0} /\ TTL \in Nat \ {0} /\ O3Exempt \in BOOLEAN

ClosedList  == {"Siwe", "X402"}
AllKinds    == ClosedList \cup OtherKinds
ASSUME OtherKinds \cap ClosedList = {}

\* Provenance of a SIWE request as core attests it over its own CDP session:
\*   "top"    : the attested top frame of a managed-browser tab
\*   "iframe" : a sub-frame, popup or worker (never budgetable, D2 #2)
\*   "attach" : attach-to-Chrome mode (never budgetable, D2 #3)
Provenances == {"top", "iframe", "attach"}

\* Taint sources: any web origin, or "ext" (search results, third-party skill or MCP
\* output, other principals' memory, verified-source comments).
Sources == Origins \cup {"ext"}
ASSUME TaintSources \subseteq Sources
ASSUME ReqKinds \subseteq AllKinds

\* Budget slots: one SIWE budget per origin, one x402 budget per pinned recipient.
\* The two kinds are different slot types, so neither can be widened into the other.
WebSlot(o)  == <<"web", o>>
PaySlot(r)  == <<"x402", r>>
Slots       == {WebSlot(o) : o \in Origins} \cup {PaySlot(r) : r \in Recipients}

NoReq == [kind |-> "none"]

VARIABLES
    clock,      \* bounded logical clock (monotonic truth)
    budget,     \* [Slots -> [gen, live, revoked, used, exp]]
    ledger,     \* [Origins -> SUBSET Nonces], the per-origin SIWE nonce ledger (persisted)
    resv,       \* Seq of reservations [kind, key, v, t] (persisted, never refunded)
    records,    \* Seq of decision records [slot, gen, status] (persisted, write-ahead)
    signed,     \* Seq of auto-sign events (history: what the gated signer produced)
    revokes,    \* set of [slot, gen, n]: revoke of (slot, gen) committed when Len(signed) = n
    req,        \* the one request waiting for the budget lock, or NoReq
    lock,       \* [phase, rq, slot, gen, rid]; phase "idle" means the budget lock is free
    taint,      \* SUBSET Sources: untrusted content in the current task's context
    down,       \* the core process has crashed and not yet recovered
    cnt         \* counters: [req, pending, signed, unknown, lost, grants, crashes]

vars == <<clock, budget, ledger, resv, records, signed, revokes, req, lock, taint, down, cnt>>

-----------------------------------------------------------------------------
(* Helpers *)

RECURSIVE SumSet(_, _)
SumSet(S, f) == IF S = {} THEN 0
                ELSE LET x == CHOOSE y \in S : TRUE IN f[x] + SumSet(S \ {x}, f)

Idx(s) == 1..Len(s)

InWindow(t, end) == t <= end /\ end < t + W     \* t in (end - W, end]

\* x402 value reserved to recipient r in the rolling window ending at `end`.
RecipWin(r, end) ==
    SumSet({i \in Idx(resv) : resv[i].kind = "X402" /\ resv[i].key = r /\ InWindow(resv[i].t, end)},
           [i \in Idx(resv) |-> resv[i].v])

\* x402 value reserved across all recipients in the rolling window ending at `end`.
GlobalWin(end) ==
    SumSet({i \in Idx(resv) : resv[i].kind = "X402" /\ InWindow(resv[i].t, end)},
           [i \in Idx(resv) |-> resv[i].v])

\* SIWE auto-sign reservations for origin o in the rolling window ending at `end`.
SiweWin(o, end) ==
    Cardinality({i \in Idx(resv) : resv[i].kind = "Siwe" /\ resv[i].key = o /\ InWindow(resv[i].t, end)})

SiweGapOk(o) ==
    \A i \in Idx(resv) : (resv[i].kind = "Siwe" /\ resv[i].key = o) => clock >= resv[i].t + SiweMinGap

BudgetLiveIgnoringCount(s) ==
    budget[s].live /\ ~budget[s].revoked /\ clock < budget[s].exp

BudgetLive(s) ==
    /\ budget[s].live
    /\ ~budget[s].revoked
    /\ clock < budget[s].exp
    /\ budget[s].used < MaxCount

\* Strict taint rule; with O-3 accepted, content from the same allowlisted origin does not
\* taint a sign-in to that origin. x402 never gets an exemption (D3 "Taint").
TaintOk(rq) ==
    \/ taint = {}
    \/ /\ O3Exempt
       /\ rq.kind = "Siwe"
       /\ taint \subseteq {rq.origin}

SlotOf(rq) == IF rq.kind = "Siwe" THEN WebSlot(rq.origin) ELSE PaySlot(rq.recip)

\* D2: every SIWE check that carries a modelled safety property.
SiweEligible(rq) ==
    /\ rq.kind = "Siwe"
    /\ rq.prov = "top"                          \* D2 #1-3: attested top frame, managed browser
    /\ rq.origin \in Allowlist                  \* D2 #4
    /\ rq.domain = rq.origin                    \* D2 #7 domain binding
    /\ rq.uri = rq.origin                       \* D2 #8 URI binding
    /\ ~rq.resource                             \* D2 #17 no Resources / ReCap
    /\ rq.nonce \notin ledger[rq.origin]        \* D2 #12 nonce ledger
    /\ BudgetLive(WebSlot(rq.origin))           \* D2 #20
    /\ SiweWin(rq.origin, clock) < SiweWindowMax \* D2 #21 rolling rate
    /\ SiweGapOk(rq.origin)                     \* D2 #21 burst gap
    /\ TaintOk(rq)                              \* D2 #19

\* D3: the x402 cap model.
X402Eligible(rq) ==
    /\ rq.kind = "X402"
    /\ rq.to = rq.recip                         \* recipient pinning
    /\ rq.value <= PerSigMax                    \* per_signature_max
    /\ BudgetLive(PaySlot(rq.recip))            \* live, not revoked, not expired, count left
    /\ RecipWin(rq.recip, clock) + rq.value <= PerRecipMax  \* rolling per-recipient
    /\ GlobalWin(clock) + rq.value <= GlobalMax              \* rolling global
    /\ TaintOk(rq)                              \* always strict for x402

Eligible(rq) == SiweEligible(rq) \/ X402Eligible(rq)

-----------------------------------------------------------------------------
TypeOK ==
    /\ clock \in 0..MaxClock
    /\ \A s \in Slots : /\ budget[s].gen \in Nat /\ budget[s].live \in BOOLEAN
                        /\ budget[s].revoked \in BOOLEAN /\ budget[s].used \in Nat
                        /\ budget[s].exp \in Nat
    /\ ledger \in [Origins -> SUBSET Nonces]
    /\ \A i \in Idx(records) :
          records[i].status \in {"reserved", "signed", "not_signed", "outcome_unknown"}
    /\ \A i \in Idx(signed) : signed[i].kind \in AllKinds
    /\ lock.phase \in {"idle", "recorded", "signed"}
    /\ taint \subseteq Sources
    /\ down \in BOOLEAN

Init ==
    /\ clock = 0
    /\ budget = [s \in Slots |-> [gen |-> 0, live |-> FALSE, revoked |-> FALSE, used |-> 0, exp |-> 0]]
    /\ ledger = [o \in Origins |-> {}]
    /\ resv = <<>>
    /\ records = <<>>
    /\ signed = <<>>
    /\ revokes = {}
    /\ req = NoReq
    /\ lock = [phase |-> "idle"]
    /\ taint = {}
    /\ down = FALSE
    /\ cnt = [req |-> 0, pending |-> 0, signed |-> 0, unknown |-> 0, lost |-> 0,
              grants |-> 0, crashes |-> 0]

Idle == lock.phase = "idle"
Up   == ~down

-----------------------------------------------------------------------------
(* Environment: time, the sidecar raising requests, untrusted content. *)

Tick ==
    /\ clock < MaxClock
    /\ clock' = clock + 1
    /\ UNCHANGED <<budget, ledger, resv, records, signed, revokes, req, lock, taint, down, cnt>>

Raise(rq) ==
    /\ Up /\ req = NoReq /\ cnt.req < MaxReq
    /\ req' = rq
    /\ cnt' = [cnt EXCEPT !.req = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, lock, taint, down>>

\* The sidecar submits {tab_id, message}; core attests origin + provenance itself.
RequestSiwe ==
    /\ "Siwe" \in ReqKinds
    /\ \E o \in Origins, p \in Provenances, d \in Origins, u \in Origins, res \in BOOLEAN, n \in Nonces :
          Raise([kind |-> "Siwe", origin |-> o, prov |-> p, domain |-> d, uri |-> u,
                 resource |-> res, nonce |-> n])

\* The escalation router submits {quote_id, recipient, asset, amount, resource}; `recip`
\* names the budget the request claims, `to` is the payee core will put in the payload.
RequestX402 ==
    /\ "X402" \in ReqKinds
    /\ \E r \in Recipients, t \in Recipients, v \in Values :
          Raise([kind |-> "X402", recip |-> r, to |-> t, value |-> v])

\* Anything else the sidecar or a page asks core to sign: a transaction, a permit, other
\* typed data. It carries the same payee and amount shape as an x402 request, so a decoder
\* that confused kinds would have something to (wrongly) budget against.
RequestOther ==
    \E k \in OtherKinds \cap ReqKinds, r \in Recipients, v \in Values :
        Raise([kind |-> k, recip |-> r, to |-> r, value |-> v])

Taint ==
    /\ Up
    /\ \E src \in TaintSources : /\ src \notin taint
                            /\ taint' = taint \cup {src}
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, req, lock, down, cnt>>

\* A new agent task starts with a clean context. The current task must be finished:
\* no request waiting and nothing under the lock.
NewTask ==
    /\ Up /\ Idle /\ req = NoReq /\ taint # {}
    /\ taint' = {}
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, req, lock, down, cnt>>

-----------------------------------------------------------------------------
(* Member actions (HIC-1, native window). Both take the budget lock. *)

\* D4 Grant: a new generation of the slot's budget with fresh count and expiry. Only for an
\* allowlisted origin (D2 #4) or a pinned recipient, and only when the slot holds no live
\* budget (a grant never widens a live budget).
Grant(s) ==
    /\ Up /\ Idle /\ cnt.grants < MaxGrants
    /\ s[1] = "web" => s[2] \in Allowlist
    /\ ~BudgetLiveIgnoringCount(s)
    /\ budget' = [budget EXCEPT ![s] = [gen |-> @.gen + 1, live |-> TRUE, revoked |-> FALSE,
                                        used |-> 0, exp |-> clock + TTL]]
    /\ cnt' = [cnt EXCEPT !.grants = @ + 1]
    /\ UNCHANGED <<clock, ledger, resv, records, signed, revokes, req, lock, taint, down>>

\* D4 Revoke: takes the dedicated budget lock, persists the tombstone, returns. The
\* revoke "commits" at this step; `revokes` remembers how many auto-signs existed then.
Revoke(s) ==
    /\ Up /\ Idle
    /\ budget[s].live /\ ~budget[s].revoked
    /\ budget' = [budget EXCEPT ![s].revoked = TRUE]
    /\ revokes' = revokes \cup {[slot |-> s, gen |-> budget[s].gen, n |-> Len(signed)]}
    /\ UNCHANGED <<clock, ledger, resv, records, signed, req, lock, taint, down, cnt>>

\* "Stop all autonomy" / budget_revoke_all: every live budget at once, under the lock.
RevokeAll ==
    /\ Up /\ Idle
    /\ \E s \in Slots : budget[s].live /\ ~budget[s].revoked
    /\ budget' = [s \in Slots |-> IF budget[s].live THEN [budget[s] EXCEPT !.revoked = TRUE]
                                                    ELSE budget[s]]
    /\ revokes' = revokes \cup {[slot |-> s, gen |-> budget[s].gen, n |-> Len(signed)] :
                                 s \in {x \in Slots : budget[x].live /\ ~budget[x].revoked}}
    /\ UNCHANGED <<clock, ledger, resv, records, signed, req, lock, taint, down, cnt>>

-----------------------------------------------------------------------------
(* Core: request_budgeted (D7). One acquisition of the budget lock covers the check,  *)
(* the reservation, the write-ahead record, and the signature.                       *)

\* Acquire the lock and decide. Eligible: reserve (counters, ledger, window spend) and
\* write the decision record atomically (write-ahead), then go on to sign. Not eligible:
\* fall through to an HIC-1 pending ceremony (never dropped, D2 / D8).
Decide ==
    /\ Up /\ Idle /\ req # NoReq
    /\ IF Eligible(req)
       THEN LET s == SlotOf(req)
                rid == Len(records) + 1 IN
            /\ budget' = [budget EXCEPT ![s].used = @ + 1]
            /\ ledger' = IF req.kind = "Siwe"
                         THEN [ledger EXCEPT ![req.origin] = @ \cup {req.nonce}]
                         ELSE ledger
            /\ resv' = Append(resv,
                         IF req.kind = "Siwe"
                         THEN [kind |-> "Siwe", key |-> req.origin, v |-> 0, t |-> clock]
                         ELSE [kind |-> "X402", key |-> req.recip, v |-> req.value, t |-> clock])
            /\ records' = Append(records, [slot |-> s, gen |-> budget[s].gen, status |-> "reserved"])
            /\ lock' = [phase |-> "recorded", rq |-> req, slot |-> s, gen |-> budget[s].gen,
                        rid |-> rid]
            /\ UNCHANGED cnt
       ELSE /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
            /\ UNCHANGED <<budget, ledger, resv, records, lock>>
    /\ req' = NoReq
    /\ UNCHANGED <<clock, signed, revokes, taint, down>>

\* The write-ahead itself fails (disk full, MAC key unavailable): nothing is signed, the
\* request falls through to HIC-1. Nothing was persisted, so nothing is reserved.
WriteAheadFail ==
    /\ Up /\ Idle /\ req # NoReq /\ Eligible(req)
    /\ req' = NoReq
    /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, lock, taint, down>>

\* Still holding the lock: call the gated signer. Taint and expiry are re-read immediately
\* before the signer (the safer reading; see README ambiguity A-2). If either now fails,
\* nothing is signed, the record is closed as not_signed (known, no crash) and the request
\* falls through to HIC-1. The reservation is kept (over-count is the safe direction).
Sign ==
    /\ Up /\ lock.phase = "recorded"
    /\ IF TaintOk(lock.rq) /\ clock < budget[lock.slot].exp
       THEN /\ signed' = Append(signed,
                  [kind |-> lock.rq.kind, slot |-> lock.slot, gen |-> lock.gen, rid |-> lock.rid,
                   rq |-> lock.rq, t |-> clock, exp |-> budget[lock.slot].exp, taint |-> taint])
            /\ lock' = [lock EXCEPT !.phase = "signed"]
            /\ UNCHANGED <<records, cnt>>
       ELSE /\ records' = [records EXCEPT ![lock.rid].status = "not_signed"]
            /\ lock' = [phase |-> "idle"]
            /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
            /\ UNCHANGED signed
    /\ UNCHANGED <<clock, budget, ledger, resv, revokes, req, taint, down>>

\* Close the record and release the lock; the signature goes back to the caller.
Return ==
    /\ Up /\ lock.phase = "signed"
    /\ records' = [records EXCEPT ![lock.rid].status = "signed"]
    /\ lock' = [phase |-> "idle"]
    /\ cnt' = [cnt EXCEPT !.signed = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, signed, revokes, req, taint, down>>

-----------------------------------------------------------------------------
(* Crash and recovery (D8). Persisted state survives: budgets (with counters and      *)
(* tombstones), ledger, reservations, records. Volatile state is lost: the waiting     *)
(* request and the lock. The signer's output may or may not have reached anyone.      *)

Crash ==
    /\ Up /\ cnt.crashes < MaxCrashes
    /\ down' = TRUE
    /\ req' = NoReq
    /\ lock' = [phase |-> "idle"]
    /\ cnt' = [cnt EXCEPT !.crashes = @ + 1,
                          !.lost = @ + (IF req # NoReq THEN 1 ELSE 0)]
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, taint>>

\* Every record still "reserved" has an unknown outcome: the crash may have come before or
\* after the signer ran. It must not be reported as not_signed.
Recover ==
    /\ down
    /\ LET open == {i \in Idx(records) : records[i].status = "reserved"} IN
       /\ records' = [i \in Idx(records) |->
                        IF i \in open THEN [records[i] EXCEPT !.status = "outcome_unknown"]
                                      ELSE records[i]]
       /\ cnt' = [cnt EXCEPT !.unknown = @ + Cardinality(open)]
    /\ down' = FALSE
    /\ UNCHANGED <<clock, budget, ledger, resv, signed, revokes, req, lock, taint>>

-----------------------------------------------------------------------------
Next ==
    \/ Tick \/ RequestSiwe \/ RequestX402 \/ RequestOther \/ Taint \/ NewTask
    \/ \E s \in Slots : Grant(s) \/ Revoke(s)
    \/ RevokeAll
    \/ Decide \/ WriteAheadFail \/ Sign \/ Return
    \/ Crash \/ Recover

Fairness == WF_vars(Decide) /\ WF_vars(Sign) /\ WF_vars(Return) /\ WF_vars(Recover)

Spec == Init /\ [][Next]_vars /\ Fairness

\* Recipients and nonces are interchangeable in the spec (safety configs only; TLC does not
\* support symmetry soundly with liveness checking).
Symmetry == Permutations(Recipients) \cup Permutations(Nonces)

-----------------------------------------------------------------------------
(* Invariants and properties. Names follow ADR D9 (and 03_TLA_SPECS).      *)

IsSiwe(i) == signed[i].kind = "Siwe"
IsX402(i) == signed[i].kind = "X402"

\* D9 OnlyClosedList (03_TLA_SPECS: OnlySiwe for the web class): no transaction, permit,
\* other typed data or eth_sign is ever auto-signed.
OnlyClosedList == \A i \in Idx(signed) : signed[i].kind \in ClosedList

\* D9 OriginBound: domain = URI origin = attested top-frame origin, and it is allowlisted.
OriginBound ==
    \A i \in Idx(signed) : IsSiwe(i) =>
        LET rq == signed[i].rq IN
        /\ rq.domain = rq.origin /\ rq.uri = rq.origin
        /\ rq.origin \in Allowlist
        /\ signed[i].slot = WebSlot(rq.origin)

\* D9 TopFrameOnly: never from a sub-frame, popup or worker, never in attach mode.
TopFrameOnly == \A i \in Idx(signed) : IsSiwe(i) => signed[i].rq.prov = "top"

\* D9 NonceUnique: no (origin, nonce) pair is auto-signed twice.
NonceUnique ==
    \A i, j \in Idx(signed) :
        (i < j /\ IsSiwe(i) /\ IsSiwe(j) /\ signed[i].rq.origin = signed[j].rq.origin)
            => signed[i].rq.nonce # signed[j].rq.nonce

\* D9 NoCapabilityDelegation: no auto-signed SIWE carries Resources.
NoCapabilityDelegation == \A i \in Idx(signed) : IsSiwe(i) => ~signed[i].rq.resource

\* D3 recipient pinning: the payee is the recipient pinned in the budget that paid.
RecipientPinned ==
    \A i \in Idx(signed) : IsX402(i) =>
        /\ signed[i].rq.to = signed[i].rq.recip
        /\ signed[i].slot = PaySlot(signed[i].rq.recip)

\* D9 NeverExceedsCaps. Windows are checked at EVERY end point up to now, not only the
\* current one, so a calendar-window implementation would fail it. Checked over the
\* reservations, which include every signature (a reservation precedes each one).
Gens == 0..MaxGrants
NeverExceedsCaps ==
    /\ \A s \in Slots : budget[s].used <= MaxCount
    /\ \A s \in Slots, g \in Gens :
          Cardinality({i \in Idx(signed) : signed[i].slot = s /\ signed[i].gen = g}) <= MaxCount
    /\ \A i \in Idx(signed) : IsX402(i) => signed[i].rq.value <= PerSigMax
    /\ \A e \in 0..clock :
          /\ GlobalWin(e) <= GlobalMax
          /\ \A r \in Recipients : RecipWin(r, e) <= PerRecipMax
          /\ \A o \in Origins : SiweWin(o, e) <= SiweWindowMax
    /\ \A i, j \in Idx(resv) :
          (i < j /\ resv[i].kind = "Siwe" /\ resv[j].kind = "Siwe" /\ resv[i].key = resv[j].key)
              => resv[j].t >= resv[i].t + SiweMinGap

\* Every reservation, and so every signature, is covered by the reservation log.
ReservedBeforeSigned == Len(signed) <= Len(resv)

\* D9 RevokeImmediate: once a revoke of (slot, gen) has committed, no later auto-sign uses
\* it. `revokes` is the history variable: n = number of auto-signs when it committed.
RevokeImmediate ==
    \A m \in revokes : \A i \in Idx(signed) :
        (signed[i].slot = m.slot /\ signed[i].gen = m.gen) => i <= m.n

\* D9 ExpiredInert: no auto-sign at or after the budget's expiry.
ExpiredInert == \A i \in Idx(signed) : signed[i].t < signed[i].exp

\* D9 TaintDowngrade: tainted context never auto-signs (strict rule unless O-3 is accepted,
\* and never for x402).
TaintDowngrade ==
    \A i \in Idx(signed) :
        \/ signed[i].taint = {}
        \/ /\ O3Exempt /\ IsSiwe(i)
           /\ signed[i].taint \subseteq {signed[i].rq.origin}

\* D9 RecordBeforeSignature: every auto-sign has its decision record, written before it.
RecordBeforeSignature ==
    \A i \in Idx(signed) :
        /\ signed[i].rid \in Idx(records)
        /\ records[signed[i].rid].slot = signed[i].slot
        /\ records[signed[i].rid].gen = signed[i].gen

\* D8 crash semantics: a record never says not_signed when a signature exists for it. A
\* crash yields outcome_unknown, never a false negative.
NoFalseNegative ==
    \A k \in Idx(records) : records[k].status = "not_signed" =>
        \A i \in Idx(signed) : signed[i].rid # k

\* Every request raised is accounted for: pending (HIC-1), signed, outcome_unknown after a
\* crash, lost with the process before core decided it, or still in progress.
NoDrop ==
    cnt.req = cnt.pending + cnt.signed + cnt.unknown + cnt.lost
              + (IF req # NoReq THEN 1 ELSE 0)
              + Cardinality({k \in Idx(records) : records[k].status = "reserved"})

\* D9 BudgetMonotone (action property): within a generation, used never decreases, expiry
\* never extends and a revoke never un-does; generations only go up (a new Grant); the
\* reservation log and the nonce ledger only grow, across Crash/Recover too.
BudgetMonotone ==
    [][ /\ \A s \in Slots :
            /\ budget'[s].gen >= budget[s].gen
            /\ budget'[s].gen = budget[s].gen =>
                  /\ budget'[s].used >= budget[s].used
                  /\ budget'[s].exp = budget[s].exp
                  /\ (budget[s].revoked => budget'[s].revoked)
        /\ Len(resv') >= Len(resv) /\ SubSeq(resv', 1, Len(resv)) = resv
        /\ \A o \in Origins : ledger[o] \subseteq ledger'[o]
        /\ Len(signed') >= Len(signed) /\ SubSeq(signed', 1, Len(signed)) = signed ]_vars

\* D9 FallThroughLive: a request is never stuck; with NoDrop, a request that fails any
\* check becomes a pending HIC-1 ceremony. The lock is always released, a crash recovered.
FallThroughLive ==
    /\ (req # NoReq) ~> (req = NoReq)
    /\ (lock.phase # "idle") ~> (lock.phase = "idle")
    /\ down ~> ~down

=============================================================================
