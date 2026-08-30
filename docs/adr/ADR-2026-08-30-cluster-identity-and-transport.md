---
created: 2026-08-30
branch: feat/cluster-crossmachine-groundwork
author: Claude (Opus 4.8), directed by @SaulBuilds
status: accepted (Stage-1) — owner-ratified 2026-08-30
supersedes: none
amends: citrate-cluster planset decision CL-2 (pending cluster-team ratification)
companions:
  - docs/CLUSTER_CROSSMACHINE_SOAK_RUNBOOK.md (the two-machine soak this gates on)
  - docs/adr/ADR-2026-08-28-cluster-daemon-extraction.md (the daemon this configures)
  - ../citrate-cluster/.agentile/planset/00_OVERVIEW.md (CL-2, CL-3 — the decisions this reconciles)
---

# ADR — Cluster identity is the comms identity; libp2p transport is soak-gated

## Context

A Group's cluster is a private P2P mesh among its members. citrate-core runs the standalone
`cluster-daemon` as a supervised sidecar and feeds it the group **roster** (from the comms
member-daemon); the daemon reconciles the mesh and enforces admission (`address ∈ role-gated allowed
set`). Today it runs the **in-process** transport (single node, real admission, no cross-machine
fan-out). Bringing partners onto a cluster means enabling the real **libp2p** transport (CL-S3).

Wiring that transport surfaced a contradiction between two locked decisions made a day apart:

- **citrate-cluster CL-2** (2026-08-28): *"A member's libp2p Noise id is bound to the same secp256k1
  key as its comms identity = its wallet address. No fresh keyring: `WalletAddress = comms id =
  cluster Noise id`."*
- **citrate-core comms Option A** (2026-08-27): the comms member-daemon uses a **fresh, scoped,
  non-value secp256k1 device key** sealed in the OS keyring — *explicitly not the custody wallet*.
  The relay keys the group roster on **that comms address**. `comms address ≠ wallet address`.

So CL-2's premise (`wallet = comms = cluster`) is false in the shipped app. Meanwhile `cluster.rs`
was setting `CITRATE_CLUSTER_SELF_ADDR` to the **wallet** address, and the libp2p transport derives
each peer's address from **its Noise/seed key** and matches it against the roster. In-process mode
hides the mismatch (no on-wire admission). The moment `CITRATE_CLUSTER_LISTEN` is set, every peer —
including this node itself — fails `address ∈ roster`, because the roster holds **comms** addresses
while the node announces/derives a **wallet** address. Cross-machine mesh would show everyone offline.

## Decision

1. **Cluster identity = comms identity.** This node's cluster `self_addr` is its **comms** address,
   and (for libp2p) its Noise seed is the **comms** secp256k1 key — the same device-sealed key the
   comms member-daemon uses. This makes the address the node announces on the wire match the roster,
   and it keeps citrate-core Rule 3 intact: the daemon holds a **non-value per-device key**, never the
   custody wallet. This **amends CL-2** to drop the stale `= wallet address` clause: the operative
   identity is `comms id = cluster Noise id`. (The comms↔wallet on-chain binding is the deferred
   `wallet_link` attestation; nothing consumes it yet.) This amendment is owned by the citrate-cluster
   planset — routed to that team to ratify in their decision table; citrate-core implements the
   consistent behavior in the meantime.

2. **One source of truth for the device identity (Rule 9).** `comms::device_identity()` provisions the
   comms seed and derives its address (`address_from_secret_hex`, the standard
   `keccak256(uncompressed_pubkey[1..])[12..]`, verified against the privkey=1 EVM vector). Both the
   comms daemon and the cluster daemon consume this — cluster does **not** re-read the keyring.

3. **The libp2p transport is OFF by default and env-gated (not a UI toggle).** citrate-core enables
   the real cross-machine mesh only when its own environment carries `CITRATE_CLUSTER_LISTEN`; it then
   also writes the comms seed to a 0600 file and forwards `CITRATE_CLUSTER_SEED_FILE` +
   optional `CITRATE_CLUSTER_BOOTSTRAP`. No user-facing control can turn it on. It stays operator-only
   until **(a)** the two-machine soak passes through packaged builds and **(b)** the Rule-8 transport
   security sign-off (citrate-cluster Rule 8) is on file. It must not be flipped on for real partner
   traffic pre-audit.

4. **Single-group scope for now (honest limit).** The CL-S1 libp2p transport is one-group-per-daemon
   (a per-group swarm reading the same listen addr). citrate-core runs one daemon for all of a
   member's groups, so cross-machine soak uses an ephemeral `/tcp/0` listen or one group per run.
   Multi-group cross-machine fan-out from the single daemon is a documented follow-on, not claimed as
   working.

## Consequences

- **Correctness:** admission now matches the roster in both transports; the wallet/comms mismatch that
  would have failed every libp2p connection is removed.
- **Security (Rule 3 / Rule 8):** the daemon never holds the wallet key; the Noise seed is the
  non-value comms key, crossed only as a 0600 file path (never argv/env). The transport itself remains
  behind the Rule-8 sign-off before any cross-org trust.
- **Reversibility:** with no `CITRATE_CLUSTER_LISTEN` set, behavior is byte-for-byte the prior
  in-process default. Nothing ships "on."
- **Cross-repo:** CL-2 in the citrate-cluster planset should be updated to reflect the comms-id
  binding; until then this ADR is the reconciling record.

## Status

Stage-1, owner-ratified 2026-08-30. Wired in `src-tauri/src/cluster.rs` + `comms.rs`; tests cover the
env plumbing (seed-as-file-path, absent-by-default) and the address vector. Cross-machine "on" is
pending the soak (runbook companion) and the Rule-8 transport sign-off.
