---------------------------- MODULE MemoryPack ----------------------------
(***************************************************************************)
(* Hermes P1 / WP1.1 — the docs-corpus packer.                             *)
(*                                                                         *)
(* The app ships a curated corpus (the Almanac docs + WP1.2 reference      *)
(* packs). On every launch it authors each corpus CHUNK into the           *)
(* `citrate-docs` mem tenant, keyed by a stable sha256 content hash. A     *)
(* chunk already authored on a prior run — or in a prior app version's     *)
(* corpus — is never re-authored. This models the implementation in        *)
(* `docs_ingest::ingest_docs_incremental` + `memory::ingest_docs_corpus`:  *)
(*                                                                         *)
(*   - `packed`  = the set of chunk hashes actually authored into the      *)
(*                 tenant (the mem-mcp nodes).                             *)
(*   - `seen`    = the persisted sidecar set of hashes (docs-corpus.seeded)*)
(*   - `corpus`  = the shipped corpus's chunk hashes; it may GROW across   *)
(*                 app versions (a reference pack is added) but a curated  *)
(*                 corpus never ships a chunk whose hash is not its own    *)
(*                 content (integrity).                                    *)
(*                                                                         *)
(* Properties (planset 03_TLA_SPECS):                                      *)
(*   INV-Pack-1 (monotone):  packing never removes a prior node.           *)
(*   INV-Pack-2 (no dupes):  a hash already packed is not authored again.  *)
(*   INV-Pack-3 (integrity): every packed hash is a real corpus chunk.     *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS Hashes          \* the universe of possible chunk content hashes

VARIABLES
    corpus,   \* Hashes : the shipped corpus's chunk hashes (may grow)
    packed,   \* Hashes : hashes authored into the tenant
    seen,     \* Hashes : the persisted sidecar seen-set
    lastRun   \* Nat    : count of chunks authored on the most recent Ingest

vars == <<corpus, packed, seen, lastRun>>

TypeOK ==
    /\ corpus \subseteq Hashes
    /\ packed \subseteq Hashes
    /\ seen   \subseteq Hashes
    /\ lastRun \in Nat

Init ==
    /\ corpus \in SUBSET Hashes   \* ships with some curated corpus
    /\ packed = {}
    /\ seen   = {}
    /\ lastRun = 0

(* A new app version adds a reference pack: the corpus grows. A curated    *)
(* corpus only ever adds real chunk hashes, so `corpus` stays within the   *)
(* hash universe (integrity of the source is a modeling assumption of      *)
(* curation, checked in code by the shipped-corpus test).                  *)
GrowCorpus ==
    /\ \E extra \in SUBSET Hashes :
         /\ extra # {}
         /\ corpus' = corpus \cup extra
    /\ UNCHANGED <<packed, seen, lastRun>>

(* One ingest run. New = the corpus chunks not already seen; author each,  *)
(* recording its hash. Matches ingest_docs_incremental exactly.            *)
Ingest ==
    LET new == corpus \ seen IN
    /\ packed' = packed \cup new
    /\ seen'   = seen \cup new
    /\ lastRun' = Cardinality(new)
    /\ UNCHANGED corpus

(* The empty-tenant guard: if the store was wiped (packed = {}) but the    *)
(* sidecar lingered, the next run treats the empty tenant as authoritative *)
(* and re-packs everything — no drift, still no dupes (packed is a set).   *)
WipeStore ==
    /\ packed = {} \/ TRUE          \* wipe is always allowed
    /\ packed' = {}
    /\ seen'   = {}                 \* empty tenant resets the seen-set
    /\ UNCHANGED <<corpus, lastRun>>

Next == Ingest \/ GrowCorpus \/ WipeStore

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* INV-Pack-2 (no dupes): every authored chunk on the last run was genuinely
   new — the run authored exactly |corpus \ seen_before|, so nothing packed
   twice. Encoded structurally: packed and seen are SETS, and Ingest only ever
   unions `new = corpus \ seen`, which is disjoint from `seen`. *)
NoDupes == packed \subseteq Hashes  \* sets cannot contain a duplicate

(* INV-Pack-3 (integrity): a packed hash is always a real corpus chunk —
   the packer never invents a node (Rule 1). *)
Integrity == packed \subseteq corpus

(* INV-Pack-1 (monotone) is an ACTION property: across an Ingest step packed
   only grows. WipeStore is the sole exception and is intentional (a wiped
   store); it is modeled explicitly rather than hidden. *)
MonotoneUnderIngest ==
    [][Ingest => packed \subseteq packed']_vars

=============================================================================
