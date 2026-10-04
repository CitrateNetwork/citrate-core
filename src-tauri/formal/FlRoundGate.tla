---------------------------- MODULE FlRoundGate ----------------------------
(***************************************************************************)
(* HUP-S9.4: starting a federated round (HIC-1) and loading its LoRA      *)
(* adapter behind the eval gate. Mirrors src-tauri/src/fl_rounds.rs.       *)
(*                                                                         *)
(* Start: core builds a plan from a coordinator read; the member approves  *)
(* that exact plan; start re-reads the coordinator and records at most one *)
(* authorization, only while the work is still open and the settlement     *)
(* mode is the one the member saw. The environment changes the coordinator *)
(* at any time.                                                            *)
(*                                                                         *)
(* Load: gate records are keyed by content hash (a version). A load needs  *)
(* the latest record for that hash to be ACCEPT for the served base, and   *)
(* serves a content-addressed copy whose bytes were re-hashed. The source  *)
(* file the member picked can be swapped at any time; llama-server re-reads *)
(* its --lora file on every restart. Assumption: the app-owned copy is not *)
(* modified while it is being served (same trust as the model files); it   *)
(* may be damaged while not served, and a load re-hashes it.               *)
(*                                                                         *)
(* Re-apply (fan-out 6): a load is remembered in fl_rounds.json (`saved`). *)
(* An app restart serves nothing until the start path re-applies `saved`   *)
(* through the same checks as a load. `wanted` is a ghost: the member's    *)
(* standing intent, set by Load and ended by Unload, a base switch while   *)
(* loaded, or a gate record that no longer allows it on its base.          *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS Plans, Files, Versions, Bases, NoAdapter

Phases == {"none", "down", "open", "done"}
Settles == {"shadow", "live"}
Verdicts == {"none", "ACCEPT", "REJECT"}

VARIABLES
    coord, settle,                   \* the coordinator now
    made, seenCoord, seenSettle,     \* plans core built and what each read
    approved,                        \* the member's HIC-1 decision per plan
    started, startCoord, startSettle,\* start records and what the re-read saw
    content,                         \* content[f]: the version the source file holds now
    gateVerdict, gateBase, gateSrc,  \* gate records, keyed by version (sha256)
    copy,                            \* copy[v]: the version stored at adapters/<v>.gguf, or "none"
    base,                            \* the served base model
    loaded,                          \* the adapter (version) llama-server is configured with
    served,                          \* the bytes the running llama-server read at its last spawn
    saved, savedBase,                \* the remembered adapter and the base it was loaded on
    wanted,                          \* ghost: the member's standing intent
    restored                         \* ghost: a restart re-applied an adapter at least once

vars == <<coord, settle, made, seenCoord, seenSettle, approved, started, startCoord,
          startSettle, content, gateVerdict, gateBase, gateSrc, copy, base, loaded, served,
          saved, savedBase, wanted, restored>>
startVars == <<made, seenCoord, seenSettle, approved, started, startCoord, startSettle>>
memVars == <<saved, savedBase, wanted, restored>>
loadVars == <<content, gateVerdict, gateBase, gateSrc, copy, base, loaded, served, saved, savedBase, wanted, restored>>

TypeOK ==
    /\ coord \in Phases /\ settle \in Settles
    /\ made \in [Plans -> BOOLEAN]
    /\ seenCoord \in [Plans -> Phases] /\ seenSettle \in [Plans -> Settles]
    /\ approved \in [Plans -> BOOLEAN]
    /\ started \in [Plans -> Nat]
    /\ startCoord \in [Plans -> Phases] /\ startSettle \in [Plans -> Settles]
    /\ content \in [Files -> Versions]
    /\ gateVerdict \in [Versions -> Verdicts]
    /\ gateBase \in [Versions -> Bases] /\ gateSrc \in [Versions -> Files]
    /\ copy \in [Versions -> Versions \cup {"none"}]
    /\ base \in Bases
    /\ loaded \in Versions \cup {NoAdapter}
    /\ served \in Versions \cup {"none", NoAdapter}
    /\ saved \in Versions \cup {NoAdapter} /\ savedBase \in Bases
    /\ wanted \in Versions \cup {NoAdapter}
    /\ restored \in BOOLEAN

Init ==
    /\ coord \in Phases /\ settle \in Settles
    /\ made = [p \in Plans |-> FALSE]
    /\ seenCoord = [p \in Plans |-> "none"] /\ seenSettle = [p \in Plans |-> "shadow"]
    /\ approved = [p \in Plans |-> FALSE]
    /\ started = [p \in Plans |-> 0]
    /\ startCoord = [p \in Plans |-> "none"] /\ startSettle = [p \in Plans |-> "shadow"]
    /\ content \in [Files -> Versions]
    /\ gateVerdict = [v \in Versions |-> "none"]
    /\ gateBase = [v \in Versions |-> CHOOSE b \in Bases : TRUE]
    /\ gateSrc = [v \in Versions |-> CHOOSE f \in Files : TRUE]
    /\ copy = [v \in Versions |-> "none"]
    /\ base \in Bases
    /\ loaded = NoAdapter
    /\ served = NoAdapter
    /\ saved = NoAdapter /\ savedBase = CHOOSE b \in Bases : TRUE
    /\ wanted = NoAdapter
    /\ restored = FALSE

(* ---- environment ---- *)
CoordChange(c, s) ==
    /\ coord' = c /\ settle' = s
    /\ UNCHANGED <<startVars, loadVars>>

Swap(f, v) ==
    /\ content' = [content EXCEPT ![f] = v]
    /\ UNCHANGED <<coord, settle, startVars, gateVerdict, gateBase, gateSrc, copy, base, loaded, served, memVars>>

DamageCopy(v, w) ==
    /\ copy[v] # "none" /\ loaded # v
    /\ copy' = [copy EXCEPT ![v] = w]
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, base, loaded, served, memVars>>

(* ---- start ---- *)
Plan(p) ==
    /\ ~made[p]
    /\ made' = [made EXCEPT ![p] = TRUE]
    /\ seenCoord' = [seenCoord EXCEPT ![p] = coord]
    /\ seenSettle' = [seenSettle EXCEPT ![p] = settle]
    /\ UNCHANGED <<coord, settle, approved, started, startCoord, startSettle, loadVars>>

Approve(p) ==
    /\ made[p] /\ ~approved[p]
    /\ approved' = [approved EXCEPT ![p] = TRUE]
    /\ UNCHANGED <<coord, settle, made, seenCoord, seenSettle, started, startCoord, startSettle, loadVars>>

Start(p) ==
    /\ approved[p]
    /\ made[p] /\ seenCoord[p] = "open"
    /\ started[p] = 0
    /\ coord = "open"
    /\ settle = seenSettle[p]
    /\ started' = [started EXCEPT ![p] = @ + 1]
    /\ startCoord' = [startCoord EXCEPT ![p] = coord]
    /\ startSettle' = [startSettle EXCEPT ![p] = settle]
    /\ UNCHANGED <<coord, settle, made, seenCoord, seenSettle, approved, loadVars>>

(* ---- gate + load ---- *)
Gate(f, verdict, b) ==
    LET v == content[f] IN
    /\ gateVerdict' = [gateVerdict EXCEPT ![v] = verdict]
    /\ gateBase' = [gateBase EXCEPT ![v] = b]
    /\ gateSrc' = [gateSrc EXCEPT ![v] = f]
    \* fl_rounds::must_unload_after_gate: the served adapter goes when its new record no longer
    \* allows it on the served base.
    /\ IF loaded = v /\ (verdict = "REJECT" \/ b # base)
          THEN /\ loaded' = NoAdapter /\ served' = NoAdapter
          ELSE UNCHANGED <<loaded, served>>
    \* FlRounds::record_gate: a record that no longer allows the remembered adapter on the base it
    \* was loaded on ends the re-apply.
    /\ IF saved = v /\ (verdict = "REJECT" \/ b # savedBase)
          THEN saved' = NoAdapter
          ELSE UNCHANGED saved
    /\ wanted' = IF wanted = v /\ (verdict = "REJECT" \/ b # savedBase) THEN NoAdapter ELSE wanted
    /\ UNCHANGED <<coord, settle, startVars, content, copy, base, savedBase, restored>>

\* The copy load would serve after refreshing it: a good copy is reused, a bad one is replaced from
\* the source, and the result is accepted only if it hashes to v.
Refreshed(v) == IF copy[v] = v THEN v ELSE content[gateSrc[v]]

Load(v) ==
    /\ gateVerdict[v] = "ACCEPT"
    /\ gateBase[v] = base
    /\ IF Refreshed(v) = v
          THEN /\ copy' = [copy EXCEPT ![v] = v]
               /\ loaded' = v
               /\ served' = copy'[v]
               /\ saved' = v /\ savedBase' = base /\ wanted' = v
               /\ UNCHANGED restored
          ELSE /\ copy' = [copy EXCEPT ![v] = "none"]
               /\ UNCHANGED <<loaded, served, memVars>>
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, base>>

Unload ==
    /\ loaded' = NoAdapter /\ served' = NoAdapter
    /\ saved' = NoAdapter /\ wanted' = NoAdapter
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, base, savedBase, restored>>

\* model_catalog_select: a switch while an adapter is loaded drops it and forgets it.
SelectBase(b) ==
    /\ base' = b
    /\ loaded' = IF b # base THEN NoAdapter ELSE loaded
    /\ served' = IF b # base THEN NoAdapter ELSE served
    /\ IF b # base /\ loaded # NoAdapter
          THEN saved' = NoAdapter /\ wanted' = NoAdapter
          ELSE UNCHANGED <<saved, wanted>>
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, savedBase, restored>>

\* The supervisor restarts llama-server after a crash with the same argv: it re-reads --lora.
CrashRestart ==
    /\ served' = IF loaded = NoAdapter THEN NoAdapter ELSE copy[loaded]
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, base, loaded, memVars>>

\* The app quits and starts again on some base: nothing is served, the store survives.
AppRestart(b) ==
    /\ base' = b /\ loaded' = NoAdapter /\ served' = NoAdapter
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, memVars>>

\* fl_rounds::reapply_before_start + restore_active: the remembered adapter goes back only on the
\* base it was loaded on, through the same checks and re-hash as Load. Not a new intent.
Restore ==
    /\ loaded = NoAdapter /\ saved # NoAdapter
    /\ savedBase = base
    /\ gateVerdict[saved] = "ACCEPT"
    /\ gateBase[saved] = base
    /\ IF Refreshed(saved) = saved
          THEN /\ copy' = [copy EXCEPT ![saved] = saved]
               /\ loaded' = saved
               /\ served' = copy'[saved]
               /\ restored' = TRUE
          ELSE /\ copy' = [copy EXCEPT ![saved] = "none"]
               /\ UNCHANGED <<loaded, served, restored>>
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, base, saved, savedBase, wanted>>

Next ==
    \/ \E c \in Phases, s \in Settles : CoordChange(c, s)
    \/ \E f \in Files, v \in Versions : Swap(f, v)
    \/ \E v, w \in Versions : DamageCopy(v, w)
    \/ \E p \in Plans : Plan(p) \/ Approve(p) \/ Start(p)
    \/ \E f \in Files, d \in {"ACCEPT", "REJECT"}, b \in Bases : Gate(f, d, b)
    \/ \E v \in Versions : Load(v)
    \/ Unload
    \/ \E b \in Bases : SelectBase(b)
    \/ CrashRestart
    \/ \E b \in Bases : AppRestart(b)
    \/ Restore

Spec == Init /\ [][Next]_vars

(* ---- safety ---- *)
\* No round is started without the member's HIC-1 approval of that plan.
StartOnlyApproved == \A p \in Plans : started[p] > 0 => approved[p]

\* A start happens only while the work is open, as the plan said, under the settlement the member saw.
StartOnlyWhatWasApproved ==
    \A p \in Plans : started[p] > 0 =>
        /\ seenCoord[p] = "open" /\ startCoord[p] = "open"
        /\ startSettle[p] = seenSettle[p]

AtMostOneStart == \A p \in Plans : started[p] <= 1

\* What llama-server is configured with is accepted for the served base.
LoadedIsAccepted ==
    loaded # NoAdapter => gateVerdict[loaded] = "ACCEPT" /\ gateBase[loaded] = base

\* What llama-server actually read is exactly the gated version, across swaps and crash restarts.
ServedIsLoaded == served = loaded

\* Only what the member loaded and has not since unloaded (or lost to a gate record or a base
\* switch) is ever configured, across restarts.
LoadedIsWanted == loaded # NoAdapter => loaded = wanted

(* ---- non-vacuity (expected to be VIOLATED, see FlRoundGate_Reach.cfg) ---- *)
NeverStarts == \A p \in Plans : started[p] = 0
NeverLoads == loaded = NoAdapter
\* A restart can put an adapter back (expected VIOLATED, see FlRoundGate_ReachRestore.cfg).
NeverRestores == ~restored
=============================================================================
