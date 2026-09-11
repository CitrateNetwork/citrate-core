---
created: 2026-09-11
author: Claude Opus 4.8, directed by @SaulBuilds + Luke
status: active
planset: 2026-09-11-hermes-agent
---

# Sprints & Work Packages

Each phase → a sprint dir `.agentile/sprints/backlog/sprint-hermes-pN-*/` with `SCOPE.md`,
`EVIDENCE.md`, `RETRO.md` (move to `active/` then `completed/` per the house convention).
Each WP is one PR, verified locally (dark CI), no test-count regression (Rule 2).

## P0 — Foundation + model router (`sprint-hermes-p0-router`)
- **WP0.1** ModelRouter core (pure): `ModelChoice`, `enumerate()`, `select()`, `active()`;
  unit-tested against fixtures for all three sources. TLA+: `ModelRouter` (see 03).
- **WP0.2** Wire the three sources: local (`model_catalog`), registry (read), gateway/Gemma.
- **WP0.3** Router UI (a picker) in the interface; persists selection.
- **WP0.4** Promote the Agent surface to a real chat reusing the coach transcript UI; send
  path resolves via `ModelRouter.active()`. Works out of the box on Gemma/gateway.

## P1 — Pre-packed memory substrate (`sprint-hermes-p1-memory`)
- **WP1.1** Corpus packer: pack the Almanac corpus into a citrate-memories tenant at
  build/first-run (idempotent, hash-verified). TLA+: `MemoryPack` (monotone, no dupes).
- **WP1.2** Reference packs: solc, EVM, Rust, front-end, business-admin, legal — decide
  per-topic memory-node vs skill; pack the memory ones.
- **WP1.3** Hermes retrieval over the packed tenant (`memory_recall`/`memory_search`);
  Rule-1 honest emptiness.

## P2 — Skills from the chain (`sprint-hermes-p2-skills`)
- **WP2.1** Read the skill/capsule manifest from the on-chain registry at startup.
- **WP2.2** Seed the WASM-capsule set from it; ceremony-gated (Rule 3). TLA+: `SkillLoad`
  (deterministic, no unapproved execution).
- **WP2.3** Honest not-configured for a skill whose backing service is unwired.

## P3 — Headline skills (`sprint-hermes-p3-headline`)
- **WP3.1** `hf-model-pull-register`: download (off-main-thread) → verify → register on-chain
  → appears in ModelRouter registry source.
- **WP3.2** `contract-deploy`: reuse the Agent-tab deploy ceremony (fork-sim → decode →
  approve → broadcast).

## P4 — Chat UX (`sprint-hermes-p4-ux`)
- **WP4.1** Voice-to-text input (on-device where possible; honest fallback).
- **WP4.2** Journal integration (Hermes reads/writes the journal surface).
- **WP4.3** Streaming + auto-scroll (reuse the v0.2.4 post-commit scroll).

## P5 — Extensibility (`sprint-hermes-p5-extend`)
- **WP5.1** User adds/extends a skill (capsule), backed by Gemma or the gateway.
- **WP5.2** Persist + list user skills; ceremony-gate any that sign.

## Dependencies / ordering
P0 gates everything (the router + chat are the surface). P1 can run parallel to P0 after
WP0.4. P2 depends on P0 (skills need a running agent) + the registry read. P3 depends on P2.
P4/P5 depend on P0. @rule8 sign-off gates P3 (money/keys) before any deploy.
