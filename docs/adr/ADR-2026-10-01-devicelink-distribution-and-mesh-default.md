---
created: 2026-10-01
branch: hup/n5-fleet-rest
author: Larry Klosowski + Claude Opus 5.5
status: proposed (the channel and the placeholders below are pending owner sign-off; defaults change nothing for members)
wp: HUP-S8.1 follow-on, HUP-S8.2, HUP-S8.4 prep
builds_on: docs/adr/ADR-2026-10-01-device-keys-and-devicelink.md
---

# How DeviceLinks reach other machines, and how the mesh turns on by default

## Context

ADR-2026-10-01 (device keys) gave every machine its own mesh key, admitted through a three-signature
DeviceLink. Two gaps were left open:

1. A machine only knew links pasted into it by hand, and only of its own member. Other members'
   nodes refused a linked machine (an open Medium from the HUP-S8.1 review, to close before S8.4).
2. A new link took effect after an app restart, and a peer refused before it was authorized was
   never dialed again.

US-8.2 also asks for the mesh to be on by default once that is safe.

## Decision

1. **Own machines: the pairing carries the link.** When a machine is linked, its link code rides in
   the fleet pairing exchange, both directions. The receiver verifies all three signatures and
   stores it only when it names the same member; another person's machine is reported, not added.
2. **Other members: the group relay carries the links.** Each member shares its own links and
   revocations with every group it is in, as a control message on the end-to-end encrypted relay
   (prefix U+0001 `cdlink1:`, hidden from the chat like the social-binding message). Core accepts a
   share only when the relay attributes it to the member it speaks for and every signature
   verifies, keeps revocations for good (in either order of arrival), caps the store (64 links per
   member, 1,024 links, 2,048 revocations), and sends the daemon this machine's own links first,
   then other members' links for members on that group's roster, never a revoked one, within the
   daemon's 64-per-update caps and its 64 KiB line. A member who never linked a device sends nothing.
3. **No restart.** Core's cluster manager can be replaced: when this machine's mesh identity
   changes (it was linked, or removed), the daemon is stopped and the next cluster call starts it
   under the new identity. The daemon re-dials its bootstrap peers that are not connected every
   5 s, so a peer authorized later gets in without a restart on either side.
4. **Mesh on by default: built, off.** `cluster_mesh.rs` decides the transport: the operator's
   `CITRATE_CLUSTER_LISTEN` wins; otherwise the mesh is on by default only when
   `TRANSPORT_SIGNED_OFF` (the CL-S4 sign-off) and `MULTI_GROUP_DAEMON` (one daemon serving every
   group) are both true, and the member has not turned it off. Both are `false` in this build, so
   the answer is today's: operator setting or off. The Cluster screen tells the member which.

## Why the relay and not the mesh

The mesh admits a device only after it knows the device's link, so it cannot carry the first link
between two members. The relay already connects exactly the group's members, is end-to-end
encrypted and server-blind, and already carries signed control messages. Links are self-verifying,
so the channel needs no trust.

## Consequences

* Links are picked up when the group is opened (Cluster screen or chat), not in the background.
* Apps older than this build show the share message as text in the chat.
* Removal is not a lock-out for a machine that still holds the wallet: it can mint a new device key
  and link itself again. Only a new wallet locks it out. The member-facing text says so.

## Pending owner sign-off (placeholders)

* The relay as the distribution channel, and the store caps above.
* The re-dial interval (5 s), per-tick cap (16) and kept addresses (64).
* The default listen address `/ip4/0.0.0.0/tcp/4211` once the mesh is on by default.
* Whether a member may turn the default-on mesh off (built: yes, after the sign-off only).
