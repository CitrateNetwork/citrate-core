----------------------------- MODULE ComponentSwap -----------------------------
(***************************************************************************)
(* HUP-S5.5: the signed component updater's verify-then-swap install      *)
(* (components/src/install.rs, manifest.rs).                               *)
(*                                                                         *)
(* What is modelled                                                        *)
(*   - manifests as (seq, ver) pairs. Begin verifies a manifest, refuses   *)
(*     one older than the last seen (anti-rollback), records it, and      *)
(*     starts an install of its version.                                   *)
(*   - an install in four steps: Fetch (into staging), Verify (size,      *)
(*     SHA-256, artifact signature and the health check, folded into one  *)
(*     outcome: only versions in Good pass), Rename (the unpacked tree     *)
(*     moves into place) and Commit (the state file is replaced: the       *)
(*     single commit point).                                               *)
(*   - Crash at any step, followed by Recover (startup): staging and every *)
(*     version directory the state file does not reference are removed.   *)
(*   - Rollback: previous and current swap, if the previous dir exists.    *)
(*                                                                         *)
(* Abstractions                                                            *)
(*   - signatures, hashes and the health check are one predicate (Good).  *)
(*     A version outside Good stands for a tampered or broken artifact.   *)
(*   - a version is identified with its directory (version + hash prefix).*)
(*   - "AlreadyCurrent" (same version and hash) is a no-op and is left    *)
(*     out: Begin only starts installs of a version that is not current.  *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    Vers,       \* artifact versions offered by manifests
    Good,       \* the versions whose artifact verifies and passes the health check
    MaxSeq,     \* manifests carry sequence numbers 1..MaxSeq
    None

ASSUME Good \subseteq Vers /\ None \notin Vers

Manifests == [seq : 1..MaxSeq, ver : Vers]

VARIABLES
    seen,       \* the last manifest sequence recorded in the state file
    phase,      \* "idle" | "fetched" | "verified" | "renamed" | "crashed"
    job,        \* the manifest being installed, or None
    cur,        \* the current version in the state file, or None
    prev,       \* the previous version in the state file, or None
    dirs        \* version directories on disk

vars == <<seen, phase, job, cur, prev, dirs>>

TypeOK ==
    /\ seen \in 0..MaxSeq
    /\ phase \in {"idle", "fetched", "verified", "renamed", "crashed"}
    /\ job \in Manifests \cup {None}
    /\ cur \in Vers \cup {None}
    /\ prev \in Vers \cup {None}
    /\ dirs \subseteq Vers

Init ==
    /\ seen = 0
    /\ phase = "idle"
    /\ job = None
    /\ cur = None
    /\ prev = None
    /\ dirs = {}

\* verify_manifest + record_manifest: a lower sequence is refused (rollback).
Begin(m) ==
    /\ phase = "idle"
    /\ m.seq >= seen
    /\ m.ver # cur
    /\ seen' = m.seq
    /\ job' = m
    /\ phase' = "fetched"
    /\ UNCHANGED <<cur, prev, dirs>>

\* Size, SHA-256, artifact signature, unpack and health check. A failure clears staging
\* and changes nothing else.
Verify ==
    /\ phase = "fetched"
    /\ IF job.ver \in Good
         THEN /\ phase' = "verified"
              /\ job' = job
         ELSE /\ phase' = "idle"
              /\ job' = None
    /\ UNCHANGED <<seen, cur, prev, dirs>>

\* The verified tree is renamed into <root>/<name>/<version>-<sha12>.
Rename ==
    /\ phase = "verified"
    /\ dirs' = dirs \cup {job.ver}
    /\ phase' = "renamed"
    /\ UNCHANGED <<seen, job, cur, prev>>

\* The state file is replaced (the commit point); the version that falls off is pruned.
Commit ==
    /\ phase = "renamed"
    /\ cur' = job.ver
    /\ prev' = cur
    /\ dirs' = IF prev # None /\ prev # job.ver /\ prev # cur
                 THEN dirs \ {prev}
                 ELSE dirs
    /\ phase' = "idle"
    /\ job' = None
    /\ UNCHANGED seen

Crash ==
    /\ phase \in {"fetched", "verified", "renamed"}
    /\ phase' = "crashed"
    /\ UNCHANGED <<seen, job, cur, prev, dirs>>

\* Store::recover at startup.
Recover ==
    /\ phase = "crashed"
    /\ dirs' = {d \in dirs : d = cur \/ d = prev}
    /\ phase' = "idle"
    /\ job' = None
    /\ UNCHANGED <<seen, cur, prev>>

Rollback ==
    /\ phase = "idle"
    /\ prev # None
    /\ prev \in dirs
    /\ cur' = prev
    /\ prev' = cur
    /\ UNCHANGED <<seen, phase, job, dirs>>

Next ==
    \/ \E m \in Manifests : Begin(m)
    \/ Verify
    \/ Rename
    \/ Commit
    \/ Crash
    \/ Recover
    \/ Rollback

Spec == Init /\ [][Next]_vars

(* ---------------------------- properties ------------------------------ *)

\* Only a version that verified is ever current or previous.
CurrentVerified == /\ cur # None => cur \in Good
                   /\ prev # None => prev \in Good

\* What the state file names is always on disk (a crash never leaves a dangling pointer).
InstalledOnDisk == /\ cur # None => cur \in dirs
                   /\ prev # None => prev \in dirs

\* Nothing unverified is ever placed among the version directories.
OnlyVerifiedOnDisk == dirs \subseteq Good

\* An install in progress comes from the newest recorded manifest.
JobIsNewest == job # None => job.seq = seen

\* Between installs only the current and the previous version are on disk; during one, at
\* most one more.
BoundedDisk == /\ Cardinality(dirs) <= 3
               /\ phase = "idle" => Cardinality(dirs) <= 2

\* The recorded sequence never goes down.
SeenMonotone == [][seen' >= seen]_vars
=============================================================================
