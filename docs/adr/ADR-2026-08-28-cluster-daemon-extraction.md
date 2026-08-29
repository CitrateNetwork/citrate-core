---
created: 2026-08-28T00:00:00Z
branch: feat/cl-s2-cluster-wiring
author: Saul + Claude Opus 4.8
status: accepted
---

# ADR-2026-08-28 — Extract the cluster admission engine to citrate-cluster; citrate-core becomes a thin daemon client

## Context

CX-S4 (Lane E) built the group P2P cluster's admission logic **in** citrate-core's
`src-tauri/src/cluster.rs`: `allowed_peers`, the `ClusterMembership` state machine (admit / leave /
reconcile-evict, invariant `admitted ⊆ allowed`), the `ClusterTransport` seam, and `ClusterSession`
(S4.1–S4.3). All of it was `#[allow(dead_code)]` — a primitive proven by 16 tests but with no live
consumer, because its real consumer is a **libp2p transport**, which is too heavy to link into
citrate-core's lean tree (the same constraint that put comms's MLS in a sidecar).

The owner's decision (2026-08-28) gave the cluster its own composable **Tier-1 repo**,
`citrate-cluster`: a pure `cluster-core` crate (the admission engine, ported verbatim + expanded) +
a `cluster-daemon` (libp2p Noise + gossipsub over `cluster-core`, serving a UDS JSON IPC).

## Decision

**The admission engine's canonical home is `citrate-cluster/cluster-core`, not citrate-core.**
citrate-core's `cluster.rs` becomes a **thin daemon client** — a `ClusterDaemonManager` (supervised
sidecar) + a UDS JSON client + the frozen `ClusterDomain` commands routed over the socket — exactly
mirroring `comms.rs`'s relationship to the comms member-daemon. citrate-core links **no** cluster
crate; it spawns a binary and speaks JSON.

Consequently the S4.1–S4.3 admission code (`allowed_peers`, `admit`, `ClusterMembership`,
`ClusterTransport`, `ClusterSession`) and its 16 tests are **removed from citrate-core** and live in
`citrate-cluster/cluster-core` (ported there, plus the daemon's own tests).

## Rule 2 (test-count monotone) note

This removes 16 tests from citrate-core. That is intentional and does **not** lose coverage: the
identical logic + tests moved to `cluster-core`, and net federation coverage **rose** (cluster-core
has the 16 admission tests, the daemon adds ~14 more for the UDS/IPC/co-pin/mesh, and the libp2p
transport adds its own). citrate-core gains 6 new daemon-client tests in their place. Rule 2's intent
— never silently lose verification — is satisfied: the coverage has a better home and grew. This ADR
is the record required to reduce a repo's local count.

## Consequences

- One source of truth for the RBAC→network boundary (`cluster-core`), reused by the daemon and any
  future lean client. No duplicated admission policy drifting between repos.
- citrate-core's cluster is daemon-backed (CL-S2): real membership + co-pinned shared files, fed the
  roster from the comms daemon. Single-node shows real admission with no fan-out until peers connect.
- The formal `ClusterAdmission.tla` moves with the code (it now lives in `citrate-cluster/formal/`);
  the citrate-core copy is removed.
- The cluster-daemon binary is bundled like the comms daemon (`build-cluster-daemon.sh` +
  `binaries/cluster-daemon` externalBin).
