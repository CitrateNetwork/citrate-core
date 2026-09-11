---
created: 2026-09-11
branch: feat/hermes-p1-memory-substrate
author: Claude Fable 5
status: active
sprint: sprint-hermes-p1-memory
planset: 2026-09-11-hermes-agent
tier: T1
---

# Sprint — Hermes P1: Pre-packed memory substrate

Second phase of the Hermes build-out (`.agentile/planset/2026-09-11-hermes-agent`).
Hermes retrieves knowledge from a pre-packed citrate-memories tenant — NOT a bespoke
RAG index — packed at first run and grown across app versions.

## Why
For the agent to answer Citrate and reference questions without fabricating, it needs a
grounded knowledge base it can recall. The mem-mcp DAG already exists and is wired; P1
curates what ships in it and makes the packer safe to grow.

## Work packages
- **WP1.1 — Corpus packer (idempotent, hash-verified).** Pack the Almanac corpus into the
  `citrate-docs` tenant at first run. DONE (W3.2) + HARDENED this sprint: replaced the
  blanket "non-empty tenant → skip" gate with a per-chunk sha256 seen-set (sidecar next to
  the store), so a corpus that GROWS across versions re-packs only new chunks. Empty tenant
  resets the seen-set (no store/file drift). TLA+ `MemoryPack` (monotone, no dupes,
  integrity) — TLC-green.
- **WP1.2 — Reference packs.** Ship curated reference docs — solc, EVM, Rust, front-end,
  business-admin, legal — under `docs-corpus/reference/`, packed as memory nodes (procedural
  how-tos stay P3 skills). DONE.
- **WP1.3 — Hermes retrieval over the packed tenant.** The agentic chat tool loop recalls
  from `citrate-docs` via `memory_search`/`memory_recall` (Rule-1 honest emptiness). Already
  wired (W3.3): the tool-aware system prompt prefers the `citrate-docs` tenant, and
  `store.handleTool` routes `memory_search`/`memory_recall` to the real
  `bridge.memory.search/recall`. VERIFIED this sprint.

## Out of scope (later phases)
- P2 skills-from-chain (SkillRegistry read shipped separately, PR #36).
- P4 voice/journal (whisper STT), P5 extensibility.

## Dependencies / DGX
- The BGE embedder (~440 MB, bundled with mem-mcp) must be present for a semantic ingest;
  the packer is gated on it and skips honestly ("not-semantic") without it. mem-mcp Linux +
  Windows builds and the BGE model bundle come from the DGX side.
