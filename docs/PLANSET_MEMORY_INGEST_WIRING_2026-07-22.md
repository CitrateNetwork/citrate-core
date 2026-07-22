---
title: Planset — wire real memory ingest (write path into the mem-dag)
created: 2026-07-22
branch: main
author: Claude (Citrate Core session)
status: proposed
scope: citrate-core (app) + citrate-memories (daemon identity) ; SIWE/F-5 identity is DGX/OIDC-coupled
---

# Planset — Memory ingest wiring

## Goal

Make the memory graph actually fill: the app writes memories (signed assertions)
into the mem-dag via the running `mem-mcp` daemon, attributed to the user's REAL
identity, from real sources — first a manual "remember", then auto-ingest from app
activity. The read side (`memory.search`/`recall`) already works; ingest is what's
missing.

## Grounding (verified 2026-07-22)

- **Daemon is running and write-capable.** `mem-mcp` (the `mcp_serve` target)
  exposes the write tools `memory.assert`, `memory.merge_diff`, `memory.propose_edge`,
  `memory.confirm_edge` (`crates/mem-mcp/src/lib.rs:258-268`), backed by
  `mem-assert` `assert_node`/`assert_edge`. (The old "write tools are a later WP"
  doc comment at `lib.rs:7` is STALE — they exist.)
- **BUT the daemon runs with a DEMO identity.** `mcp_serve.rs:233`
  `new_with_asserter(store, demo_grant(), Asserter::new(SigningKey::from_bytes([2u8;32])))`;
  the grant is a "demo wildcard" (`[1u8;32]`). Comment `mcp_serve.rs:18-19`:
  **"per-user signed grants (citrate-identity SIWE) are the v2 integration (F-5)."**
  So any write today is attributed to a FAKE key — wrong provenance (Rule 1).
- **The app has NO ingest driver.** Nothing in `citrate-core/src-tauri/src/memory.rs`
  calls `memory.assert`; the only "memory write" is `store.ts:1113/1120` — an explicit
  DEMO ("Approve a memory write (demo — not durable)", "does not write to the memory
  graph"). This is the "nothing ingests" gap.
- The app spawns the daemon with args `[store_path, socket_path]` + `MEM_STORE_KEY`
  env (`memory.rs:553`) — **no identity is passed**, which is why it falls to the
  demo asserter.

## THE dependency — real asserter identity (SIWE/F-5, DGX/OIDC-coupled)

Honest ingest requires the daemon to sign assertions with the USER's identity + a
real `CapabilityGrant` (issued via citrate-identity SIWE), not the demo keys. Memory
note: "auto-ingest + team gateway blocked on DGX deploy + OIDC". So WP-1 is coupled
to the identity/OIDC work — until it lands, any ingest is demo-provenance and must
stay gated/honest.

## Work packages

### WP-1 — real asserter identity (BLOCKER for honest provenance)
- Start the daemon with the user's signing identity + a real SIWE-issued
  `CapabilityGrant`, replacing the demo `demo_grant()` / `[2u8;32]` asserter. Either
  parameterize `mcp_serve` to accept identity/grant (env/arg/handshake) or ship a
  real daemon entrypoint (not the example). The app passes the user's identity/grant
  when spawning (or via the socket handshake).
- Coupled to OIDC/SIWE (DGX). Name the source of the grant (citrate-identity endpoint).
- Gate: an ingested node's `asserter`/signature verifies to the real user, not `[2u8;32]`.

### WP-2 — app ingest command (can build in parallel; demo-provenance until WP-1)
- Rust `#[tauri::command]` `memory_assert` that sends `memory.assert {repo, kind,
  content}` over the daemon socket (reuse `memory.rs`'s JSON-RPC transport) and
  returns the new node's content-hash. Zero `.unwrap()`; honest error when the daemon
  isn't running (`MemoryError::NotRunning`). Data source named: the mem-mcp socket +
  `memory.assert` tool.
- Gate: a written node is retrievable via `memory.search`/`recall`.

### WP-3 — manual "remember"
- An app action (from chat / journal / a highlighted entry) that calls `memory_assert`
  with real content, routed through the human-approval path (the real memory-write
  confirmation, NOT the `store.ts:1113` demo toast). Replaces the demo write.

### WP-4 — auto-ingest
- A driver that turns real app activity into assertions automatically — journal
  entries, agent decisions (the ceremony/agent origin events), wallet/chain events,
  chat turns — with dedup, node kinds, and edges (`propose_edge`/`confirm_edge`).
  This is the "auto-ingest" the roadmap names; it's what makes the graph grow without
  a manual click.

## Sequencing & gates
WP-2 can be built now (functional, but demo-provenance — gate it honestly). WP-1
gates *honest* provenance and is DGX/OIDC-coupled. WP-3 → WP-4 layer on top. Hard
gates: (a) ingested nodes carry the REAL user identity (not `[2u8;32]`); (b) ingested
nodes are queryable via the existing read tools; (c) no demo-provenance in a release
build; (d) writes are human-approved; (e) zero mocks / zero unwraps / data sources
named.

## Owner decision before WP-1
Confirm the **SIWE/F-5 per-user grant** path is ready on the identity/OIDC side (DGX),
or sequence WP-1 alongside that work — real-provenance ingest can't land without it.

## Related
- Read side (`memory.search`/`recall`/`neighbors`/`as_of`/`verify`) already works
  against the running daemon — once ingest lands, the graph is immediately queryable.
- Sibling build: `docs/PLANSET_PIN_CONTRACT_WIRING_2026-07-22.md`.
