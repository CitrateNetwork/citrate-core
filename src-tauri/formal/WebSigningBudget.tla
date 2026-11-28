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
(* B-3 budgeted contract calls (ADR-2026-11-27 Rule-3 amendment 2, L2-     *)
(* WP06a; PROPOSED, nothing wired):                                        *)
(*   - a separate request type and entry point (DecideCall, the model of   *)
(*     request_call_budgeted) under the same budget lock, signed by the    *)
(*     same gated signer (Sign, Return);                                   *)
(*   - one budget per agent session (CallSlot), granted by an HIC-1        *)
(*     ceremony (GrantCall) over a target set chosen from the contracts    *)
(*     this member's wallet deployed from a READY gate record (deployed,   *)
(*     grown only by DeployOwn: a per-deploy ceremony, never a budget);    *)
(*   - an adversarial environment (RequestCall) that asks for creations,   *)
(*     value, foreign targets, other chains, raw-ack calldata, approval    *)
(*     and permit selectors, and other sessions' budgets;                  *)
(*   - max_ops <= 20, expiry <= the model's 24 h, a per-budget gas cap with*)
(*     write-ahead gas reservation, strict taint (no O-3 exemption).       *)
(*   - B3Mutant names one broken guard in a mutant config; "none" in every *)
(*     green config. Mutant and witness configs: WebSigningBudgetB3_*.cfg. *)
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
    TaintSources,     \* which taint sources the environment produces (subset of Sources)
    \* B-3 (ADR-2026-11-27). Every pre-B-3 config sets Sessions = {} and Contracts = {}, so
    \* none of the B-3 actions is enabled there and those state spaces are unchanged.
    Sessions,         \* agent sessions that can hold a B-3 budget (origin agent:<session>)
    Contracts,        \* contract addresses on chain (strings); "create" is a contract creation
    Chains,           \* chain ids a call request can name; only "c40204" is coverable
    CallValues,       \* tx value a call request can carry; only 0 is coverable
    Gas,              \* worst-case gas cost of one call (gas_limit * max_fee), positive
    Selectors,        \* calldata classes: "plain" or a never-covered class ("approve", ...)
    CallMaxOps,       \* max_ops of a B-3 budget (at most 20)
    CallTTL,          \* B-3 budget lifetime at grant, in ticks (at most W, the model's 24 h)
    GasCap,           \* per-budget gas cap (sum of reserved worst-case gas)
    MaxDeploys,       \* bound on member deploys (per-deploy ceremonies, outside any budget)
    B3Mutant          \* "none", or the one B-3 guard a mutant config breaks

ASSUME Allowlist \subseteq Origins
ASSUME /\ MaxCount \in Nat /\ PerSigMax \in Nat /\ PerRecipMax \in Nat /\ GlobalMax \in Nat
       /\ W \in Nat \ {0} /\ TTL \in Nat \ {0} /\ O3Exempt \in BOOLEAN

\* D1 as amended by ADR-2026-11-27: B-1 SIWE, B-2 x402 and B-3 budgeted contract calls. A
\* "BudgetedCall" is a transaction submitted through the separate B-3 request type; a "Tx" in
\* OtherKinds is any transaction submitted as a generic intent, which is never budgetable.
ClosedList  == {"Siwe", "X402", "BudgetedCall"}
AllKinds    == ClosedList \cup OtherKinds
ASSUME OtherKinds \cap ClosedList = {}

B3Mutants == {"none", "Covers", "SignsCreation", "SignsValue", "ForeignTarget", "GrantForeign",
              "AnyChain", "RawAck", "Delegation", "ForeignSession", "NoOpsCap", "NoGasCap",
              "TaintIgnored", "WriteBehind"}
ASSUME /\ "c40204" \in Chains /\ 0 \in CallValues /\ "plain" \in Selectors
       /\ "create" \notin Contracts /\ "other" \notin Sessions
       /\ CallValues \subseteq Nat /\ Gas \subseteq Nat \ {0} /\ GasCap \in Nat
       /\ CallMaxOps \in 1..20                 \* ADR-2026-11-27: max_ops <= 20
       /\ CallTTL \in 1..W                     \* ADR-2026-11-27: expiry <= 24 h
       /\ MaxDeploys \in Nat /\ B3Mutant \in B3Mutants

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
\* B-3: one budget per agent session, a third slot type.
WebSlot(o)  == <<"web", o>>
PaySlot(r)  == <<"x402", r>>
CallSlot(x) == <<"call", x>>
CallSlots   == {CallSlot(x) : x \in Sessions}
Slots       == {WebSlot(o) : o \in Origins} \cup {PaySlot(r) : r \in Recipients} \cup CallSlots

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
    cnt,        \* counters: [req, pending, signed, unknown, lost, grants, crashes]
    deployed    \* contracts this member's wallet deployed from a READY gate record (deploy
                \* records, persisted, append-only); the only possible B-3 targets

vars == <<clock, budget, ledger, resv, records, signed, revokes, req, lock, taint, down, cnt,
          deployed>>

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
\* taint a sign-in to that origin. x402 never gets an exemption (D3 "Taint"), and neither
\* does B-3 (ADR-2026-11-27). The last disjunct is the TaintIgnored mutant only.
TaintOk(rq) ==
    \/ taint = {}
    \/ /\ O3Exempt
       /\ rq.kind = "Siwe"
       /\ taint \subseteq {rq.origin}
    \/ B3Mutant = "TaintIgnored" /\ rq.kind = "BudgetedCall"

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

\* B-3 (ADR-2026-11-27). Worst-case gas reserved under generation g of session x's budget.
CallGas(x, g) ==
    SumSet({i \in Idx(resv) : resv[i].kind = "BudgetedCall" /\ resv[i].key = x /\ resv[i].gen = g},
           [i \in Idx(resv) |-> resv[i].v])

CallBudgetLiveIgnoringCount(s) ==
    budget[s].live /\ ~budget[s].revoked /\ clock < budget[s].exp

\* The target set a call slot's current generation was granted over ({} for other slots).
TermsOf(s) == IF s \in CallSlots THEN budget[s].targets ELSE {}

\* Mutant switch: the guard holds, or the mutant config breaks exactly this guard.
Broken(m) == B3Mutant = m

\* B-3 eligibility, first cut: exactly what the dormant SessionBudget::covers (ADR-2026-08-29)
\* checks: origin, kind, chain, ops left, not expired. RED: the B-3 configs expect more.
CallCovers(rq) ==
    /\ rq.att = rq.sess
    /\ rq.chain = "c40204"
    /\ CallBudgetLiveIgnoringCount(CallSlot(rq.sess))
    /\ budget[CallSlot(rq.sess)].used < CallMaxOps

CallEligible(rq) ==
    /\ rq.kind = "BudgetedCall"
    /\ CallCovers(rq)

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
    /\ deployed \subseteq Contracts
    /\ \A s \in CallSlots : budget[s].targets \subseteq Contracts

Init ==
    /\ clock = 0
    /\ budget = [s \in Slots |->
                  IF s \in CallSlots
                  THEN [gen |-> 0, live |-> FALSE, revoked |-> FALSE, used |-> 0, exp |-> 0,
                        targets |-> {}]
                  ELSE [gen |-> 0, live |-> FALSE, revoked |-> FALSE, used |-> 0, exp |-> 0]]
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
    /\ deployed = {}

Idle == lock.phase = "idle"
Up   == ~down

-----------------------------------------------------------------------------
(* Environment: time, the sidecar raising requests, untrusted content. *)

Tick ==
    /\ clock < MaxClock
    /\ clock' = clock + 1
    /\ UNCHANGED <<budget, ledger, resv, records, signed, revokes, req, lock, taint, down, cnt, deployed>>

Raise(rq) ==
    /\ Up /\ req = NoReq /\ cnt.req < MaxReq
    /\ req' = rq
    /\ cnt' = [cnt EXCEPT !.req = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, lock, taint, down, deployed>>

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

\* B-3: a contract call submitted through the B-3 request type. The sidecar names the session
\* budget it claims (`sess`), the target, the calldata and a gas hint; core attests the
\* session of the channel the request came in on (`att`; "other" is a page, an external
\* client or the member's own UI). The environment is adversarial: it targets any contract
\* (own or foreign) or asks for a creation, at any gas, and each request can carry one more
\* defect: another origin, another chain, value, raw-ack calldata or an approval-class
\* selector. One extra defect per request loses nothing: every B-3 check is a conjunct over
\* one field, so a request with two defects fails whenever either single-defect request
\* fails, and an ineligible request only adds one pending HIC-1 ceremony (README, B-3).
Defects == {"none", "origin", "chain", "value", "raw", "selector"}
RequestCall ==
    /\ "BudgetedCall" \in ReqKinds
    /\ \E x \in Sessions, t \in Contracts \cup {"create"}, g \in Gas, f \in Defects :
       \E a \in (IF f = "origin" THEN (Sessions \cup {"other"}) \ {x} ELSE {x}),
         ch \in (IF f = "chain" THEN Chains \ {"c40204"} ELSE {"c40204"}),
         v \in (IF f = "value" THEN CallValues \ {0} ELSE {0}),
         sel \in (IF f = "selector" THEN Selectors \ {"plain"} ELSE {"plain"}) :
          Raise([kind |-> "BudgetedCall", sess |-> x, att |-> a, chain |-> ch, value |-> v,
                 to |-> t, decoded |-> f # "raw", sel |-> sel, gas |-> g])

Taint ==
    /\ Up
    /\ \E src \in TaintSources : /\ src \notin taint
                            /\ taint' = taint \cup {src}
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, req, lock, down, cnt, deployed>>

\* A new agent task starts with a clean context. The current task must be finished:
\* no request waiting and nothing under the lock.
NewTask ==
    /\ Up /\ Idle /\ req = NoReq /\ taint # {}
    /\ taint' = {}
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, req, lock, down, cnt, deployed>>

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
    /\ UNCHANGED <<clock, ledger, resv, records, signed, revokes, req, lock, taint, down, deployed>>

\* D4 Revoke: takes the dedicated budget lock, persists the tombstone, returns. The
\* revoke "commits" at this step; `revokes` remembers how many auto-signs existed then.
Revoke(s) ==
    /\ Up /\ Idle
    /\ budget[s].live /\ ~budget[s].revoked
    /\ budget' = [budget EXCEPT ![s].revoked = TRUE]
    /\ revokes' = revokes \cup {[slot |-> s, gen |-> budget[s].gen, n |-> Len(signed)]}
    /\ UNCHANGED <<clock, ledger, resv, records, signed, req, lock, taint, down, cnt, deployed>>

\* "Stop all autonomy" / budget_revoke_all: every live budget at once, under the lock.
RevokeAll ==
    /\ Up /\ Idle
    /\ \E s \in Slots : budget[s].live /\ ~budget[s].revoked
    /\ budget' = [s \in Slots |-> IF budget[s].live THEN [budget[s] EXCEPT !.revoked = TRUE]
                                                    ELSE budget[s]]
    /\ revokes' = revokes \cup {[slot |-> s, gen |-> budget[s].gen, n |-> Len(signed)] :
                                 s \in {x \in Slots : budget[x].live /\ ~budget[x].revoked}}
    /\ UNCHANGED <<clock, ledger, resv, records, signed, req, lock, taint, down, cnt, deployed>>

\* B-3 grant (ADR-2026-11-27 D4): one HIC-1 ceremony, the member's personal_sign over the
\* terms, opens a new generation of the session's budget. The terms name the target set,
\* chosen from the contracts this wallet deployed from a READY gate record, never empty. A
\* grant never widens a live budget; a later deploy is not covered until a new grant.
GrantCall(s) ==
    /\ Up /\ Idle /\ cnt.grants < MaxGrants
    /\ ~CallBudgetLiveIgnoringCount(s)
    /\ \E T \in (SUBSET (IF Broken("GrantForeign") THEN Contracts ELSE deployed)) \ {{}} :
          budget' = [budget EXCEPT ![s] = [gen |-> @.gen + 1, live |-> TRUE, revoked |-> FALSE,
                                           used |-> 0, exp |-> clock + CallTTL, targets |-> T]]
    /\ cnt' = [cnt EXCEPT !.grants = @ + 1]
    /\ UNCHANGED <<clock, ledger, resv, records, signed, revokes, req, lock, taint, down, deployed>>

\* A contract creation the member approved in its own per-deploy SignatureCeremony, with a
\* READY gate record for exactly that init code (DeployGate.tla), lands in the deploy
\* records. It is never a budgeted signature, so it never appears in `signed`.
DeployOwn(c) ==
    /\ Up /\ c \notin deployed /\ Cardinality(deployed) < MaxDeploys
    /\ deployed' = deployed \cup {c}
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, req, lock, taint, down, cnt>>

-----------------------------------------------------------------------------
(* Core: request_budgeted (D7). One acquisition of the budget lock covers the check,  *)
(* the reservation, the write-ahead record, and the signature.                       *)

\* Acquire the lock and decide. Eligible: reserve (counters, ledger, window spend) and
\* write the decision record atomically (write-ahead), then go on to sign. Not eligible:
\* fall through to an HIC-1 pending ceremony (never dropped, D2 / D8).
Decide ==
    /\ Up /\ Idle /\ req # NoReq
    /\ req.kind # "BudgetedCall"
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
    /\ UNCHANGED <<clock, signed, revokes, taint, down, deployed>>

\* The write-ahead itself fails (disk full, MAC key unavailable): nothing is signed, the
\* request falls through to HIC-1. Nothing was persisted, so nothing is reserved.
WriteAheadFail ==
    /\ Up /\ Idle /\ req # NoReq /\ Eligible(req)
    /\ req' = NoReq
    /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, lock, taint, down, deployed>>

\* B-3: request_call_budgeted (ADR-2026-11-27 D7), the same shape as Decide under the same
\* budget lock. Eligible: debit one op, reserve the worst-case gas, and write the decision
\* record (which carries the request) in one atomic write, then go on to Sign. Not
\* eligible: an HIC-1 pending ceremony. WriteBehind mutant: the record is not written here.
DecideCall ==
    /\ Up /\ Idle /\ req # NoReq /\ req.kind = "BudgetedCall"
    /\ IF CallEligible(req)
       THEN LET s == CallSlot(req.sess)
                rid == Len(records) + 1 IN
            /\ budget' = [budget EXCEPT ![s].used = @ + 1]
            /\ resv' = Append(resv, [kind |-> "BudgetedCall", key |-> req.sess, v |-> req.gas,
                                     t |-> clock, gen |-> budget[s].gen])
            /\ records' = IF Broken("WriteBehind") THEN records
                          ELSE Append(records, [slot |-> s, gen |-> budget[s].gen,
                                                status |-> "reserved", rq |-> req])
            /\ lock' = [phase |-> "recorded", rq |-> req, slot |-> s, gen |-> budget[s].gen,
                        rid |-> rid]
            /\ UNCHANGED cnt
       ELSE /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
            /\ UNCHANGED <<budget, resv, records, lock>>
    /\ req' = NoReq
    /\ UNCHANGED <<clock, ledger, signed, revokes, taint, down, deployed>>

\* B-3 write-ahead failure: nothing reserved, nothing signed, HIC-1.
CallWriteAheadFail ==
    /\ Up /\ Idle /\ req # NoReq /\ req.kind = "BudgetedCall" /\ CallEligible(req)
    /\ req' = NoReq
    /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, lock, taint, down, deployed>>

\* Still holding the lock: call the gated signer. Taint and expiry are re-read immediately
\* before the signer (the safer reading; see README ambiguity A-2). If either now fails,
\* nothing is signed, the record is closed as not_signed (known, no crash) and the request
\* falls through to HIC-1. The reservation is kept (over-count is the safe direction).
Sign ==
    /\ Up /\ lock.phase = "recorded"
    /\ IF TaintOk(lock.rq) /\ clock < budget[lock.slot].exp
       THEN /\ signed' = Append(signed,
                  [kind |-> lock.rq.kind, slot |-> lock.slot, gen |-> lock.gen, rid |-> lock.rid,
                   rq |-> lock.rq, t |-> clock, exp |-> budget[lock.slot].exp, taint |-> taint,
                   own |-> deployed, terms |-> TermsOf(lock.slot)])
            /\ lock' = [lock EXCEPT !.phase = "signed"]
            /\ UNCHANGED <<records, cnt>>
       ELSE /\ records' = [records EXCEPT ![lock.rid].status = "not_signed"]
            /\ lock' = [phase |-> "idle"]
            /\ cnt' = [cnt EXCEPT !.pending = @ + 1]
            /\ UNCHANGED signed
    /\ UNCHANGED <<clock, budget, ledger, resv, revokes, req, taint, down, deployed>>

\* Close the record and release the lock; the signature goes back to the caller.
Return ==
    /\ Up /\ lock.phase = "signed"
    /\ records' = [records EXCEPT ![lock.rid].status = "signed"]
    /\ lock' = [phase |-> "idle"]
    /\ cnt' = [cnt EXCEPT !.signed = @ + 1]
    /\ UNCHANGED <<clock, budget, ledger, resv, signed, revokes, req, taint, down, deployed>>

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
    /\ UNCHANGED deployed
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
    /\ UNCHANGED <<clock, budget, ledger, resv, signed, revokes, req, lock, taint, deployed>>

-----------------------------------------------------------------------------
Next ==
    \/ Tick \/ RequestSiwe \/ RequestX402 \/ RequestOther \/ Taint \/ NewTask
    \/ \E s \in Slots \ CallSlots : Grant(s)
    \/ \E s \in CallSlots : GrantCall(s)
    \/ \E s \in Slots : Revoke(s)
    \/ RevokeAll
    \/ \E c \in Contracts : DeployOwn(c)
    \/ Decide \/ WriteAheadFail \/ Sign \/ Return
    \/ RequestCall \/ DecideCall \/ CallWriteAheadFail
    \/ Crash \/ Recover

Fairness == WF_vars(Decide) /\ WF_vars(DecideCall) /\ WF_vars(Sign) /\ WF_vars(Return)
            /\ WF_vars(Recover)

Spec == Init /\ [][Next]_vars /\ Fairness

\* Recipients and nonces are interchangeable in the spec (safety configs only; TLC does not
\* support symmetry soundly with liveness checking).
Symmetry == Permutations(Recipients) \cup Permutations(Nonces)

-----------------------------------------------------------------------------
(* Invariants and properties. Names follow ADR D9 (and 03_TLA_SPECS).      *)

IsSiwe(i) == signed[i].kind = "Siwe"
IsX402(i) == signed[i].kind = "X402"
IsCall(i) == signed[i].kind = "BudgetedCall"

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
SlotMax(s) == IF s \in CallSlots THEN CallMaxOps ELSE MaxCount
NeverExceedsCaps ==
    /\ \A s \in Slots : budget[s].used <= SlotMax(s)
    /\ \A s \in Slots, g \in Gens :
          Cardinality({i \in Idx(signed) : signed[i].slot = s /\ signed[i].gen = g}) <= SlotMax(s)
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
        /\ IsCall(i) => records[signed[i].rid].rq = signed[i].rq   \* B-3: the record binds the call

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
                  /\ (s \in CallSlots => budget'[s].targets = budget[s].targets)
        /\ Len(resv') >= Len(resv) /\ SubSeq(resv', 1, Len(resv)) = resv
        /\ \A o \in Origins : ledger[o] \subseteq ledger'[o]
        /\ Len(signed') >= Len(signed) /\ SubSeq(signed', 1, Len(signed)) = signed
        /\ deployed \subseteq deployed' ]_vars

-----------------------------------------------------------------------------
(* B-3 invariants (ADR-2026-11-27 D9). Each has a mutant config that breaks it.  *)

\* No contract creation is ever auto-signed: every 40204 publish is a per-deploy ceremony.
NoCreationAutoSigned == \A i \in Idx(signed) : IsCall(i) => signed[i].rq.to # "create"

\* No auto-signed call carries value.
NoValueAutoSigned == \A i \in Idx(signed) : IsCall(i) => signed[i].rq.value = 0

\* The target is in the set the budget was granted over, and that set holds only contracts
\* this member's wallet deployed from a READY gate record, as of the signature.
OnlyOwnDeployedTargets ==
    \A i \in Idx(signed) : IsCall(i) =>
        /\ signed[i].rq.to \in signed[i].terms
        /\ signed[i].terms \subseteq signed[i].own

\* Chain 40204 only.
ChainPinned == \A i \in Idx(signed) : IsCall(i) => signed[i].rq.chain = "c40204"

\* Calldata that needs a raw-mode ack is never auto-signed.
NoRawAckAutoSigned == \A i \in Idx(signed) : IsCall(i) => signed[i].rq.decoded

\* Approvals, permits, operator, role, ownership and upgrade selectors are never auto-signed.
NoDelegationAutoSigned == \A i \in Idx(signed) : IsCall(i) => signed[i].rq.sel = "plain"

\* The call came in on the session whose budget paid for it (origin agent:<session>).
SessionBound ==
    \A i \in Idx(signed) : IsCall(i) =>
        /\ signed[i].rq.att = signed[i].rq.sess
        /\ signed[i].slot = CallSlot(signed[i].rq.sess)

\* Worst-case gas reserved under one generation of a session's budget never exceeds the cap.
\* Every signature is preceded by its reservation, so this bounds signed gas as well.
GasCapped == \A x \in Sessions, g \in Gens : CallGas(x, g) <= GasCap

\* Reachability witness (RT-17): TLC must find this violated on the green B-3 constants,
\* which shows a budgeted call can be auto-signed at all (the invariants are not vacuous).
W_NoCallAutoSigned == \A i \in Idx(signed) : ~IsCall(i)

\* D9 FallThroughLive: a request is never stuck; with NoDrop, a request that fails any
\* check becomes a pending HIC-1 ceremony. The lock is always released, a crash recovered.
FallThroughLive ==
    /\ (req # NoReq) ~> (req = NoReq)
    /\ (lock.phase # "idle") ~> (lock.phase = "idle")
    /\ down ~> ~down

=============================================================================
