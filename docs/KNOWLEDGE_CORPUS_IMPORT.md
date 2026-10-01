---
created: 2026-10-01
branch: hup/n4-corpus
author: Larry Klosowski + Claude Opus 5.5
status: implemented; release staging pending (see "Not done")
wp: HUP-S3.1
planset: .agentile/planset/2026-09-30-hermes-upskill
---

# First-run knowledge-corpus import (HUP-S3.1)

Hermes answers Citrate questions offline on first launch from a knowledge graph
that ships with the app. citrate-memories builds that graph at release time
(`mem-corpus`, spec `corpus/hermes-knowledge.toml`, format described in
citrate-memories `docs/KNOWLEDGE_CORPUS.md`). This app imports it once into the
member's local memory store.

## Flow

1. `store.startMemoryDaemon()` calls `bridge.memory.importKnowledge()` before it
   starts the daemon.
2. Rust (`src-tauri/src/knowledge_import.rs`, command `memory_import_knowledge`)
   resolves the corpus directory: `CITRATE_KNOWLEDGE_CORPUS_DIR` (dev), else the
   app resource `knowledge-corpus/`.
3. It reads the corpus `bundle_digest` from `manifest.json` and decides:
   - no corpus directory: `skipped: no-bundle`;
   - `memory/knowledge-corpus.imported` already holds this digest (and the store
     exists): `skipped: already-imported`, and the daemon is not touched;
   - no bundled BGE model: `skipped: not-semantic` (importing with the hashing
     fallback would lock a fresh store to lexical vectors);
   - otherwise it stops the daemon if it is running, runs
     `mem-mcp import-corpus <store> <corpus-dir>` with the daemon's embedder
     environment, and restarts the daemon afterwards if it was running.
4. The importer verifies the corpus against its manifest before writing anything
   and prints JSON lines. Rust forwards each line as the
   `memory://knowledge-import-progress` event; the Storage surface shows the
   importer's own per-tenant counts.
5. The marker is written only after a `done` line whose digest equals the bundled
   manifest's. An `error` line, a non-zero exit, a missing `done` line or a digest
   mismatch is `failed` with the reason, no marker is written, and the next
   launch retries. Storage shows the failure with a Retry button.

The store also records each tenant's bundle hash, so a lost marker re-imports
nothing. A newer corpus (new digest) imports only the tenants that changed.

## Safety

- The command is async and runs off the main thread (`crate::blocking::off_main`).
  It is in the main-window ACL and not in the pop-out capability.
- Nothing signs or holds a key. The import is a local memory-store write of
  release content. The importer accepts only knowledge tenants (`citrate-docs`,
  `skills`, `refs`, `methodology`), Derived-plane unsigned nodes, and
  `DerivedFrom` / `References` edges inside the same bundle, so it cannot touch
  `personal` or `chain-state` or retire anything.

## Tests

- `knowledge_import::tests` (Rust): contract parsing, digest reading, every skip
  gate, marker write and reuse, wiped-store marker, honest failures, per-store
  overlap guard, daemon stop and restart, ACL/off-main tripwire. The `#[ignore]`d
  `live_real_mem_mcp_imports_the_fixture_corpus_once` runs the real `mem-mcp`
  binary (`CITRATE_MEM_MCP_BIN=…`) against `tests/fixtures/knowledge-corpus`.
- `knowledge.contract.test.ts`, `store.test.ts`, `storageHonesty.test.tsx`
  (vitest): bridge contract, ordering (import before start), failure display.

## Not done

- The release workflow does not stage `knowledge-corpus/` yet, and the
  `mem-mcp` binary in the `runtime-deps` prerelease predates `import-corpus`.
  Until both land, a packaged app reports `skipped: no-bundle`. If a corpus were
  staged with the old binary, that binary would not answer; the import is killed
  after 120 s without a first line and reported as failed. Stage both together.
- `tauri.conf.json` has no `knowledge-corpus` resource entry yet (adding one
  before the directory is staged would break packaging).
