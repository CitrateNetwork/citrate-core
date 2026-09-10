---
created: 2026-07-13
branch: docs/c1-scope
author: Claude Fable 5, directed by @SaulBuilds
status: scoped — dispatching C1.0
sprint: CORE-C1 (SidecarSupervisor + node) — Phase C, maps to CORE-S3 + S0.4
rule8: C1.1 (encrypted data dir + keyring key) and C1.2 (node-agent bearer + signature-request bridge) are @rule8; C1.0 is security-sensitive (process spawning)
planset: citrate-federation/.agentile/planset/2026-07-12-core-beta-wiring/00_STATE_AND_PLAN.md (Phase C)
grounded_in:
  - src-tauri/src/* — NO supervisor scaffold exists yet (new)
  - src/bridge/domains.ts NodeDomain { status(), start(), stop() } — sim seam ready to wire
  - src/surfaces/Node.tsx — Operations/Earning/Pinning tabs, sim; refs node-agent 127.0.0.1:19600 bearer
  - citrate-chain/node (the citrate-node binary); citrate-node-agent (federation repo, manifest:342)
depends_on:
  - Phase B (custody + ceremony) — COMPLETE. C1.2 routes node-agent signature-requests through the hardened ceremony.
---

# CORE-C1 — SidecarSupervisor + node operations (Phase C foundation)

## What Phase C is, and where C1 sits
Phase C brings the native process spine to life: citrate-core spawns and supervises the
real sidecars (citrate-node, node-agent, and later mcp_serve / llama-server). C1 is the
foundation: a **SidecarSupervisor** and the **citrate-node** wiring so Node Operations go
live. Storage/memory sidecars (mcp) = C3; earnings = C2.

## The bridge seam this fills
`NodeDomain { status(): {state,peers,height,syncPct}; start(); stop() }` is a sim seam
today. C1 wires its real tauri adapter to the supervisor + a live citrate-node.

## Sub-work-packages
### C1.0 — SidecarSupervisor core (DISPATCH FIRST; self-contained, no external binary)
The reusable process-supervision primitive (this is the deferred CORE-S0.4 deliverable).
- Spawn a child from a typed `SidecarSpec { bin, args, env, workdir, health_check }` — NO
  shell interpolation (spawn the binary directly; no `sh -c`; args as a vec) — command-
  injection-proof by construction.
- Monitor liveness; on crash, **restart with bounded exponential backoff** (cap + max-
  retries → give-up state, so a crash-looping child can't fork-bomb) and write a **crash
  record** (timestamp, exit status, stderr tail) to a file.
- Graceful **stop** (SIGTERM→timeout→SIGKILL) with clean teardown; supervised children are
  killed on app exit (no orphans).
- A supervision status API (state: Off/Starting/Running/Backoff/Failed; restarts; last crash).
- **Acceptance (Rule 11):** kill a spawned dummy child → a crash record file exists AND a
  restart is observed (supervisor test suite); backoff caps (a child that exits instantly N
  times → Failed, bounded attempts, no tight loop); stop → child gone, no orphan (pid check).
- Security tests: a `SidecarSpec` with shell metacharacters in args does NOT execute a shell;
  no arbitrary-path spawn beyond the configured allowlist/bundled dir.

### C1.1 — citrate-node sidecar @rule8 (after C1.0)
- WP-0: locate/bundle the `citrate-node` binary (citrate-chain/node) as a Tauri sidecar (or a
  resolved path); Rule-12 drift if pinned. Decide bundling approach + document.
- Encrypted data dir: the node's data dir encrypted with a **keyring master key** (reuse the
  A2 custody keyring pattern — the data-dir key lives in the OS keyring, NOT on disk in clear).
- provision / start / stop / pause; wire `NodeDomain.status` to the node's real sync state
  (height, peers, syncPct) via its status/RPC; `start`→spawn, `stop`→"supervisor released".
- **Acceptance:** live testnet sync 0→head on a clean data dir (or an honest bounded-time
  sync-progress proof); raw-disk grep of the data dir proves ciphertext (ENCRYPT pattern).

### C1.2 — node-agent sidecar + signature-request bridge @rule8 (after C1.1)
- Spawn/supervise node-agent (citrate-node-agent); the supervision API at **127.0.0.1:19600**
  with a **bearer token** (generated per-session, 0600, never logged).
- The unsigned **SignatureRequest** seam: node-agent emits UNSIGNED requests → they route
  through the **SignatureCeremony** (Phase B). node-agent NEVER signs directly (this is the
  ADV-7 property we built and hardened — verify it end-to-end here).
- **Acceptance:** supervision API round-trip (127.0.0.1:19600); an unsigned request → a
  ceremony approval → a signed broadcast (reusing B1.4), proven end-to-end; node-agent cannot
  obtain a signature without the ceremony.

## Locked / open decisions (flag for owner as they arise)
- **D-C1-1 (open):** citrate-node binary sourcing — bundle-as-Tauri-sidecar vs
  build-from-source-pinned vs resolve-installed. Recommend bundled sidecar (self-contained
  install); confirm at C1.1.
- **D-C1-2 (open):** the C1.1 sync proof — full 0→head live testnet sync (heavy, minutes) vs
  a bounded sync-progress proof. Recommend bounded-progress for CI + a documented full-sync
  run. Decide at C1.1.
- Encrypted-data-dir key: reuse the A2 vault/keyring master pattern (locked — consistency).

## Definition of done (C1)
- SidecarSupervisor spawns/monitors/restarts (bounded backoff) with crash records + clean
  teardown, injection-proof; citrate-node runs under it with an encrypted data dir and Node
  Operations (status/start/stop) go live against a real node; node-agent runs under it and
  its signature requests route ONLY through the ceremony.
- Gates green (cargo/vitest up, clippy -D, audit 0 lean); build-and-stop → independent review
  (C1.1/C1.2 @rule8). Owner merges cleared code (authorized).
- Honest gaps stated (real node sync time; any sidecar not yet bundled).

## Out of scope (later in Phase C)
- C2 earnings (node-agent sweep + ContributionAccounting claim through ceremony).
- C3 storage/memory (mcp_serve/mcp_connect bundle, recall/search, chain-facts tenant / E-4).
- Pinning proofs (E-3 sealer). Node soak (S3.5).

## Dispatch
C1.0 first (build-and-stop → independent review: process-spawn security + supervision
correctness). Then C1.1 (@rule8), then C1.2 (@rule8).
