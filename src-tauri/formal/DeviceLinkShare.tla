---------------------------- MODULE DeviceLinkShare ----------------------------
(***************************************************************************)
(* HUP-S8.1 follow-on: DeviceLinks shared between members over the group  *)
(* relay (src-tauri/src/device_link_share.rs: `share_offer`, `ingest`,    *)
(* `roster_update`).                                                       *)
(*                                                                         *)
(* What is modelled                                                        *)
(*   - each member links its own devices (Link) and revokes them (Revoke).*)
(*     A link names its member; it is valid only if that member really    *)
(*     linked that device (the three signatures in the code). A          *)
(*     revocation is valid only if its member signed it.                  *)
(*   - Announce: a member puts its full current set on the relay. The     *)
(*     relay is a set of messages, so delivery is out of order, repeated  *)
(*     and may never happen; the relay attributes each message to its    *)
(*     real sender.                                                        *)
(*   - an adversary member sends messages with forged or relayed items    *)
(*     (other members' links, links it never signed, revocations it did  *)
(*     not sign).                                                          *)
(*   - Ingest: a node takes any message from the relay, keeping only the  *)
(*     sender's own valid items; revocations first and for good; nothing  *)
(*     from itself.                                                        *)
(*   - Update: what the node sends its cluster daemon (own links, then    *)
(*     peers' links), never a revoked one.                                 *)
(*                                                                         *)
(* Abstractions                                                            *)
(*   - one node per member; the per-machine split is ADR / cluster-side    *)
(*     (formal/DeviceLink.tla in citrate-cluster).                         *)
(*   - signatures are the validity predicates below (secp256k1 recovery   *)
(*     is unit-tested in Rust, with flipped-byte forgeries).               *)
(*   - caps and the IPC-line trimming are unit-tested in Rust; trimming   *)
(*     only removes items, which preserves every invariant here.          *)
(*   - roster membership filtering is not modelled (it only removes      *)
(*     items from Update).                                                 *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Members,    \* member ids (one node each)
    Devices,    \* device keys
    Owner,      \* Devices -> Members: whose device each key is
    Mallory,    \* the adversary member (an element of Members)
    MaxNet      \* bound on relay messages (state space)

ASSUME Owner \in [Devices -> Members] /\ Mallory \in Members

Pairs == Members \X Devices

VARIABLES
    ownLinks,   \* member -> devices it currently links
    ownRevs,    \* member -> devices it revoked (signed revocations)
    everLinked, \* member -> devices it ever signed a link for (signatures exist forever)
    net,        \* relay: set of [from, links, revs] with links/revs sets of <<member, device>>
    peerLinks,  \* node -> accepted other members' links (set of pairs)
    peerRevs    \* node -> accepted other members' revocations (set of pairs)

vars == <<ownLinks, ownRevs, everLinked, net, peerLinks, peerRevs>>

ValidLink(l) == l[2] \in everLinked[l[1]]
ValidRev(r)  == r[2] \in ownRevs[r[1]]

Init ==
    /\ ownLinks = [m \in Members |-> {}]
    /\ ownRevs = [m \in Members |-> {}]
    /\ everLinked = [m \in Members |-> {}]
    /\ net = {}
    /\ peerLinks = [m \in Members |-> {}]
    /\ peerRevs = [m \in Members |-> {}]

Link(m, d) ==
    /\ Owner[d] = m
    /\ d \notin ownRevs[m]
    /\ d \notin ownLinks[m]
    /\ ownLinks' = [ownLinks EXCEPT ![m] = @ \cup {d}]
    /\ everLinked' = [everLinked EXCEPT ![m] = @ \cup {d}]
    /\ UNCHANGED <<ownRevs, net, peerLinks, peerRevs>>

Revoke(m, d) ==
    /\ d \in ownLinks[m]
    /\ ownLinks' = [ownLinks EXCEPT ![m] = @ \ {d}]
    /\ ownRevs' = [ownRevs EXCEPT ![m] = @ \cup {d}]
    /\ UNCHANGED <<everLinked, net, peerLinks, peerRevs>>

\* share_offer: only this member's own links and revocations.
Announce(m) ==
    /\ Cardinality(net) < MaxNet
    /\ net' = net \cup {[from |-> m,
                         links |-> {<<m, d>> : d \in ownLinks[m]},
                         revs |-> {<<m, d>> : d \in ownRevs[m]}]}
    /\ UNCHANGED <<ownLinks, ownRevs, everLinked, peerLinks, peerRevs>>

\* The adversary sends one arbitrary item (forged, relayed, or replayed).
ForgeLink(l) ==
    /\ Cardinality(net) < MaxNet
    /\ net' = net \cup {[from |-> Mallory, links |-> {l}, revs |-> {}]}
    /\ UNCHANGED <<ownLinks, ownRevs, everLinked, peerLinks, peerRevs>>

ForgeRev(r) ==
    /\ Cardinality(net) < MaxNet
    /\ net' = net \cup {[from |-> Mallory, links |-> {}, revs |-> {r}]}
    /\ UNCHANGED <<ownLinks, ownRevs, everLinked, peerLinks, peerRevs>>

\* ingest: from the sender only, valid only, revocations first and sticky, never our own.
Ingest(n, msg) ==
    /\ msg \in net
    /\ msg.from # n
    /\ LET revs == {r \in msg.revs : r[1] = msg.from /\ ValidRev(r)}
           allRevs == peerRevs[n] \cup revs
           links == {l \in msg.links : l[1] = msg.from /\ ValidLink(l) /\ l \notin allRevs}
       IN /\ peerRevs' = [peerRevs EXCEPT ![n] = allRevs]
          /\ peerLinks' = [peerLinks EXCEPT ![n] = (@ \ allRevs) \cup links]
    /\ UNCHANGED <<ownLinks, ownRevs, everLinked, net>>

\* roster_update: what node n sends its daemon.
Revoked(n) == peerRevs[n] \cup {<<n, d>> : d \in ownRevs[n]}
Update(n) ==
    ({<<n, d>> : d \in ownLinks[n]} \cup peerLinks[n]) \ Revoked(n)

Next ==
    \/ \E m \in Members, d \in Devices : Link(m, d) \/ Revoke(m, d)
    \/ \E m \in Members : Announce(m)
    \/ \E p \in Pairs : ForgeLink(p) \/ ForgeRev(p)
    \/ \E n \in Members, msg \in net : Ingest(n, msg)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ ownLinks \in [Members -> SUBSET Devices]
    /\ ownRevs \in [Members -> SUBSET Devices]
    /\ peerLinks \in [Members -> SUBSET Pairs]
    /\ peerRevs \in [Members -> SUBSET Pairs]

\* A stored peer link was really signed by the member it names.
NoForgedLink == \A n \in Members : \A l \in peerLinks[n] : ValidLink(l)

\* A stored peer revocation was really signed by its member.
NoForgedRevocation == \A n \in Members : \A r \in peerRevs[n] : ValidRev(r)

\* Nobody's links come back to them through others; the store holds other members only.
OnlyOthers == \A n \in Members : \A l \in peerLinks[n] \cup peerRevs[n] : l[1] # n

\* A device is never stored as linked once its revocation is known (either arrival order).
StoreNeverHoldsRevoked == \A n \in Members : peerLinks[n] \cap peerRevs[n] = {}

\* The daemon is never sent a device the node knows is revoked.
RevokedNeverSent == \A n \in Members : Update(n) \cap Revoked(n) = {}

\* Every link sent to the daemon belongs to its real owner (a device key is never sent under
\* another member, so the daemon's cross-member conflict rule is never needed for honest keys).
SentUnderOwner == \A n \in Members : \A l \in Update(n) : Owner[l[2]] = l[1]

=============================================================================
