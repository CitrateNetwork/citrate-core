---
created: 2026-08-27T00:00:00Z
branch: cx/s3.<wp>-<slug> (one branch per WP; Lane C is serial — the keystone)
author: Larry Klosowski (@SaulBuilds) + Claude Opus 4.8
status: active
sprint: CX-S3
planset: Commons / citrate-core-social (citrate-federation/.agentile/planset/2026-08-26-citrate-core-social/)
tier: T1
lane: C (owns .agentile/cx-ownership.map lane s3 — comms.rs, group.rs, comms_tests.rs, seam.rs
       + bridge/*/comms.ts + slices/groups.ts + surfaces/Groups.tsx)
---

# Sprint CX-S3 — The Group object (Lane C, the keystone) · "Commons"

## Goal

Secure 1:1 + group comms with admin RBAC, server-blind (MLS), owned by the room/group admin —
manifesting the citrate-comms format inside citrate-core. Gate gB unblocks Lane E (cluster). Lane C
is serial (shares comms.rs/seam.rs). Every WP passes `scripts/cx-ownership-check.sh s3`.

## Locked decision (S3.1)

| # | Decision | Choice |
|---|---|---|
| D-C1 | Where the relay lives | **Local supervised `comms-relay` sidecar, owner = the signed-in wallet.** Reuses the node/mem-mcp supervised-sidecar seam (resolve binary → spawn with ENV config → admin `GET /health` → bounded-backoff restart); the relay is server-blind (ciphertext only, MLS E2E). Reaching OTHER members' relays is Lane E's P2P job. Rationale: no new src-tauri crate dep (spawned binary, declared in S0.5 overlays), key material stays on-device, and it needs no managed-state wiring in the s0-owned lib.rs (the manager is a process-wide singleton in comms.rs). |

## Baseline (Rule 2 — must not decrease)
- src-tauri lib tests: 300 (post CX-S2). CX-S3 adds tests per WP.

## Work packages (serial within the lane)
- [x] **S3.1** `comms-relay` sidecar lifecycle in `comms.rs`: `CommsRelayManager` (resolve binary →
      ENV-configured spawn (bind/admin/data/owner/master-key, never argv) → admin `/health`
      liveness → start/stop/status), mirroring serve.rs. Injected stub-binary tests (no real relay
      in CI). +6 tests. **M.** — lib 300 → 306. (`#![allow(dead_code)]` for one WP: the consumer is
      S3.2; removed there.)
- [ ] **S3.2** real comms commands over the member-client + SIWE via the existing wallet/ceremony
      (no fresh keyring); replace the `comms_connections` seam. Lazy-start singleton instantiates the
      S3.1 manager. **L.**
      - **OPEN QUESTION D-C2 (member-client transport, 2026-08-27) — needs a decision before build.**
        Confirmed by inspection: do NOT link `comms-client` (pulls `comms-core`/MLS + `comms-relay`/
        RocksDB+axum + `comms-session` + tokio + rustls — violates the SCOPE lean-tree gate). BUT the
        two obvious alternatives don't work either: `comms-wire` is NOT light (deps: `comms-core`,
        `comms-proto`, tokio, tokio-tungstenite, futures-util — still drags MLS + async in), and
        `comms-agent-bridge` is a **library, not a spawnable binary** (ipc.rs/socket.rs/lib.rs, no
        main.rs) so it can't be a supervised sidecar as-is. Viable paths, pick one:
        1. **New thin bridge binary in citrate-comms** — a small `[[bin]]` wrapping `comms-agent-bridge`
           that exposes a simple local socket (JSON-RPC-over-UDS, mem-mcp style); citrate-core spawns
           it + speaks a light IPC. Keeps src-tauri lean; needs a citrate-comms change (cross-repo).
        2. **Hand-rolled minimal WS client in src-tauri** — speak the relay wire protocol at the byte
           level with only the message types Commons needs. Light, but re-implements/mirrors the
           protocol (fragile; must track comms-wire — Rule 9 tension).
        3. **Extend `comms-relay`** to expose a loopback member IPC on its admin/socket that
           citrate-core speaks minimally.
        Recommendation: **path 1** (a comms-side thin bridge binary) — cleanest lean-tree boundary,
        reuses the sidecar seam already built in S3.1, and the socket IPC is small to test. This makes
        S3.2 a cross-repo WP (like S2.2 was for the chain). Common to all paths: SIWE via the
        SignatureCeremony (no fresh keyring), master key in the OS keyring (mem-mcp `Keyring`), owner
        from `wallet::address_auto_unlocked(custody)`.
- [ ] **S3.3** Group object: create/join, roster, roles, signed `RoleAssertion` grant/revoke,
      **atomic offboard** (drop allowed_peers + co-pin + cohort in one epoch). Source: `rbac.rs`,
      `comms-relay/lib.rs`. **TLA+: RosterRBAC.** → `group.rs`(new), `comms_tests.rs`. **L.**
- [ ] **S3.4** React conversation surface (1:1 + group) in `domain::ChatMessage` format; replace
      ping-only Comms. `bridge/*/comms.ts`, `slices/groups.ts`, `surfaces/Groups.tsx`. **XL.**
- [ ] **S3.5** admin panel (invite/assign/remove) — RBAC enforced at the relay, not the client.
      `surfaces/Groups.tsx` admin subview. **M.**

## S3.2 progress (2026-08-27) — D-C2 resolved: build a member daemon

Investigation confirmed the member-client can't link any comms crate (all pull MLS/relay/tokio —
lean-tree). Resolution: a NEW **`comms-member-daemon`** in citrate-comms that runs a wallet-owned MLS
member + an in-process server-blind relay and (next increment) exposes a loopback UDS JSON IPC;
citrate-core spawns it and speaks light JSON — no comms crate linked.

- **Increment 1 SHIPPED (for review): citrate-comms PR #50** — the daemon core, real OpenMLS, no
  stubs. `MemberDaemon`: new (MLS identity + wallet-owned relay + SIWE login + publish key package),
  create_group, list_groups, add_member (real commit + welcome + onboard), send (MLS-encrypt →
  relay), poll_messages (drain → MLS-decrypt). Round-trip test proves owner→add→member-join→encrypt
  →owner-decrypt (3 tests green). Generalized from `comms-client::backend`'s bootstrap.
- **Remaining for S3.2 (blocked on PR #50 review, then citrate-core wiring):**
  1. citrate-comms: the UDS JSON IPC server + `[[bin]]` (the transport), cross-process member join
     (fetch welcome/tree from relay), RBAC roster/assignRole/offboard (remove by leaf index).
  2. citrate-core (this repo): the S3.1 sidecar manager spawns the daemon bin; a thin UDS JSON
     client in comms.rs; wire `groups_*` over it; SIWE via the SignatureCeremony; swap the seam.
  Note: the daemon subsumes the S3.1 bare `comms-relay` sidecar for the local single-node case (the
  relay is in-process in the daemon). S3.1's CommsRelayManager may be repointed at the daemon bin or
  retired — decide when wiring citrate-core.

## Daily
- 2026-08-27 — S3.1: CommsRelayManager sidecar lifecycle + 6 stub tests green, lib 300→306, no new
  warnings, ownership s3 green. Locked D-C1 (local relay sidecar, owner = signed-in wallet).
- 2026-08-27 — S3.2: resolved D-C2 (member-daemon approach). Built comms-member-daemon core in
  citrate-comms (PR #50) — real MLS round-trip, 3 tests green. citrate-core wiring pending PR #50.
