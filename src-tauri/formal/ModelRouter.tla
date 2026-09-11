---------------------------- MODULE ModelRouter ----------------------------
(***************************************************************************)
(* Formal model of the Hermes ModelRouter selection logic (Hermes P0 /      *)
(* WP0.1; the pure core in src/agent/modelRouter.ts). The router enumerates  *)
(* model choices from THREE sources — local (downloaded+verified GGUFs),     *)
(* the on-chain registry, and the always-ready gateway/Gemma — and holds a   *)
(* single active selection. This model pins the router's guarantees:         *)
(*                                                                          *)
(*   INV-Router-1  exactly one active selection (never two / a multiset).    *)
(*   INV-Router-2  the send path NEVER serves a not-ready model: it resolves *)
(*                 the active choice if ready, else the always-ready gateway. *)
(*   INV-Router-3  no phantom — the resolved backend is always a real         *)
(*                 enumerated choice (never a fabricated model id, Rule 1).   *)
(*   LIVE-Router-1 a selected not-ready model eventually becomes ready        *)
(*                 (download/verify or registry pull) — never a silent stuck  *)
(*                 state; meanwhile Resolved falls back to the gateway.       *)
(*                                                                          *)
(* Non-gateway choices start NOT ready (they must download/verify or pull);  *)
(* the gateway is always ready, so a fresh install works out of the box.     *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS
    LocalModels,     \* set of local (downloaded/verified) model ids
    RegistryModels,  \* set of on-chain-registry model ids
    Gateway          \* the single always-ready gateway/Gemma model id

VARIABLES
    ready,   \* [ Choices -> BOOLEAN ] : is a choice ready to serve?
    active   \* the selected choice id, or NoActive

NoActive == "NoActive"
Choices   == LocalModels \cup RegistryModels \cup {Gateway}
Preppable == LocalModels \cup RegistryModels   \* choices that must download/verify/pull

vars == <<ready, active>>

TypeOK ==
    /\ ready \in [Choices -> BOOLEAN]
    /\ active \in Choices \cup {NoActive}
    /\ ready[Gateway] = TRUE           \* the gateway is always ready

Init ==
    /\ ready = [ c \in Choices |-> (c = Gateway) ]  \* only the gateway is ready at start
    /\ active = Gateway                              \* default backend = gateway (out of the box)

(* The backend the send path actually resolves: the active choice iff it is    *)
(* ready, else the always-ready gateway. This is the load-bearing rule — the   *)
(* app never serves a not-ready local/registry model.                          *)
Resolved == IF active # NoActive /\ ready[active] THEN active ELSE Gateway

(* Select any enumerated choice, ready or not. Selecting a not-ready choice is  *)
(* allowed — it triggers the real download/pull; Resolved uses the gateway      *)
(* until it is ready.                                                           *)
Select(c) ==
    /\ c \in Choices
    /\ active' = c
    /\ UNCHANGED ready

(* A not-ready preppable choice finishes download+verify (local) or pull        *)
(* (registry) and becomes ready. Never touches the gateway.                     *)
BecomeReady(c) ==
    /\ c \in Preppable
    /\ ready[c] = FALSE
    /\ ready' = [ ready EXCEPT ![c] = TRUE ]
    /\ UNCHANGED active

Next ==
    \/ \E c \in Choices   : Select(c)
    \/ \E c \in Preppable : BecomeReady(c)

Fairness == \A c \in Preppable : WF_vars(BecomeReady(c))

Spec == Init /\ [][Next]_vars /\ Fairness

------------------------------------------------------------------------------
\* Safety invariants
INV_SingleActive == active \in Choices \cup {NoActive}
INV_ReadyToServe == ready[Resolved] = TRUE
INV_NoPhantom    == Resolved \in Choices

\* Liveness: a selected not-ready model eventually becomes ready (no stuck state).
LIVE_ReadyEventually == \A c \in Preppable : (active = c /\ ~ready[c]) ~> ready[c]
=============================================================================
