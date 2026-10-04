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
(* n5: the loaded adapter is remembered (active, activeBase) and re-applied *)
(* after an app restart and on a switch back to its base, only through the *)
(* same check a load makes. An eval run loads a candidate at scale 0: chats *)
(* (served) stay on the base model, the loaded adapter is suspended.       *)
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
    served,                          \* what a chat (a request naming no adapter) is answered with
    active, activeBase,              \* the remembered adapter and the base it was loaded on
    evalCand,                        \* the candidate of an eval run (scale 0), or NoAdapter
    restarted                        \* the app has restarted at least once (non-vacuity only)

vars == <<coord, settle, made, seenCoord, seenSettle, approved, started, startCoord,
          startSettle, content, gateVerdict, gateBase, gateSrc, copy, base, loaded, served,
          active, activeBase, evalCand, restarted>>
startVars == <<made, seenCoord, seenSettle, approved, started, startCoord, startSettle>>
memVars == <<active, activeBase, evalCand, restarted>>
loadVars == <<content, gateVerdict, gateBase, gateSrc, copy, base, loaded, served, memVars>>

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
    /\ active \in Versions \cup {NoAdapter} /\ activeBase \in Bases
    /\ evalCand \in Versions \cup {NoAdapter}
    /\ restarted \in BOOLEAN

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
    /\ active = NoAdapter /\ activeBase = CHOOSE b \in Bases : TRUE
    /\ evalCand = NoAdapter
    /\ restarted = FALSE

\* What a chat is answered with, given the configured adapter: nothing while an eval run holds
\* the candidate at scale 0 (the loaded adapter is suspended), else the bytes of the loaded copy.
ChatBytes(l, c, e) == IF e # NoAdapter \/ l = NoAdapter THEN NoAdapter ELSE c[l]

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
    \* FlRounds::record_gate: the remembered adapter is forgotten when its new record no longer
    \* allows it on the base it was loaded on.
    /\ IF active = v /\ (verdict = "REJECT" \/ b # activeBase)
          THEN active' = NoAdapter
          ELSE UNCHANGED active
    /\ UNCHANGED <<coord, settle, startVars, content, copy, base, activeBase, evalCand, restarted>>

\* The copy load would serve after refreshing it: a good copy is reused, a bad one is replaced from
\* the source, and the result is accepted only if it hashes to v.
Refreshed(v) == IF copy[v] = v THEN v ELSE content[gateSrc[v]]

Load(v) ==
    /\ evalCand = NoAdapter            \* fl_adapter_load refuses during an eval run
    /\ gateVerdict[v] = "ACCEPT"
    /\ gateBase[v] = base
    /\ IF Refreshed(v) = v
          THEN /\ copy' = [copy EXCEPT ![v] = v]
               /\ loaded' = v
               /\ served' = copy'[v]
               /\ active' = v /\ activeBase' = base
          ELSE /\ copy' = [copy EXCEPT ![v] = "none"]
               /\ UNCHANGED <<loaded, served, active, activeBase>>
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, base, evalCand, restarted>>

Unload ==
    /\ loaded' = NoAdapter /\ served' = NoAdapter
    /\ active' = NoAdapter
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, base,
                   activeBase, evalCand, restarted>>

\* fl_rounds::reapply_for: the remembered adapter, on its own base, only if a load would pass now.
ReapplyOk(b) ==
    /\ active # NoAdapter
    /\ activeBase = b
    /\ gateVerdict[active] = "ACCEPT" /\ gateBase[active] = b
    /\ Refreshed(active) = active
Reapplied(b) == IF ReapplyOk(b) THEN active ELSE NoAdapter
ReapplyCopy(b) == IF ReapplyOk(b) THEN [copy EXCEPT ![active] = active] ELSE copy

\* serve::select_inner: a different base asks the resolver; an eval run's candidate is dropped.
SelectBase(b) ==
    /\ base' = b
    /\ loaded' = IF b # base THEN Reapplied(b) ELSE loaded
    /\ copy' = IF b # base THEN ReapplyCopy(b) ELSE copy
    /\ evalCand' = IF b # base THEN NoAdapter ELSE evalCand
    /\ served' = ChatBytes(loaded', copy', evalCand')
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, active, activeBase, restarted>>

\* The app quits and launches again on some base model: fl_rounds::install_reapply.
AppRestart(b) ==
    /\ base' = b
    /\ loaded' = Reapplied(b)
    /\ copy' = ReapplyCopy(b)
    /\ evalCand' = NoAdapter          \* an eval run lives in memory only
    /\ served' = ChatBytes(loaded', copy', NoAdapter)
    /\ restarted' = TRUE
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, active, activeBase>>

\* fl_eval_begin: any staged version (hashed from a source file), loaded at scale 0.
EvalBegin(f) ==
    /\ evalCand = NoAdapter
    /\ evalCand' = content[f]
    /\ served' = ChatBytes(loaded, copy, content[f])
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, base, loaded,
                   active, activeBase, restarted>>

\* fl_eval_finish / fl_eval_end: the candidate is taken out, the loaded adapter comes back. (The
\* finish's gate record is the Gate action.)
EvalEnd ==
    /\ evalCand # NoAdapter
    /\ evalCand' = NoAdapter
    /\ served' = ChatBytes(loaded, copy, NoAdapter)
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, base, loaded,
                   active, activeBase, restarted>>

\* The supervisor restarts llama-server after a crash with the same argv: it re-reads --lora.
CrashRestart ==
    /\ served' = IF evalCand # NoAdapter \/ loaded = NoAdapter THEN NoAdapter ELSE copy[loaded]
    /\ UNCHANGED <<coord, settle, startVars, content, gateVerdict, gateBase, gateSrc, copy, base, loaded, memVars>>

Next ==
    \/ \E c \in Phases, s \in Settles : CoordChange(c, s)
    \/ \E f \in Files, v \in Versions : Swap(f, v)
    \/ \E v, w \in Versions : DamageCopy(v, w)
    \/ \E p \in Plans : Plan(p) \/ Approve(p) \/ Start(p)
    \/ \E f \in Files, d \in {"ACCEPT", "REJECT"}, b \in Bases : Gate(f, d, b)
    \/ \E v \in Versions : Load(v)
    \/ Unload
    \/ \E b \in Bases : SelectBase(b)
    \/ \E b \in Bases : AppRestart(b)
    \/ \E f \in Files : EvalBegin(f)
    \/ EvalEnd
    \/ CrashRestart

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

\* A chat is answered with exactly the gated version, across swaps, crash restarts, app restarts
\* and eval runs (during which it is the base model alone).
ServedIsLoaded == served = IF evalCand # NoAdapter THEN NoAdapter ELSE loaded

\* The goal: nothing that has not passed the gate for the served base ever answers a chat.
ChatOnlyAccepted == served # NoAdapter => gateVerdict[served] = "ACCEPT" /\ gateBase[served] = base

\* What a restart would bring back is still accepted for the base it was loaded on.
RememberedIsAccepted ==
    active # NoAdapter => gateVerdict[active] = "ACCEPT" /\ gateBase[active] = activeBase

(* ---- non-vacuity (expected to be VIOLATED, see FlRoundGate_Reach.cfg) ---- *)
NeverStarts == \A p \in Plans : started[p] = 0
NeverLoads == loaded = NoAdapter
NeverReappliedAfterRestart == ~(restarted /\ loaded # NoAdapter)
NeverEvals == evalCand = NoAdapter
=============================================================================
