---
created: 2026-08-30
branch: feat/cluster-crossmachine-groundwork
author: Claude (Opus 4.8), directed by @SaulBuilds
status: draft — operator runbook (soak-gated, pre Rule-8 transport sign-off)
companions:
  - docs/adr/ADR-2026-08-30-cluster-identity-and-transport.md
  - ../citrate-cluster/scripts/soak (soakctl.py, soak-node.sh)
---

# Cluster cross-machine soak runbook (CL-S3)

Bring a second machine onto a Group's cluster over the real libp2p mesh, to soak it before partners.
This transport is **OFF by default** and **operator-only** — it does not turn on for normal users, and
it must not carry real partner traffic until this soak passes on packaged builds **and** the Rule-8
transport security sign-off (citrate-cluster Rule 8) is on file.

## What has to be true first

1. **Both machines are members of the SAME Group.** Admission is `address ∈ roster`; the roster comes
   from the comms member-daemon. Create the group on machine A, invite machine B by @handle (D4
   invite), have B join. Confirm both appear in the group roster before touching the mesh.
2. **Network reachability.** libp2p dials a real address. Same LAN, a VPN, or public IPs all work;
   the simplest reliable path is a **Tailnet** (each machine has a stable 100.x address). NAT with no
   relay will not connect — hole-punching/relay is a follow-on.
3. **Identity = comms key.** Nothing to configure: `self_addr` and the Noise seed are the device-sealed
   comms identity automatically (see the ADR). The seed is written to `<app_data>/cluster/cluster.seed`
   (0600) only when libp2p is enabled, and removed on teardown.

## Env knobs (set in citrate-core's OWN environment before launch)

| Var | Machine A (seed/bootstrap host) | Machine B (partner) |
|---|---|---|
| `CITRATE_CLUSTER_LISTEN` | `/ip4/0.0.0.0/tcp/4001` (fixed port) | `/ip4/0.0.0.0/tcp/4001` |
| `CITRATE_CLUSTER_BOOTSTRAP` | *(unset)* | `/ip4/<A-ip>/tcp/4001/p2p/<A-peerId>` |

Setting `CITRATE_CLUSTER_LISTEN` is what selects the libp2p transport. Use a **fixed** port (not
`/tcp/0`) so B has a stable address to dial. One group per daemon for the soak (single-group scope —
see the ADR).

## Steps

1. **Machine A — start with LISTEN set**, open the group's Cluster surface once (this lazily starts the
   cluster-daemon and writes `cluster.seed`).
2. **Machine A — get its peerId** (offline, no swarm needed):
   ```
   cluster-daemon --print-identity     # reads CITRATE_CLUSTER_SEED_FILE
   # → {"address":"<comms-addr>","peerId":"12D3KooW…"}
   ```
   The `cluster-daemon` binary lives next to the app executable (`…/Contents/MacOS/cluster-daemon`);
   point `CITRATE_CLUSTER_SEED_FILE` at `<app_data>/cluster/cluster.seed`. Confirm `address` equals
   A's entry in the group roster (the identity check).
3. **Build B's bootstrap addr**: `/ip4/<A-tailnet-ip>/tcp/4001/p2p/<A-peerId>`. Set it as
   `CITRATE_CLUSTER_BOOTSTRAP` on machine B.
4. **Machine B — start with LISTEN + BOOTSTRAP set**, open the same group's Cluster surface. B dials A
   on startup.
5. **Verify the mesh from both sides.** In each app's Cluster surface, the other member flips from
   `authorized · offline` to `online`. `cluster_status` should report `online: 2, total: 2`. Share a
   file from A (`Groups → cluster → share`) and confirm the CID appears in B's shared set.

## Soak

- Run the two-machine mesh for an extended window under the citrate-cluster soak harness
  (`../citrate-cluster/scripts/soak/soakctl.py`), watching for: dropped/re-dialed peers, admission
  drift (an offboarded member must evict in the same step — the CL-3 invariant), and seed-file hygiene
  (present only while running, 0600, gone after quit).
- Record the honest ceiling that actually holds (CL-S3 "ship the ceiling that holds", RT-3). Do not
  claim a peer count the soak did not sustain.

## Exit criteria (before partners)

- [ ] Two packaged builds mesh over the real transport; `online` reflects true connectivity.
- [ ] Offboard evicts the peer from the mesh in the same step (admission invariant holds on the wire).
- [ ] Seed file is 0600, written only in libp2p mode, removed on teardown.
- [ ] Rule-8 transport security sign-off on file (citrate-cluster Rule 8) — the gate for cross-org trust.
- [ ] Honest ceiling recorded; multi-group fan-out limitation noted, not hidden.
