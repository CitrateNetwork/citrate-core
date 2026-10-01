------------------------------ MODULE DeployGate ------------------------------
(***************************************************************************)
(* HUP-S6.4 — the D-4 deploy gate (src-tauri/src/deploy_gate.rs and       *)
(* contract_deploy.rs).                                                    *)
(*                                                                         *)
(* What is modelled                                                        *)
(*   - the member's working bytecode `src`, which can change at any time   *)
(*     (Edit). A "code" stands for one init code; hashing is modelled as   *)
(*     the identity (keccak256 assumed collision-free), so "the record     *)
(*     for hash h" is gate[c].                                             *)
(*   - the gate store: the latest verdict per code (Evaluate). Latest     *)
(*     wins, so a NOT_READY after a READY revokes it, and a NOT_READY      *)
(*     rejects every ceremony still open for that code, under the store    *)
(*     lock (GateStore::record_and_revoke).                                *)
(*   - contract_deploy as TWO steps, because the implementation reads the  *)
(*     bytes once and then opens the ceremony: Check (parse the init code  *)
(*     into `held[i]`, take the store lock, require READY) and Open (build *)
(*     the ceremony from `held[i]`, release the lock). The lock is what    *)
(*     GateStore::open_ceremony holds across the re-check and the request.*)
(*     A refused Check changes nothing (the honest error goes to the UI).  *)
(*   - the ceremony: Approve signs a pending ceremony (one signature per   *)
(*     approval), Reject drops it. `signed` logs (code, verdict at the     *)
(*     instant of signing).                                                *)
(*                                                                         *)
(* Abstractions                                                            *)
(*   - the evidence parsers are folded into the verdict Evaluate writes:   *)
(*     any failing item (including "not installed") yields NOT_READY.     *)
(*   - compiler settings are part of the binding hash in the code; here   *)
(*     they are folded into the code identity.                             *)
(*   - the size bound on the store (eviction revokes, like NOT_READY) is   *)
(*     not modelled; it only removes READY records.                        *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Codes,      \* distinct init codes (distinct hashes)
    Ids,        \* ceremony slots (bounds the number of deploy attempts)
    NoCode,     \* a value outside Codes
    NoLock      \* a value outside Ids (the store lock is free)

ASSUME NoCode \notin Codes /\ NoLock \notin Ids

Verdicts == {"none", "READY", "NOT_READY"}
CerStates == {"free", "checked", "pending", "signed", "rejected"}

VARIABLES
    src,        \* the member's current bytecode
    gate,       \* gate[c] = latest verdict for code c
    cer,        \* cer[i] = ceremony slot state
    held,       \* held[i] = the bytes read by Check for slot i (NoCode when none)
    code,       \* code[i] = the bytes the ceremony carries (NoCode before Open)
    lock,       \* the store lock: NoLock or the slot id holding it
    signed      \* set of <<code, verdict at signing>>

vars == <<src, gate, cer, held, code, lock, signed>>

TypeOK ==
    /\ src \in Codes
    /\ gate \in [Codes -> Verdicts]
    /\ cer \in [Ids -> CerStates]
    /\ held \in [Ids -> Codes \cup {NoCode}]
    /\ code \in [Ids -> Codes \cup {NoCode}]
    /\ lock \in Ids \cup {NoLock}
    /\ signed \subseteq (Codes \X Verdicts)

Init ==
    /\ src \in Codes
    /\ gate = [c \in Codes |-> "none"]
    /\ cer = [i \in Ids |-> "free"]
    /\ held = [i \in Ids |-> NoCode]
    /\ code = [i \in Ids |-> NoCode]
    /\ lock = NoLock
    /\ signed = {}

\* The member edits the source; the bytecode changes.
Edit(c) ==
    /\ c # src
    /\ src' = c
    /\ UNCHANGED <<gate, cer, held, code, lock, signed>>

\* deploy_gate_submit: store the verdict for c (latest wins). NOT_READY rejects the
\* ceremonies still open for c. Runs under the store lock.
Evaluate(c, v) ==
    /\ lock = NoLock
    /\ v \in {"READY", "NOT_READY"}
    /\ gate' = [gate EXCEPT ![c] = v]
    /\ cer' = [i \in Ids |->
                 IF v = "NOT_READY" /\ cer[i] = "pending" /\ code[i] = c
                 THEN "rejected" ELSE cer[i]]
    /\ UNCHANGED <<src, held, code, lock, signed>>

\* contract_deploy, step 1: read the bytes once, take the lock, require READY for them.
Check(i) ==
    /\ cer[i] = "free"
    /\ lock = NoLock
    /\ gate[src] = "READY"
    /\ held' = [held EXCEPT ![i] = src]
    /\ cer' = [cer EXCEPT ![i] = "checked"]
    /\ lock' = i
    /\ UNCHANGED <<src, gate, code, signed>>

\* contract_deploy, step 2: open the ceremony from the bytes that were checked; release.
Open(i) ==
    /\ cer[i] = "checked"
    /\ lock = i
    /\ code' = [code EXCEPT ![i] = held[i]]
    /\ cer' = [cer EXCEPT ![i] = "pending"]
    /\ lock' = NoLock
    /\ UNCHANGED <<src, gate, held, signed>>

\* The member approves the pending ceremony: exactly one signature over its bytes.
Approve(i) ==
    /\ cer[i] = "pending"
    /\ cer' = [cer EXCEPT ![i] = "signed"]
    /\ signed' = signed \cup {<<code[i], gate[code[i]]>>}
    /\ UNCHANGED <<src, gate, held, code, lock>>

Reject(i) ==
    /\ cer[i] = "pending"
    /\ cer' = [cer EXCEPT ![i] = "rejected"]
    /\ UNCHANGED <<src, gate, held, code, lock, signed>>

Next ==
    \/ \E c \in Codes : Edit(c)
    \/ \E c \in Codes, v \in {"READY", "NOT_READY"} : Evaluate(c, v)
    \/ \E i \in Ids : Check(i) \/ Open(i) \/ Approve(i) \/ Reject(i)

Spec == Init /\ [][Next]_vars

---------------------------------------------------------------------------
(* Invariants. Stated over the logged facts (signed, cer, code, held), not  *)
(* with the guards the actions use.                                        *)

\* DeployImpliesGateGreenForSameHash / "NOT READY never signs": every signature
\* was made while the latest verdict for exactly the signed bytes was READY.
NotReadyNeverSigns ==
    \A e \in signed : e[2] = "READY"

\* "Any change of bytecode invalidates READY": a ceremony that can still be
\* approved carries bytes whose own latest verdict is READY. A READY for other
\* bytes (an earlier version of the source) never covers it.
OpenOnlyForReadyBytes ==
    \A i \in Ids : cer[i] = "pending" => gate[code[i]] = "READY"

\* NoTOCTOU: the bytes a ceremony carries are the bytes the gate check read,
\* whatever the source became in between.
NoTOCTOU ==
    \A i \in Ids : cer[i] \in {"pending", "signed"} => code[i] = held[i]

\* Liveness-free sanity: the model can actually sign (guards against vacuity).
\* Checked as an invariant that TLC is expected to VIOLATE in DeployGate_Reach.cfg.
NeverSigns == signed = {}

=============================================================================
