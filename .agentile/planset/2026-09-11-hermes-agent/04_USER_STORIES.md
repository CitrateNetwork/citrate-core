---
created: 2026-09-11
author: Claude Opus 4.8, directed by @SaulBuilds + Luke
status: active
planset: 2026-09-11-hermes-agent
---

# User Stories

Format: **As a** ROLE, **I want** GOAL, **so that** VALUE. Acceptance criteria are the
BDD given/when/then in `04_FEATURES_BDD` per sprint (authored with the sprint SCOPE).

## P0 — router + chat
- **US-P0-1** As a member, I want to **pick which model the agent uses** from one place, so
  that I can switch between my local Gemma, a gateway model, and a registry model without
  hunting through settings. *AC: the router lists all three sources with a ready state;
  selecting persists and the very next message uses it.*
- **US-P0-2** As a member, I want the **agent to work the moment I open the app**, so that I
  don't have to configure anything first. *AC: with no local model, the agent answers on the
  gateway/Gemma by default.*
- **US-P0-3** As a member, I want a **familiar chat**, so that talking to Hermes feels like
  the coach chat (streaming, auto-scroll).

## P1 — knows the ecosystem
- **US-P1-1** As a member, I want Hermes to **know Citrate** (the Almanac), so that it
  answers product/chain/SDK questions accurately instead of guessing. *AC: a "how does X
  work" question returns an answer grounded in the packed corpus, with honest emptiness when
  there's no match (no fabrication).*
- **US-P1-2** As a builder, I want Hermes to **know solc/EVM/Rust/front-end/business/legal
  basics**, so that it can help me build and operate without me leaving the app.

## P2 — ships with skills
- **US-P2-1** As a member, I want Hermes to **already have skills** on first run, so that it's
  useful immediately instead of empty. *AC: the skill list is seeded from the chain at
  startup; a skill that isn't wired says so.*

## P3 — does the real work
- **US-P3-1** As a builder, I want to **tell Hermes to download and register a model from
  Hugging Face**, so that I don't do it by hand. *AC: it downloads (off-main-thread, with
  progress), verifies, and registers on-chain via the ceremony; the model then appears in the
  router.*
- **US-P3-2** As a builder, I want to **deploy a smart-contract app from chat**, so that I can
  ship from one place. *AC: it simulates on a fork, shows the decoded calldata at the
  ceremony, and only broadcasts after I approve.*

## P4 — talk to it, keep the thread
- **US-P4-1** As a member, I want to **talk to Hermes by voice**, so that I can work
  hands-free. *AC: voice-to-text input with an honest fallback when unavailable.*
- **US-P4-2** As a member, I want Hermes to **connect to my journal**, so that it can recall
  and add to my field notes.

## P5 — make it mine
- **US-P5-1** As a power user, I want to **add my own skills** to Hermes, backed by Gemma or
  the gateway, so that I can extend it to my workflow. *AC: a user-added skill persists,
  lists, and (if it signs) routes through the ceremony.*
