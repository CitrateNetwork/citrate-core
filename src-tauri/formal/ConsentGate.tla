--------------------------- MODULE ConsentGate ---------------------------
(***************************************************************************)
(* Telemetry WP-T.1 — the consent gate for diagnostic reports.            *)
(*                                                                         *)
(* Citrate telemetry is private, local, opt-in, anonymous. NOTHING leaves *)
(* the device except a bundle the member explicitly reviewed AND consented*)
(* to send, and only while the telemetry toggle is on. This models the     *)
(* gate that `telemetry_send` (the one pinned HTTPS POST) must satisfy.    *)
(*                                                                         *)
(* State:                                                                  *)
(*   toggle    : the Settings telemetry switch (default OFF)               *)
(*   bundles   : captured diagnostic bundles, each with flags              *)
(*                 reviewed  (the member saw the scrubbed JSON)            *)
(*                 consented (the member clicked Send for THIS bundle)     *)
(*   egressed  : the set of bundle ids that were POSTed                    *)
(*   sentBody  : id :> the body actually sent (to check it == reviewed)    *)
(*   reviewed_body : id :> the exact body the member reviewed              *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   INV-Consent-1 : every egressed bundle had a consent event for THAT id *)
(*   INV-Consent-2 : toggle OFF ⇒ no new egress                            *)
(*   INV-Consent-3 : what was sent == what was reviewed (no swap/mutate)   *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS Bundles   \* a finite universe of bundle ids

VARIABLES
    toggle,        \* BOOLEAN — the telemetry switch
    reviewed,      \* SUBSET Bundles — reviewed by the member
    consented,     \* SUBSET Bundles — consent-to-send clicked
    egressed,      \* SUBSET Bundles — actually POSTed
    reviewedBody,  \* [Bundles -> Nat] — the body content the member reviewed (0 = none)
    sentBody       \* [Bundles -> Nat] — the body content actually sent (0 = none)

vars == <<toggle, reviewed, consented, egressed, reviewedBody, sentBody>>

TypeOK ==
    /\ toggle \in BOOLEAN
    /\ reviewed \subseteq Bundles
    /\ consented \subseteq Bundles
    /\ egressed \subseteq Bundles
    /\ reviewedBody \in [Bundles -> Nat]
    /\ sentBody \in [Bundles -> Nat]

Init ==
    /\ toggle = FALSE          \* default OFF (opt-in)
    /\ reviewed = {}
    /\ consented = {}
    /\ egressed = {}
    /\ reviewedBody = [b \in Bundles |-> 0]
    /\ sentBody = [b \in Bundles |-> 0]

ToggleOn  == /\ toggle' = TRUE  /\ UNCHANGED <<reviewed, consented, egressed, reviewedBody, sentBody>>
(* Turning the toggle OFF stops all egress — enforced by Egress's `toggle = TRUE` guard, so
   nothing new can leave while off (INV-Consent-2). We do NOT erase the per-bundle consent
   RECORD: consent for an already-sent bundle is a historical fact, and INV-Consent-1 checks
   that every egressed bundle was consented — clearing it would falsely fail that. *)
ToggleOff == /\ toggle' = FALSE /\ UNCHANGED <<reviewed, consented, egressed, reviewedBody, sentBody>>

(* The member opens the review pane for bundle b: they see its scrubbed body (modelled as
   a nonzero content value). Reviewing does not send. *)
Review(b) ==
    /\ b \notin egressed
    /\ reviewed' = reviewed \cup {b}
    /\ reviewedBody' = [reviewedBody EXCEPT ![b] = 1]  \* the reviewed content
    /\ UNCHANGED <<toggle, consented, egressed, sentBody>>

(* The member clicks Send for b. Guarded: toggle ON and b already reviewed. This records
   consent for THAT bundle. *)
Consent(b) ==
    /\ toggle = TRUE
    /\ b \in reviewed
    /\ consented' = consented \cup {b}
    /\ UNCHANGED <<toggle, reviewed, egressed, reviewedBody, sentBody>>

(* The one pinned HTTPS POST. It may fire ONLY for a bundle that is (a) consented and
   (b) reviewed, and ONLY while the toggle is on; it sends exactly the reviewed body. *)
Egress(b) ==
    /\ toggle = TRUE
    /\ b \in consented
    /\ b \in reviewed
    /\ b \notin egressed
    /\ egressed' = egressed \cup {b}
    /\ sentBody' = [sentBody EXCEPT ![b] = reviewedBody[b]]  \* sent == reviewed
    /\ UNCHANGED <<toggle, reviewed, consented, reviewedBody>>

Next ==
    \/ ToggleOn \/ ToggleOff
    \/ \E b \in Bundles : Review(b) \/ Consent(b) \/ Egress(b)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* INV-Consent-1 — nothing egresses without a consent event for THAT bundle. *)
INV_Consent_1 == \A b \in egressed : b \in consented

(* INV-Consent-3 — what was sent is exactly what was reviewed (nonzero + equal). *)
INV_Consent_3 == \A b \in egressed : sentBody[b] = reviewedBody[b] /\ sentBody[b] # 0

(* INV-Consent-2 is an ACTION property: no egress step may occur while the toggle is off.
   (Egress's guard requires toggle = TRUE, so egressed never grows on a toggle-off state.) *)
INV_Consent_2 == [][ (egressed' # egressed) => toggle ]_vars

=============================================================================
