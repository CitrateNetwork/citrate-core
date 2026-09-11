---
created: 2026-09-11
author: Claude Fable 5
status: active
sprint: sprint-hermes-p1-memory
---

# Evidence — Hermes P1

## WP1.1 — corpus packer, hardened (monotone, no dupes)
- `docs_ingest::chunk_hash` — stable sha256 (first 16 bytes) content key.
- `docs_ingest::ingest_docs_incremental` — authors only chunks whose hash is not in
  `seen`; a failed author does NOT mark the chunk seen (retried next run).
- `memory::ingest_docs_corpus` — loads/persists the seen-set from a sidecar
  (`memory/docs-corpus.seeded`); an empty tenant resets the set (drift-safe).
- Tests (cargo, all green): `chunk_hash_is_stable_and_content_addressed`,
  `incremental_skips_already_seen_chunks_on_a_second_run`,
  `incremental_authors_only_the_new_docs_when_the_corpus_grows`,
  `incremental_does_not_mark_a_chunk_seen_when_its_author_fails`,
  `memory::tests::seeded_chunks_sidecar_round_trips_next_to_the_store`.
- TLA+: `formal/MemoryPack.tla` + `.cfg` — TLC **No error found** (221 distinct states;
  TypeOK + NoDupes + Integrity + MonotoneUnderIngest).

## WP1.2 — reference packs
- `docs-corpus/reference/`: `solidity-and-solc.md`, `the-evm-and-citrate-40204.md`,
  `rust-for-node-and-sidecars.md`, `front-end-design-principles.md`,
  `business-administration-basics.md`, `legal-foundations-for-web3.md`.
- Bundling: `docs-corpus/*` → `docs-corpus/**/*` in all 5 tauri configs so the
  `reference/` subdir ships in the resource dir.
- Test: `the_reference_packs_ship_and_are_ingestable` (asserts all six topics ship and
  chunk to real content). `read_corpus_dir` recurses into the subdir.

## WP1.3 — retrieval over the packed tenant (verified, already wired)
- `ai.rs` `AGENT_SYSTEM_PROMPT_TOOLS` instructs the model to prefer `memory_search`/
  `memory_recall` over the `citrate-docs` tenant and to say so — never fabricate — on an
  empty search.
- `store.ts` `handleTool` routes `memory_search`/`memory_recall` to the real
  `bridge.memory.search/recall(tenant, …)` (tenant `citrate-docs` default | `personal`),
  honest on failure.

## Suite
- `cargo test --lib`: 399 passed, 5 ignored.
- Frontend unchanged this WP set; last full run 480 passed (PR #36).
