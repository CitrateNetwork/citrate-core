---
created: 2026-07-14
branch: docs/c3-scope
author: Claude Fable 5, directed by @SaulBuilds
status: scoped — dispatch after C2 merges
sprint: CORE-C3 (storage / memory) — Phase C, maps to CORE-S4.1-4.3
rule8: per-user encrypted store key (keyring) — @rule8
grounded_in:
  - citrate-memories/crates/mem-mcp: `mcp_serve` daemon over a Unix socket (memdag.sock);
    tools memory.recall/search/neighbors (read); RocksDB LOCK before socket (singleton);
    encrypted store `.bge.memdag` is the ONLY on-disk copy; needs rocksdb+transformer (bge_base) features
  - citrate-memories/crates/mem-ingest: E-4 chain-facts (40204.json contract catalog → memory graph) — MERGED (memories #9)
  - src/surfaces/Storage.tsx: 2.5D constellation served by the local MCP socket (s.socketPath), tenants personal + chain-facts
  - src/bridge/domains.ts MemoryDomain { assert, recall } — extend with search/neighbors
depends_on:
  - C1.0/C1.0b supervisor (spawn the daemon under it), C1.1 keyring pattern (store key). B1/C1/C2 merged.
---

# CORE-C3 — storage / memory (mem-mcp sidecar + recall/search + chain-facts) — closes Phase C

## What C3 is
Bundle the mem-mcp memory daemon as a supervised sidecar, hold its encrypted-store key in the
OS keyring, wire `MemoryDomain` recall/search/neighbors to its local Unix socket so the Storage
constellation shows a real graph, and populate the chain-facts tenant via the merged E-4
ingestor. This is the last Phase-C piece.

## Work packages
### C3-WP0 — bundle + supervise mem-mcp (Tauri sidecar, D-C1-1 pattern)
- externalBin sidecar for the mem-mcp daemon (`mcp_serve`, features rocksdb+transformer — HEAVY:
  RocksDB + the bge_base transformer embedder + a model). GROUND the daemon's CLI (store path,
  socket path, key intake). Gitignore the binary; CI cross-build + model provisioning = S7.
  Rule-12 `[[drift]]` for the citrate-memories source pin. Lean tree: mem-mcp = spawned binary,
  NOT a Cargo dep → NO mem-* crates in src-tauri `cargo tree`.
- Supervise it under C1.0 (crash → restart; the daemon is singleton via the RocksDB LOCK — a
  restart after a stale socket must be safe, per its docs).

### C3-WP1 @rule8 — per-user encrypted store + key
- GROUND how mem-mcp takes its store-encryption key (the `.bge.memdag` is encrypted; the daemon
  needs the key). Hold that key in the OS keyring (C1.1 pattern — never on disk clear); pass it
  the way the daemon accepts it (env/arg/stdin — grounded). Per-user store dir lifecycle.
- Acceptance (Rule 11): the store on disk is ciphertext (`.bge.memdag` — raw-bytes grep finds no
  plaintext node text); the key is in the keyring. Kill-9 the daemon mid-write → restart recovers
  from the encrypted store (the daemon-durability property).

### C3-WP2 — MemoryDomain recall/search/neighbors over the local socket
- `MemoryDomain` real adapter: JSON-RPC `tools/call` (memory.recall / memory.search /
  memory.neighbors) over the daemon's Unix socket (`s.socketPath`) → the Storage constellation
  renders real nodes / labels / links / tenant counts from the store (replace the seed GRAPH
  module, NOT the UI — Rule 1). Lexical → semantic upgrade gated on the embedder/model.
- Acceptance: a recall/search query returns real nodes from the local store (data source: the
  MCP socket transcript); the constellation shows the personal tenant.

### C3-WP3 — chain-facts tenant via the E-4 ingestor
- Run/wire `mem-ingest` (E-4, merged) to ingest the 40204 contract catalog
  (`citrate-chain/contracts/addresses/40204.json`) → a **chain-facts** tenant in the store.
- Acceptance: `memory.recall(repo="chain-facts")`/search returns seeded nodes matching 40204.json
  (data source: the ingested catalog); the constellation shows personal + chain-facts tenants.

## Test plan (agentile:test-plan)
- CI-safe: a **stub MCP socket** (a helper answering tools/list + tools/call with fixture nodes)
  → MemoryDomain recall/search/neighbors parsing; the store-key-in-keyring + ciphertext-at-rest
  grep (against a fixture encrypted store or the stub's output); the supervised-restart of a stub
  daemon; the socket singleton/stale-socket safety.
- Live (documented, heavy): the REAL mem-mcp daemon (build with rocksdb+transformer + a model —
  may take long) → recall/search returns real nodes; chain-facts ingest from 40204.json. If not
  runnable headless, write the proof script + exact commands; do NOT fabricate a graph (Rule 1).
- Tripwire: ciphertext-at-rest grep; no plaintext store. Baseline: rust 204 → up; update BASELINE.md.

## Definition of done (C3) — and Phase C
- mem-mcp runs under the supervisor with an encrypted, keyring-keyed per-user store; Storage
  recall/search/neighbors are live over the socket; the chain-facts tenant is populated from
  40204.json; the constellation shows a real graph (or honest "coming" where a model isn't bundled).
- Gates green (cargo/vitest up, clippy -D, audit 0, lean tree); build-and-stop → independent
  review (@rule8 store key). Owner merges cleared code. Honest gaps (heavy model build; per-platform
  bundle + model = S7).
- **On C3 merge: Phase C is COMPLETE** → write the Phase-C `RETRO.md` (agentile:retro) + a journal;
  move the sprint to completed/.

## Out of scope
- Write tools (assert/diff) beyond the existing seam (later WP). The agent tool loop (Phase E / S4.5).
  Pinning (E-3). Per-platform model provisioning (S7).
