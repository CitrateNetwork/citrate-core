---------------------------- MODULE AnchorSettle ----------------------------
(* HUP-S7.3 (core half): from a batched day to an anchored day.                     *)
(*                                                                                  *)
(* The runtime's AnchorBatch.tla proves the batch side (what a day's root covers,   *)
(* no re-batching, no double anchor in the ledger). This module covers what core    *)
(* adds on top: the registry may not be deployed, the member turns anchoring on and *)
(* off, the scheduler raises one approval card per day, an approval is single use   *)
(* and signs once with the anchor key, the receipt may stay unmined or revert, and  *)
(* the day is marked anchored only on a mined, successful receipt.                  *)
(*                                                                                  *)
(* Code map: chain_agent.rs (apply_settings, anchor_gate, nightly_tick, settle,     *)
(* drop_pending_when_off) and kit ceremony/anchor.rs (AnchorCeremony::request,      *)
(* approve_and_broadcast, reject, receipt_confirms).                                *)
EXTENDS Naturals

CONSTANTS Days, MaxCards

VARIABLES
    deployed,   \* AnchorRegistry is in the address book
    enabled,    \* the member turned nightly anchoring on
    card,       \* per day: "none" | "pending" | "consumed"
    cardNo,     \* per day: how many cards were raised
    sigs,       \* per <<day, card number>>: signatures produced for that card
    signedOff,  \* TRUE once any signature was produced while anchoring was off
    receipt,    \* per day: "none" | "pending" | "ok" | "reverted"
    inflight,   \* per day: anchor transactions sent and not yet mined
    anchored    \* per day: the sidecar ledger says anchored

vars == <<deployed, enabled, card, cardNo, sigs, signedOff, receipt, inflight, anchored>>

Cards == 1..MaxCards

TypeOK ==
    /\ deployed \in BOOLEAN /\ enabled \in BOOLEAN
    /\ card \in [Days -> {"none", "pending", "consumed"}]
    /\ cardNo \in [Days -> 0..MaxCards]
    /\ sigs \in [Days \X Cards -> 0..2]
    /\ signedOff \in BOOLEAN
    /\ receipt \in [Days -> {"none", "pending", "ok", "reverted"}]
    /\ inflight \in [Days -> 0..MaxCards]
    /\ anchored \in [Days -> BOOLEAN]

Init ==
    /\ deployed = FALSE /\ enabled = FALSE
    /\ card = [d \in Days |-> "none"]
    /\ cardNo = [d \in Days |-> 0]
    /\ sigs = [x \in Days \X Cards |-> 0]
    /\ signedOff = FALSE
    /\ receipt = [d \in Days |-> "none"]
    /\ inflight = [d \in Days |-> 0]
    /\ anchored = [d \in Days |-> FALSE]

\* The redeploy lands and the address book is regenerated.
Deploy ==
    /\ ~deployed
    /\ deployed' = TRUE
    /\ UNCHANGED <<enabled, card, cardNo, sigs, signedOff, receipt, inflight, anchored>>

\* apply_settings: turning on is refused while the registry is not deployed.
Enable ==
    /\ deployed /\ ~enabled
    /\ enabled' = TRUE
    /\ UNCHANGED <<deployed, card, cardNo, sigs, signedOff, receipt, inflight, anchored>>

\* Turning off drops every pending card unsigned.
Disable ==
    /\ enabled
    /\ enabled' = FALSE
    /\ card' = [d \in Days |-> IF card[d] = "pending" THEN "consumed" ELSE card[d]]
    /\ UNCHANGED <<deployed, cardNo, sigs, signedOff, receipt, inflight, anchored>>

\* nightly_tick + AnchorCeremony::request: one card per day, only for a day not anchored and not
\* waiting on a receipt.
Raise(d) ==
    /\ deployed /\ enabled
    /\ ~anchored[d]
    /\ card[d] # "pending"
    /\ receipt[d] \in {"none", "reverted"}
    /\ cardNo[d] < MaxCards
    /\ card' = [card EXCEPT ![d] = "pending"]
    /\ cardNo' = [cardNo EXCEPT ![d] = @ + 1]
    /\ receipt' = [receipt EXCEPT ![d] = "none"]
    /\ UNCHANGED <<deployed, enabled, sigs, signedOff, inflight, anchored>>

\* approve_and_broadcast: consume first, then sign once and send.
Approve(d) ==
    /\ card[d] = "pending"
    /\ card' = [card EXCEPT ![d] = "consumed"]
    /\ sigs' = [sigs EXCEPT ![<<d, cardNo[d]>>] = @ + 1]
    /\ signedOff' = (signedOff \/ ~enabled)
    /\ receipt' = [receipt EXCEPT ![d] = "pending"]
    /\ inflight' = [inflight EXCEPT ![d] = @ + 1]
    /\ UNCHANGED <<deployed, enabled, cardNo, anchored>>

Reject(d) ==
    /\ card[d] = "pending"
    /\ card' = [card EXCEPT ![d] = "consumed"]
    /\ UNCHANGED <<deployed, enabled, cardNo, sigs, signedOff, receipt, inflight, anchored>>

\* The chain decides: mined with status 1, or reverted.
Mine(d, r) ==
    /\ receipt[d] = "pending"
    /\ inflight[d] > 0
    /\ receipt' = [receipt EXCEPT ![d] = r]
    /\ inflight' = [inflight EXCEPT ![d] = @ - 1]
    /\ UNCHANGED <<deployed, enabled, card, cardNo, sigs, signedOff, anchored>>

\* settle: the ledger is told only on a confirming receipt.
Settle(d) ==
    /\ receipt[d] = "ok"
    /\ ~anchored[d]
    /\ anchored' = [anchored EXCEPT ![d] = TRUE]
    /\ UNCHANGED <<deployed, enabled, card, cardNo, sigs, signedOff, receipt, inflight>>

Next ==
    \/ Deploy \/ Enable \/ Disable
    \/ \E d \in Days :
        \/ Raise(d) \/ Approve(d) \/ Reject(d) \/ Settle(d)
        \/ \E r \in {"ok", "reverted"} : Mine(d, r)

Spec == Init /\ [][Next]_vars

(* ---- safety ---- *)

\* A day is anchored only after a mined receipt with status 1.
AnchoredOnlyOnConfirmedReceipt == \A d \in Days : anchored[d] => receipt[d] = "ok"

\* One approval, one signature.
SingleUseCard == \A x \in Days \X Cards : sigs[x] <= 1

\* Nothing is signed for a registry that is not deployed.
NoSignatureBeforeDeploy == (\E x \in Days \X Cards : sigs[x] > 0) => deployed

\* Turning anchoring off leaves no card that could still be approved, and nothing is signed
\* while it is off.
NoPendingCardWhileOff == ~enabled => \A d \in Days : card[d] # "pending"
NothingSignedWhileOff == ~signedOff

\* At most one anchor transaction per day is ever waiting on the chain (no double send).
AtMostOneInFlight == \A d \in Days : inflight[d] <= 1

\* An anchored day never gets another card (no second anchor of the same day).
NoCardAfterAnchored == \A d \in Days : anchored[d] => card[d] # "pending"

=============================================================================
