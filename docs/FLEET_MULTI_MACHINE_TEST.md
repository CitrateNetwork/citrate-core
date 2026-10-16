---
created: 2026-10-01
branch: hup/n5-fleet-rest
author: Larry Klosowski + Claude Opus 5.5
status: ready for the DGX team and a person at each machine; not yet run across machines
updated: 2026-10-04 (hup/n7-cluster-mesh-prereqs, multi-group daemon + group links)
wp: HUP-S8.1, HUP-S8.2, HUP-S8.3 (US-8.1), S8.4 prep
companions:
  - docs/FLEET_WIZARD_RUNBOOK.md (what each wizard step does)
  - docs/CLUSTER_CROSSMACHINE_SOAK_RUNBOOK.md (env knobs, firewall, network prerequisites)
  - citrate-cluster scripts/soak/DEVICELINK_MULTI_MACHINE.md (the same proofs without the app, no sign-in)
supersedes: docs/CLUSTER_DEVICELINK_TWO_MACHINE_TEST.md for builds that include hup/n5-fleet-rest
---

# Fleet: the two- and three-machine test with the app

This is the US-8.1 run with real people at real machines: the wizard pairs the machines, the
DeviceLinks travel with the pairing (and between members over the group relay), each machine meshes
under its own key, and removing one evicts it. It closes the `g4-fleet` evidence apart from the CL-S4
sign-off.

If nobody can sign in on the Linux box, run the daemon-level version first (citrate-cluster
`scripts/soak/DEVICELINK_MULTI_MACHINE.md`): same proofs, test keys, no app.

## Builds (every machine)

* citrate-cluster `hup/n5-fleet-rest`: `cargo build --release -p cluster-daemon`.
* Citrate Core from `release/0.5.0-hermes-upskill` with `hup/n5-fleet-rest` merged (or that branch),
  packaged for the OS. Set `CITRATE_CLUSTER_BIN=<path to the cluster-daemon above>` if the package
  does not bundle it yet.
* Open TCP 4001 inbound (Linux: `sudo ufw allow 4001/tcp`; macOS: allow the firewall prompt).

## Environment (set before launching the app; the cross-machine mesh is off by default)

| Variable | Value |
|---|---|
| `CITRATE_CLUSTER_LISTEN` | `/ip4/0.0.0.0/tcp/4001` |
| `CITRATE_CLUSTER_GROUP` | the group id from step 1 (Groups shows it). With a daemon built from citrate-cluster `hup/n7-cluster-mesh-prereqs` or later, leave it unset to have one daemon serve every group (each group then listens on its own port near 4001; see below) |
| `CITRATE_CLUSTER_BOOTSTRAP` | machines after the first: `/ip4/<A ip>/tcp/4001/p2p/<A peerId>`, comma-separated for more |

To read a machine's PeerId once it is linked and has opened the Cluster screen:
`CITRATE_CLUSTER_SEED_FILE="<app data>/cluster/cluster.seed" cluster-daemon --print-identity`
(macOS app data: `~/Library/Application Support/ai.citrate.core`). The `address` printed must be the
machine's DEVICE address shown under "Your devices", not the member address.

## Test 1: two machines, one member (Mac A, Linux box B)

1. **A.** Launch with LISTEN and GROUP (create the group first if needed, then relaunch with its id).
   Cluster screen, "Connect my machines", Start, Pair. "Link this machine", approve the review card
   (the wallet signs a short text; no funds move). Select the group on the Cluster screen once (the
   daemon restarts under the device key; no app restart). Read A's PeerId.
2. **B.** Import the same wallet (same member) and be in the group on B too (the Cluster screen needs
   the group's roster from the relay; if the relay will not let a second machine of the same member
   in, record that and run the daemon-level version for the mesh part). Launch with LISTEN, GROUP and
   BOOTSTRAP pointing at A. Cluster screen, wizard, Start, Pair, "Link this machine", approve.
3. **A.** "Create pairing link". **B.** paste it, "Check link", "Pair with this machine".
   Expect on B: A listed as "linked under you". Expect on A within 3 s, without touching anything: B
   listed as "linked under you". No link code was copied by hand.
4. **Both.** Select the group. Expect: the peers list shows the other machine's device address
   `online`, labelled "<name> of <member>"; "Your devices" lists both; the two PeerIds differ.
5. **Revoke.** On A, "Remove" B, confirm. Select the group again. Expect: A no longer lists B; B's
   Cluster screen shows `0` online within about 10 s; leave it 2 minutes: B keeps re-dialing and
   never gets back in. Pasting B's old link code on A is refused.

Record: both PeerIds, pairing to "linked under you" time on A, Remove to drop time, and any
`[cluster] daemon refused device links:` lines in either app's log.

## Test 2: two machines, two members (A with wallet 1, C with wallet 2)

Proves other members' links travel over the group relay.

1. **A** creates the group and invites wallet 2 (Groups, invite). **C** joins.
2. Each machine links itself (wizard "Link this machine", or "Your devices").
3. Launch both with LISTEN, GROUP and (on C) BOOTSTRAP to A. On each, open the Cluster screen and
   select the group: this shares the machine's links with the group (a hidden message on the
   encrypted relay) and picks up the other member's. Do it once more on the machine that went first.
4. Expect: each machine lists the other member's device `online` under that member. Neither chat
   shows the hidden messages.
5. **C** removes its own device ("Your devices", Remove, confirm), then opens the Cluster screen
   (shares the revocation). **A** selects the group. Expect: A drops C's device within one roster
   update, and C's re-dials stay refused.

## Test 3: three machines (A and B for member 1, C for member 2)

Run Test 1 on A and B, then add C as in Test 2 with `CITRATE_CLUSTER_BOOTSTRAP` listing A and B.
Expect a full mesh: on each machine the other two devices are `online`, three distinct PeerIds.
Remove B on A, then select the group on A and on C. Expect B evicted from both, A and C still meshed.

## Expected limits (not failures)

* The mesh is off unless `CITRATE_CLUSTER_LISTEN` is set. It turns on by default only after the CL-S4
  transport sign-off and a daemon that serves every group (`cluster_mesh.rs`); the Cluster screen
  says which applies.
* With `CITRATE_CLUSTER_GROUP` set, the daemon serves only that group, as before. Unset (needs a
  daemon from citrate-cluster `hup/n7-cluster-mesh-prereqs` or later), one daemon serves every group,
  each on port `4001 + keccak(group) mod 1024`; open that range, or read the ports from a group link.
  Instead of `CITRATE_CLUSTER_BOOTSTRAP`, machines can find each other from a group link:
  `cluster_group_seed` on one machine, `cluster_add_seed` on the other (bridge
  `cluster.groupSeed` / `cluster.addGroupSeed`; no screen yet). `CITRATE_CLUSTER_MDNS=1` adds LAN
  discovery only with a daemon built with `--features mdns`; a default daemon refuses it.
* Members' links are picked up when the group is opened (Cluster screen or the chat). An app older
  than this build shows the hidden share message as text.
* A machine that still holds the wallet can link itself again after being removed; only a new wallet
  locks it out. The "Your devices" text says so.
