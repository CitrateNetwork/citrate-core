---
created: 2026-10-01
branch: hup/n5-fleet-rest
author: Larry Klosowski + Claude Opus 5.5
status: evidence for review (pushed branches, no PR yet)
sprint: HUP-S8 Fleet (fan-out 5, lane P-fleet)
wps: S8.1, S8.2, S8.3, S8.4 prep, S8.5 (US-8.3 device list)
repos: citrate-core hup/n5-fleet-rest, citrate-cluster hup/n5-fleet-rest
---

# HUP-S8 fleet: what fan-out 5 closed, and what is left

Base: citrate-core `release/0.5.0-hermes-upskill` @ 00e6353, citrate-cluster `main` @ 35a4caf.
Decisions and placeholders: [ADR-2026-10-01-devicelink-distribution-and-mesh-default](../../../../docs/adr/ADR-2026-10-01-devicelink-distribution-and-mesh-default.md).

## Closed on these branches

| Gap (from #158, #159, cluster #9) | Now |
|---|---|
| Other members' links not distributed (review Medium, "before S8.4") | Shared over the group relay as a hidden control message; core verifies sender and signatures, keeps revocations for good, caps the store, and sends the daemon own links first then roster members' links within its caps and 64 KiB line (`device_link_share.rs`) |
| A new link takes effect after an app restart | Core's cluster manager is replaceable; linking or removing this machine restarts the daemon on the next cluster call (`cluster::reload_mesh_identity`). The daemon re-dials bootstrap peers, so a late authorization needs no restart on either side (cluster) |
| Wallet-signed DeviceLink not issued from a pairing (#159) | Link codes ride in the pairing exchange both ways; same-member links are verified and stored, another person's machine is reported only. The wizard offers "Link this machine" on the pair step |
| `citrate://pair` deep link not routed (#159) | Routed to the Cluster wizard, filled in, never auto-joined; before this it fell into the group-invite parser |
| Install link for machines without Core (#159) | Pair step shows `https://citrate.ai/download` with a QR |
| Issuer's list does not refresh after the other machine joins (#159 review) | The issuing machine re-reads its roster every 3 s while a link is open, and shows paired machines on the pair step |
| Revocation cap had no core-side match (cluster #9 Low) | Core sends at most 64 links and 64 revocations, newest revocations first, trimmed to the IPC line; a test pins the worst case (64 links with the longest labels, 64 revocations, a 700-member roster) under 64 KiB |
| "Remove one without the others" overstated (review) | "Your devices" now says removal is not a lock-out for a machine that still holds the wallet |
| US-8.3 AC1 device list as an MCP tool | `cluster_devices` read tool on the node MCP server (read-only annotations) |
| US-8.2 mesh on by default | Machinery built and off: `cluster_mesh.rs` keeps today's answer (operator env or off) until `TRANSPORT_SIGNED_OFF` and `MULTI_GROUP_DAEMON` are true; the Cluster screen says which |
| CL-S4 has no sign-off record (S8.4 proof) | citrate-cluster `.agentile/planset/CL-S4_SIGNOFF_PACKET.md`: claims, evidence, limits, steps, blank sign-off lines |
| Two- and three-machine scripts | App level: `docs/FLEET_MULTI_MACHINE_TEST.md`. Daemon level (no sign-in, test keys): citrate-cluster `scripts/soak/DEVICELINK_MULTI_MACHINE.md`, `devicelink-node.sh`, `examples/devicelink_fixture.rs`; both flows dry-run green on one Mac over loopback |

## Proof

* **Formal.** `src-tauri/formal/DeviceLinkShare.tla` with `MCDeviceLinkShare` (three members, one an
  adversary that forges, relays and replays; three devices; out-of-order, repeated delivery):
  1,572,480 distinct states, no error, 7 invariants. `DeviceLinkShare_mutants.py`: 8 of 8 mutants
  killed. Dropping only the update's revoked filter survives on its own (the store invariant already
  holds), so M06 removes it together with the store check; the Rust filter also covers own-store
  revocations and is unit-tested.
* **Multi-process, one machine (cluster).** `fleet_multiprocess.rs`: another member's device admitted
  when its link arrives late, without a restart (killed by stretching the re-dial to an hour); three
  devices of two members, revocation evicts from both nodes, re-dials refused.
  `ladder_multiprocess.rs`: full mesh and one co-pin to every node at 4 (in the suite), 16, 32 and 50
  processes.
* **Rust mutation checks (core).** Listed in the PR body with their results.

## Not done (and why)

* Two- and three-machine runs on separate hosts, with people at the machines (DGX team; steps posted
  on the sprint issue).
* CL-S4 signing, the separate-machine ladder (CL-S3), and the mesh-on-by-default flip: owner and
  security lead.
* Peer discovery and a multi-group daemon (preconditions for the default-on mesh): not built.
* Members' links are picked up when the group is opened (Cluster screen or chat), not in the
  background.
* Linux and Windows hardware runs of the wizard (Tailscale detection on Windows, firewall prompts).

## Fan-out 6 review (2026-10-04)

The branch above was committed unreviewed when fan-out 5 stopped. Fan-out 6 read the full diff
against `hup/m2-core`, merged `hup/m2-core` in (no conflicts, no rebase), reran the gates, and broke
the key guards to confirm the tests bite.

| Finding | Severity | Fix |
|---|---|---|
| The per-member link cap in `device_link_share::ingest` was disabled (`if false || ...`, its count unused), so one member could fill the shared 1,024-link store over several messages (the old flood test passed only because one oversized message is refused whole) | Medium | Per-member cap of 64 links enforced; new test sends 90 links in three messages and expects exactly 64 kept |
| No per-member revocation cap: one member's own valid revocations could fill the 2,048 shared slots, after which another member's real revocation was refused | Medium | 128 revocations per member (placeholder, pending owner sign-off); test proves another member's revocation is still recorded |
| `roster_update` sorted all revocations newest first and cut at 64; `revoked_at` is chosen by the signer, so one member with late timestamps (or many links, by address order) could push another member's revocation or link out of the daemon update | Medium | Own items first, then roster members in turn (each member's newest revocation first); test with a noisy member at the cap |
| A device revoked twice took two update slots (dedup only removed neighbours) | Low | One slot per device; test |
| `ensure_started` read the manager, released the slot lock, then restarted it if stopped; a mesh-identity reload in between could restart the old daemon while the next call built a second one (two daemons, bearer overwritten) | Low | Restart runs under the slot lock, only on the value still in the slot (`Slot::get_or_try_insert_ensured`); a threaded test is red with the old pattern |
| The new node MCP `cluster_devices` read started the group daemon as a side effect | Low | Answers "not running" instead, like the other reads after the node MCP review; `cluster::is_daemon_running` |
| citrate-cluster: the re-dial list stopped growing at 64 entries, so a peer dialed late in a long session was never re-dialed | Low | A full list drops its oldest entry (`remember_bootstrap`); red-green test |
| WIP was not formatted and failed clippy (`assertions_on_constants`, unused variable) | Gate | `cargo fmt`, tripwire rewritten as `assert_eq!` |

Mutation checks on the share and pairing guards (sender check for links and revocations, revocation
signature, link verification, revoked filter at ingest and in the update, roster-member filter,
other-member pairing code): results in the sprint issue comment.

Stacking note: `hup/n5-rt-core` (pairing reply proof, revoke prepare step, node MCP read guard) and
citrate-cluster `hup/n5-rt-other` (comms identity after revocation) touch the same files. Expect
conflicts in `fleet.rs`, `cluster.rs`, `device_link.rs`, `Cluster.tsx` and the cluster slice; both
sides' changes are needed.
