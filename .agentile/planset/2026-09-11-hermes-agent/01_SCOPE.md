---
created: 2026-09-11
author: Claude Opus 4.8, directed by @SaulBuilds + Luke
status: active
planset: 2026-09-11-hermes-agent
---

# Scope of Work

## In scope
- A **model router** in the interface unifying local / on-chain-registry / gateway models (P0).
- Promoting the agent surface to a **real chat** on the router backend, working out of the box (P0).
- **Pre-packing citrate-memories** with the Almanac corpus + solc/EVM/Rust/front-end/business/legal (P1).
- **Seeding skills from the chain** at startup (P2).
- Two headline skills: **HF model pull+register** and **contract deploy via terminal** (P3).
- Chat UX: **voice-to-text**, **journal integration**, streaming/auto-scroll (P4).
- **User extensibility** of skills, backed by Gemma/gateway (P5).
- TLA+ specs for the router, memory-pack, and skill-load; journals + essays + retros per sprint.

## Out of scope (this planset)
- Re-architecting the agent-runtime, mem-mcp, model_catalog, or the chain contracts — we
  **consume/adapt** them (CLAUDE.md Rule 6). New capability lands in citrate-core's app layer
  or as a capsule skill, not by forking a federation crate.
- New chain contracts. If the on-chain skill/model registry needs a shape it doesn't have,
  that's a citrate-chain change tracked separately, not done here.
- Server-side identity/authority work (that's citrate-identity).

## Dependencies (all EXISTING — consume, don't fork)
- citrate-agent-runtime (agent-sidecar, ApprovalQueue, WASM capsules)
- mem-mcp / citrate-memories (the memory DAG + recall)
- citrate-core `model_catalog`/`model.rs` (local model download/verify — off-main-thread as of v0.2.3)
- ModelCooperative / registry contracts (`storage.rs` `register_model`)
- SignatureCeremony (ceremony.rs)
- inference-gateway (OpenAI-compatible)

## Constraints
- **T1 @rule8**: P3 (money/keys via deploy + register) needs security sign-off before deploy.
- **Rule 1** honesty on every surface; **Rule 2** coverage ratchet; **Rule 3** all signing via ceremony.
- Dark CI: verify locally, pre-push local-CI gate is the real gate.

## Success
A member opens the app and immediately talks to an ecosystem-aware Hermes on a model they
chose; it ships with skills; it can pull+register a model and deploy a contract from chat;
they can extend it. Every phase leaves a spec, an evidence trail, a journal, and (where
worthwhile) an essay — the public build habit.
