---
created: 2026-10-01
branch: hup/n4-devicelink
author: Larry Klosowski + Claude Opus 5.5
status: ready for the DGX team (two hosts needed); single-machine equivalent passes in CI
wp: HUP-S8.1
companions:
  - docs/adr/ADR-2026-10-01-device-keys-and-devicelink.md
  - docs/CLUSTER_CROSSMACHINE_SOAK_RUNBOOK.md (env knobs and network prerequisites)
---

# DeviceLink: the two-machine test

What it proves: two machines of ONE member mesh over the real transport under two distinct
PeerIds, both are listed under the member, and revoking one evicts it while the other stays.

The single-machine version (three real daemon processes on loopback, real keys) is
`citrate-cluster/crates/cluster-daemon/tests/devicelink_multiprocess.rs` and passes locally. This
run adds separate hosts, a real network path, and the packaged Citrate Core apps.

## Builds

* citrate-cluster branch `hup/n4-devicelink`: `cargo build --release -p cluster-daemon`.
* citrate-core: the integration branch with `hup/n4-devicelink` merged, packaged per OS, with
  `CITRATE_CLUSTER_BIN` pointing at the cluster-daemon above if it is not bundled yet.

## Setup (both machines)

1. Import the SAME wallet on both machines (one member: the comms identity is derived from it).
2. Be in one group whose roster lists that member (create the group on machine A).
3. Network path between the hosts (same LAN or a tailnet; see the soak runbook).
4. Env before launch: `CITRATE_CLUSTER_LISTEN=/ip4/0.0.0.0/tcp/4001` and
   `CITRATE_CLUSTER_GROUP=<group id>`. Machine B also gets `CITRATE_CLUSTER_BOOTSTRAP` (step 4).

## Steps

1. **Machine A:** Cluster screen, "Your devices", name it (for example "Mac"), "Link this device",
   approve the wallet signature at the review card. Then "Copy link code".
2. **Machine B:** same, name it "Linux box", link, approve, copy its code.
3. Paste each machine's code into the other's "Add device" field. Both lists now show two devices.
   Quit and relaunch both apps (the mesh identity is chosen when the cluster daemon starts).
4. **Machine A:** open the group's Cluster screen, then read its PeerId:
   `CITRATE_CLUSTER_SEED_FILE=<app data>/cluster/cluster.seed cluster-daemon --print-identity`.
   The `address` must equal A's device address shown under "Your devices" (not the member address).
   Set `CITRATE_CLUSTER_BOOTSTRAP=/ip4/<A ip>/tcp/4001/p2p/<A peerId>` on B and relaunch B.
5. **Check on both machines:** the peers list shows the other machine's device address `online`,
   labelled "<name> of <member>"; the two `--print-identity` PeerIds differ.
6. **Revoke:** on machine A, "Remove" the Linux box and confirm. Within one roster update (open the
   Cluster screen again) A no longer lists it, and B shows `0` peers online. Paste B's old code into
   A again: it is refused (revoked for good).
7. **Record:** both PeerIds, the time from Remove to B dropping, and any refused-link reasons the
   app logged (`[cluster] daemon refused device links: ...`).

## Expected limits (not failures)

* Links of OTHER members' devices are not shared automatically yet (follow-on through the comms
  roster). This test uses one member on purpose.
* The transport stays operator-only until the CL-S4 sign-off (HUP-S8.4).
