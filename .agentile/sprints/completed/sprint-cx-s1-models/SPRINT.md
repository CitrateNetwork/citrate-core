---
created: 2026-08-26T00:00:00Z
branch: cx/s1.<wp>-<slug> (one branch per WP; Lane A is serial)
author: Larry Klosowski (@SaulBuilds) + Claude Opus 4.8
status: archived
sprint: CX-S1
planset: Commons / citrate-core-social (citrate-federation/.agentile/planset/2026-08-26-citrate-core-social/)
tier: T1
lane: A (owns .agentile/cx-ownership.map lane s1 — model.rs, model_catalog.rs, serve.rs, ai.rs,
       connections.rs, oidc.rs + bridge/*/models.ts + slices/models.ts + surfaces/Models.tsx)
---

# Sprint CX-S1 — Model catalog & switcher (+ HF/GitHub OAuth) · "Commons"

## Goal

A built-in model switcher: pick and run any local model, plus download models from Hugging
Face and GitHub with easy OAuth. Lane A off the merged CX-S0 scaffold. Each WP is its own
branch off `main`, merged before the next (Lane A is serial; it shares `connections.rs`/
`model.rs`/`serve.rs`/`ai.rs`). Every WP passes `scripts/cx-ownership-check.sh s1`.

## Baseline (Rule 2 — must not decrease)
- src-tauri lib tests: 285 · kit: 192 · frontend: 344 (post-S0). CX-S1 adds tests per WP.

## Work packages (serial within the lane)
- [~] **S1.1** `Service::HuggingFace` in `connections.rs` (authorize/token/redirect/scope/env_prefix/
      ALL_SERVICES) + tests. HF connect is driven from the CX **Models** surface (s1-owned) via the
      existing generic `connections` domain — NOT the legacy Settings WIRED_CONN (out of s1's set). **S.**
- [ ] **S1.2** GitHub token exchange `Accept: application/json` fix in `oidc.rs::UreqClient::post_form`
      (else GitHub returns form-encoded and JSON parse fails). **S.**
- [ ] **S1.3** `ModelDescriptor` + generalize `ModelManager` off the single-model consts to per-model dirs. **L.**
- [ ] **S1.4** Catalog resolver: HF Hub API + GitHub Releases API; reuse `connection-hf`/`connection-github` tokens. **L.**
- [ ] **S1.5** Runtime-selectable `llama-server -m` (`select_model`); thread active id into `ai_chat_local`. **M.**
- [ ] **S1.6** Switcher UI in `surfaces/Models.tsx` + `slices/models.ts` + `bridge/*/models.ts`; wire local inference into chat. **L.**

## Method: TLA+ n/a · BDD = 04 C-16 scenarios · failing test first · Rule 3 (any chain write via ceremony) · Rule 8.

## Daily
- 2026-08-26 — Lane A opened. Name locked: **Commons** (D-25). Branch cx/s1.1-hf-oauth off main@40efe57. Starting S1.1.
