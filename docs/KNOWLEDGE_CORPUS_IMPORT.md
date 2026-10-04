---
created: 2026-10-01
updated: 2026-10-01
branch: hup/m2-knowledge
author: Larry Klosowski + Claude Opus 5.5
status: implemented; staging wired and bound to the bundled BGE model; release upload pending (see "Not done")
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
   app resource `knowledge-corpus/`. A directory counts only when it holds a
   `manifest.json`: every build ships `knowledge-corpus/README.md` so the bundle
   resource glob always matches, and a README-only directory is `no-bundle`.
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

## Release staging

The corpus (format `citrate-corpus/2`) is built in citrate-memories
(`scripts/build-corpus.sh`, `EMBED_BGE_DIR=` to precompute BGE vectors) and shipped
as the pinned `knowledge-corpus.tar.gz` runtime-deps asset. `release.yml` runs
`scripts/stage-knowledge-corpus.mjs` after the pin check: it re-hashes the manifest
(same canonical JSON as `mem_corpus::manifest::Manifest::compute_digest`), checks
every tenant file, vectors file and the skills.lock against it, refuses any extra
file or symlink, refuses a `mem-mcp` without `import-corpus`, and copies the corpus
into `src-tauri/knowledge-corpus/` next to the committed README. The
`tauri.bundle-*.conf.json` and `tauri.local-run.conf.json` overlays carry the
`knowledge-corpus/**/*` resource. Procedure: `docs/RELEASE.md` section 3.

## Offline needs the bundled BGE model

The import embeds with the store's embedder, and the corpus is useful only with the
bundled BGE model (`models/bge-base-en-v1.5/`: `config.json`, `tokenizer.json`,
`model.safetensors`, staged by `release.yml` from the pinned `bge-base-en-v1.5.tar.gz`).
Without it the import is skipped as `not-semantic`, and Storage says so plainly ("not
imported, this build has no bundled search model"). The release stager makes the
dependency explicit: `--bge-dir` is required, it refuses a missing or partial model, and it
refuses any tenant whose precomputed vectors were not made with exactly the bundled weights
(model id, dimension and `model.safetensors` sha256), because the importer would ignore those
vectors and every member would embed the whole corpus on their own CPU.

## Precomputed vectors and first-run time

Embedding the corpus on a member's CPU is slow: 2.7 to 3.7 nodes per second on an
Apple M2 Max (2026-10-01 builds), so the full 32,702-node corpus would hold the memory
store (and keep the daemon down) for two to three hours. A corpus built with
`EMBED_BGE_DIR` ships `tenants/<tenant>.vectors.f16`; the importer reuses them when the
model id, dimension and the sha256 of the bundled `model.safetensors` match, and the
report says how many nodes were embedded and how many took the bundled vectors
(`nodesEmbedded`, `vectorsReused`).

Measured on 2026-10-02 (M2 Max, the app's bundled BGE files, weights `c7c1988a...67d7`):
the release-time build embedded all 32,702 nodes in 10,010 s (niced, beside other work);
the corpus is 99,669,598 bytes on disk (49.4 MB corpus files, 50.2 MB vectors) and
58,498,298 bytes as `knowledge-corpus.tar.gz`. `stage-knowledge-corpus.mjs` with
`--bge-dir` accepted it ("vectors for every node match the bundled bge-base-en-v1.5"). A
real `mem-mcp import-corpus` into a fresh store then took 224 s with `vectorsReused`
32,702 and `nodesEmbedded` 0 (31,302 edges; the store is 262 MB), and a second import was
a no-op in 4.9 s.

## Answering with citations

Knowledge tenants are `citrate-docs`, `methodology`, `refs` and `skills`. The
in-app `memory_search` tool (`src/agent/knowledgeSearch.ts`) asks the daemon for
passages on those tenants (`memory.search` with `passages: true`), so the model
gets each passage's text and the `<repo>:<path>#<anchor>` citation to quote;
personal notes stay title-only. The `memory_search` tool object lives in
`knowledgeSearch.ts` (`MEMORY_SEARCH_TOOL`) and the agent's tool list uses it.

The Citrate QA eval measures this path (gate g2-knowledge). With `--memory-socket` and the
default `--retrieval-mode tool`, `scripts/eval-qa.mjs` offers the model the same
`MEMORY_SEARCH_TOOL`, runs each call on a `mem-mcp` daemon whose store imported the corpus,
renders the result with `formatMemoryHits`, and stops after the app's `AGENT_MAX_TURNS`
(`src/agent/eval/toolLoop.ts`). Every search's node ids are recorded per item, and every
citation in the answer is resolved to them: `citedNodes` per item and `citationNodeRate` in
the scorecard (the share of answer citations that name a node the run retrieved from the
imported graph). `--retrieval-mode passages` keeps the older retrieve-then-answer run.

## Licences

The corpus ships its own `NOTICE.md` and `manifest.json` (source, upstream, licence,
pinned commit and attribution per source). Every source in the spec is cleared. The Medusa
and Slither docs are AGPL-3.0 and ship by owner decision (2026-10-01) with attribution and
the upstream link; the only change is chunking for search. The licence review of the
bundled AGPL/GPL tools themselves is gate g3-licence (`docs/COMPONENT_UPDATER.md`).

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

- The release upload: `knowledge-corpus.tar.gz` and a `mem-mcp` built with
  `import-corpus` (`--features rocksdb,transformer`) must be uploaded to the
  `runtime-deps` prerelease and pinned in `src-tauri/runtime-deps.sha256` together.
  Until then the release workflow fails closed on the unpinned asset, and a local
  build without a staged corpus reports `skipped: no-bundle`.
- Linux and Windows release docs stage the same way; not yet exercised on those hosts.
