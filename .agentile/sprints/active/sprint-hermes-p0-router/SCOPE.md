---
created: 2026-09-11
branch: (unstarted)
author: (assign at start)
status: active
sprint: sprint-hermes-p0-router
planset: 2026-09-11-hermes-agent
tier: T1
---

# Sprint — Hermes P0: Foundation + Model Router

First sprint of the Hermes build-out planset (`.agentile/planset/2026-09-11-hermes-agent`).
Establishes the model router (the spine) and promotes the agent surface to a real chat.

## Why
Hermes is robust but unextended. Nothing works out of the box because there is no single
model backend selector and the agent surface is not a chat. P0 fixes the foundation so every
later phase (skills, memory, headline skills) has a running, model-backed chat to build on.

## Work packages
- **WP0.1 — ModelRouter core (pure + TLA+ first).** Author `spec/hermes/ModelRouter.tla`
  (+ `.cfg`), TLC-green (INV-Router-1/2/3, LIVE-Router-1 — see planset 03). Then the pure
  TS: `ModelChoice`, `enumerate()`, `select()`, `active()`, unit-tested on fixtures.
- **WP0.2 — Wire the three sources.** local (`model_catalog`/`is_file_ready`), on-chain
  registry (read `register_model` entries), gateway/Gemma.
- **WP0.3 — Router UI.** A picker in the interface; selection persists; the Models section
  and the picker share the same source of truth.
- **WP0.4 — Agent chat.** Promote the Agent surface to a real chat reusing the coach
  transcript UI + the v0.2.4 post-commit auto-scroll; send path resolves `ModelRouter.active()`;
  default gateway/Gemma so it works out of the box.

## Acceptance (BDD — expand in 04_FEATURES_BDD at start)
- Given no local model, when I open the app and message the agent, then it answers on the
  gateway/Gemma (out of the box).
- Given a downloaded+verified local model and a gateway model, when I open the router, then
  both appear with a ready state; when I select one, then the next message uses it.
- Given a not-ready registry model, when I select it, then it triggers the real
  download/verify or an honest "register/pull first" — never a silent stuck state.

## Definition of done
TLA+ TLC-green · code+tests local-green (no ratchet drop) · EVIDENCE.md complete ·
RETRO.md written · journal entry in citrate-journals · essay "why a model router, not a
setting" in docs/essays (phase-worthy).

## Evidence / Retro
See `EVIDENCE.md` and `RETRO.md` in this dir (created as the sprint runs).
