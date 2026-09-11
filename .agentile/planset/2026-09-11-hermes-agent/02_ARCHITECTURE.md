---
created: 2026-09-11
author: Claude Opus 4.8, directed by @SaulBuilds + Luke
status: active
planset: 2026-09-11-hermes-agent
---

# Architecture

## 1. The model router (P0) — the spine

A single **ModelRouter** the interface exposes and every agent surface reads. It enumerates
and selects the active model from **three sources**, unified behind one selector:

```
                         ┌──────────────────────────────┐
   on-chain registry ───▶│                              │
   (ModelCooperative /   │        ModelRouter           │──▶ active model handle
    model registry)      │  enumerate() → ModelChoice[] │     (id, source, backend)
                         │  select(id)  → persists       │
   Models section  ─────▶│  active()    → ModelChoice    │──▶ Hermes chat + Agent
   (local GGUFs, verified)│                              │     surface + coach chat
                         │                              │
   gateway / Gemma ─────▶│                              │
                         └──────────────────────────────┘
```

- **Local GGUFs** — from the existing `model_catalog` / Models section (`is_file_ready`,
  verified+downloaded). Reuse, do not reimplement.
- **On-chain registry** — read registry-registered models (the `register_model` calldata
  path already exists in `storage.rs`); surface them as installable/selectable choices.
- **Gateway / Gemma** — the OpenAI-compatible gateway + the bundled/downloaded Gemma
  (the default out-of-box backend).

**Contract:** `ModelChoice { id, label, source: "local"|"registry"|"gateway", ready: bool }`.
`select()` persists the choice; `active()` is what the chat/agent send path resolves. The
router is the SINGLE place a backend is chosen — no surface hard-codes gateway-vs-local.
Rule 1: a not-ready choice is shown as such; selecting it triggers the real download/verify
(via the fixed off-main-thread path) or an honest "register/pull first".

## 2. Chat wiring (P0)

Promote the Agent surface to a real chat that reuses the coach-chat transcript UI
(`chatMsgs`, streaming, the now-fixed post-commit auto-scroll). Its send path resolves the
backend through `ModelRouter.active()`. Works out of the box on Gemma/gateway.

## 3. Knowledge substrate = pre-packed citrate-memories (P1) — NOT RAG

Hermes already reuses `mem-mcp` (the citrate-memories DAG). Instead of a bespoke RAG index,
**pre-pack the memory graph** at build/first-run with:

- the **Almanac** corpus (the docs content — the same source the docs site serves),
- reference knowledge: **solc, EVM, Rust, front-end design, business administration, legal**.

Retrieval = the existing `memory_recall`/`memory_search` over the packed tenant. Knowledge
that is **procedural** (e.g. "deploy a contract", "pull a model") is delivered as an
executable **skill** (P2/P3), not a memory node — the split is decided per topic in P1.

## 4. Skills from the chain (P2)

At startup, load the capability/skill manifest from the on-chain registry (the capsule/skill
registry) and seed the WASM-capsule set, so Hermes ships WITH skills. Every skill that signs
routes through the SignatureCeremony (Rule 3). A skill whose backing service is unwired
reports honest not-configured (Rule 1).

## 5. Headline skills (P3)

- **hf-model-pull-register** — download a GGUF from Hugging Face (reuse the fixed
  off-main-thread `model_download`/`model_catalog_download`), verify, then register it on-chain
  (the `register_model` ceremony) so it appears in the ModelRouter's registry source.
- **contract-deploy** — deploy a smart-contract app via the terminal, reusing the Agent-tab
  contract-deploy ceremony (simulate on a fork → decode calldata → human approves → broadcast).

## 6. Reuse map (consume/adapt, never fork)

| Need | Federation source |
|---|---|
| Agent loop + capsule skills + ApprovalQueue | citrate-agent-runtime (agent-sidecar) |
| Memory graph + recall | mem-mcp / citrate-memories |
| Local model catalog + download/verify | citrate-core `model_catalog`/`model.rs` (fixed off-main-thread) |
| On-chain model + skill registry | ModelCooperative / registry contracts (`storage.rs`) |
| Signing | SignatureCeremony (ceremony.rs) |
| Gateway | inference-gateway (OpenAI-compatible) |
