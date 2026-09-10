---
created: 2026-07-13
branch: feat/core-c3-memory
author: Claude Opus 4.8, directed by @SaulBuilds
status: built — build-and-stop → independent review (@rule8)
sprint: CORE-C1 / C3 (storage/memory — mem-mcp sidecar + MemoryDomain + chain-state tenant) — @rule8
grounded_in:
  - citrate-memories crates/mem-mcp/examples/mcp_serve.rs (daemon), src/lib.rs (JSON-RPC tools),
    crates/mem-store/src/shred.rs + lib.rs (encryption model), crates/mem-ingest/src/chain.rs +
    examples/chain_ingest.rs (E-4 chain catalog ingest)
  - src-tauri/src/supervisor.rs (C1.0), node.rs (C1.1 keyring/sidecar mirror), agent.rs (C1.2
    transport/sidecar mirror), src/bridge/domains.ts MemoryDomain, src/surfaces/Storage.tsx
depends_on:
  - C1.0 (supervisor) MERGED, C1.1 (node sidecar + keyring pattern) MERGED, C1.2 MERGED
---

# C3 — storage/memory: mem-mcp sidecar + MemoryDomain + chain-state tenant @rule8

## Grounded mem-mcp facts (citrate-memories, read from source)
- **Daemon** `mcp_serve` (examples/mcp_serve.rs): CLI `mcp_serve <db-path> <sock-path>` —
  positional args 1,2 (:137-138), defaults `./data/federation.memdag` / `./data/memdag.sock`.
  Build features `rocksdb,transformer`. **Singleton via RocksDB LOCK**: `open_rocksdb_auto`
  takes the DB lock (:142) BEFORE the socket; a second daemon on the same DB exits at open.
  **Stale-socket-safe**: holding the DB lock proves any existing socket file is stale →
  `remove_file` then bind (:151-156). Env: `MEM_CHECKPOINT_INTERVAL_SECS` (default 1800, 0 to
  disable), `MEM_CHECKPOINT_KEEP` (default 3). Audit chain `<db>.audit.jsonl`.
- **Socket protocol** (src/lib.rs): newline-delimited JSON-RPC 2.0. Methods `initialize`,
  `tools/list`, `tools/call`, `ping`. Tools/call requests:
  `memory.recall {repo, budget=15}` (:238), `memory.search {repo, query, budget=10}` (:248),
  `memory.neighbors {repo, id_prefix, budget=20}` (:266). Every response:
  `{"content":[{"type":"text","text":...}],"isError":bool}`. Tenant = `repo`.
- **Encryption / KEY INTAKE (@rule8 — the load-bearing finding):** mem-store seals per tenant
  with XChaCha20-Poly1305 (shred.rs). **The tenant keys are stored INSIDE the store's own
  `KEYS` RocksDB column family, on disk beside the data.** `open_rocksdb_auto(path)` takes
  ONLY a path — **the daemon accepts NO encryption key** (no env, no arg, no stdin). A store
  is "encrypted" iff its KEYS CF is non-empty (mem-store lib.rs:166 `new_auto`). The shred.rs
  module doc is explicit: *"moving key custody out of the store (citrate-identity / operator
  HSM) is the v2 federation step."* So the literal C3 ask — "hold the store key in the keyring,
  pass it to the daemon" — has NO seam to attach to today (see Concern 1).
- **Chain ingest (E-4)**: `ingest_chain_catalog(path, store, embedder, now_ms)` (chain.rs),
  example `chain_ingest`. **Tenant is `chain-state`** (`CHAIN_STATE_TENANT`), NOT `chain-facts`
  (see Concern 2). Deterministic: same catalog → byte-identical node ids → re-run is a no-op.

## What this build does (WP0-WP3)
- **WP0** — mem-mcp `mcp_serve` bundled as a Tauri `externalBin` (`binaries/mem-mcp`,
  gitignored) in the overlay; spawned+supervised under C1.0 (singleton = RocksDB LOCK by
  construction; stale-socket-safe by construction). Lean tree: mem-mcp is SPAWNED, not a Cargo
  dep — no `mem-*` crate in `src-tauri` `cargo tree`.
- **WP1 @rule8** — the store lives in a **per-user app-data dir** (`memory/store.bge.memdag`);
  citrate-core mints/holds a per-user **store wrapping key** in the OS keyring (C1.1 pattern),
  and passes it to the daemon via `CITRATE_MEM_STORE_KEY` env — honoured IFF the daemon accepts
  it. Because the grounded daemon does NOT (Concern 1), the env is a forward-compatible seam;
  the ciphertext-at-rest guarantee we CAN prove today comes from the store's own per-tenant
  seal (node text is XChaCha20 ciphertext on disk). Ciphertext-at-rest tripwire proves node
  text is absent from raw bytes.
- **WP2** — `MemoryDomain` gains `search`/`neighbors`; a real JSON-RPC-over-Unix-socket adapter
  (tauri) + a sim adapter. The Storage constellation renders REAL nodes/labels/links/tenant
  counts parsed from the store (replaces the seed GRAPH module, not the UI). Semantic gated on
  the bundled model (honest lexical label until the model lands).
- **WP3** — run/wire `mem-ingest` chain catalog ingest of `citrate-chain/.../40204.json` → the
  `chain-state` tenant; `memory.recall/search(repo="chain-state")` returns catalog nodes; the
  constellation shows personal + chain-state tenants.

## Concerns surfaced (NOT acted on — for the reviewer)
1. **@rule8 key-intake gap (design-level).** The daemon has no store-key intake; keys live on
   disk in the KEYS CF. True keyring custody of the store key requires wrapping the KEYS CF (or
   the store dir) under a keyring-held KEK — a mechanism mem-store does not expose. We implement
   the keyring seam that IS actionable (mint/hold a per-user wrapping key + own the per-user
   store dir + pass it via env for forward-compat) and prove ciphertext-at-rest via the store's
   OWN per-tenant seal. The residual: the tenant keys are still on disk (their own CF), so this
   is NOT yet the full @rule8 property C1.1 achieved for the node. Needs an owner decision +
   an upstream mem-store change (external KEK / citrate-identity custody).
2. **Tenant naming drift.** UI + prompt say `chain-facts`; the real E-4 ingest tenant is
   `chain-state`. We wire to the REAL `chain-state` (Rule 1) and label the constellation from
   the actual tenants present. The UI's `chain-facts` copy is a cosmetic label mismatch to
   reconcile (rename UI copy OR alias in the tenant-count map) — flagged, minimal UI touch made.
3. **Live graph is heavy.** Building the real daemon needs rocksdb+transformer+a ~440MB bge
   model; CI cannot. The live recall/search + chain ingest is a documented proof script with
   exact commands (Rule 1 — no sim graph presented as live).
