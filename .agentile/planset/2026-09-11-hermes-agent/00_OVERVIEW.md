---
created: 2026-09-11
branch: docs/hermes-agent-planset
author: Claude Opus 4.8 (Hermes build-out planning), directed by @SaulBuilds + Luke
status: active
repo: citrate-core
tier: T1
planset: 2026-09-11-hermes-agent
---

# Hermes Agent Build-Out — Planset Overview

> **Public build.** citrate-core is going public. This planset is authored in the
> Agentile format so the plan, the specs, and the retrospectives are part of the
> public record — the habit is: plan in the open, spec what matters in TLA+, keep a
> journal + an essay + a retro per sprint.

## The problem

The Hermes agent is **robust underneath but unextended** — a "nothing burger" UX. The
sidecar (agent-sidecar in citrate-agent-runtime), the human-in-the-loop ceremony
(`ApprovalQueue`), and WASM-capsule skills all exist, but Hermes ships with **no seeded
skills**, **no ecosystem awareness**, and a weak surface. It does not manifest as a real
chat agent, does not know Citrate, and cannot yet do the things a Citrate power-user needs
(pick a model, download/register a model, deploy a contract).

## The goal

Turn Hermes into a **real, ecosystem-aware chat agent** that works out of the box on Gemma
or the gateway, is pre-loaded with skills from the chain, knows the ecosystem, and can be
extended by the user.

## Phases (each is its own sprint under `.agentile/sprints/`)

| Phase | Deliverable | Sprint |
|---|---|---|
| **P0** | Foundation + **model router** — chat surface + a router that picks through models, wired to the on-chain **registry**, the **Models section** (local GGUFs), and the **gateway**/Gemma. One router, three sources. | `sprint-hermes-p0-router` |
| **P1** | Knowledge substrate = **pre-packed citrate-memories** (NOT bespoke RAG): the Almanac corpus + solc/EVM/Rust/front-end/business-admin/legal reference. Some delivered as skills. | `sprint-hermes-p1-memory` |
| **P2** | **Seed skills from the chain** — load the capability/skill set from on-chain registries at startup; Hermes ships WITH skills. | `sprint-hermes-p2-skills` |
| **P3** | Headline skills — **HF model download + register**, **deploy smart-contract apps via terminal** (reuse the ceremony). | `sprint-hermes-p3-headline` |
| **P4** | Chat UX — **voice-to-text**, **journal integration**, streaming, auto-scroll. | `sprint-hermes-p4-ux` |
| **P5** | **Extensibility** — user adds/extends skills, backed by Gemma or the gateway. | `sprint-hermes-p5-extend` |

## The two locked refinements (owner + Luke, 2026-09-11)

1. **Model router is the spine.** Not just "Gemma or gateway" — a router in the interface
   that enumerates + selects models from THREE sources (on-chain registry, local Models
   section, gateway) and is the single backend selector every agent surface reads.
2. **citrate-memories, pre-packed — not RAG.** The knowledge substrate is the
   citrate-memories DAG (mem-mcp, which Hermes already reuses), pre-packed with the corpus
   above. Split reference knowledge that is better as an executable SKILL out of the memory.

## Non-negotiables

- **Consume/adapt, never fork** the federation crates (agent-runtime, mem-mcp,
  model_catalog, registry, ceremony). See CLAUDE.md Rule 6/9.
- **Rule 1 (honesty).** Every surface states what is real; no fabricated skill result,
  model, memory hit, or tx. A skill that isn't wired says so.
- **Rule 3 (ceremony).** Every signature — user, node-agent, chat-agent, micro-app —
  routes through the SignatureCeremony. Hermes skills that sign are no exception.
- **T1 @rule8.** Money/keys/identity paths need security sign-off before deploy.

## Documents in this planset

- `01_SCOPE.md` — scope of work, in/out, dependencies.
- `02_ARCHITECTURE.md` — model router, chat wiring, memory substrate, skills-from-chain.
- `03_TLA_SPECS.md` — the specs to build where none exist (skeletons + properties).
- `04_USER_STORIES.md` — user stories per phase.
- `05_SPRINTS_AND_WPS.md` — phases → sprints → work packages.
- `06_CADENCE.md` — the public-build habit: journals, essays, retrospectives.
