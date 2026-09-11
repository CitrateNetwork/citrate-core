---
created: 2026-09-11
branch: feat/hermes-p5-user-skills
author: Claude Fable 5
status: active
sprint: sprint-hermes-p5-extend
planset: 2026-09-11-hermes-agent
tier: T1
---

# Sprint — Hermes P5: Extensibility (final phase)

The member extends Hermes with their OWN skills.

## Design decision
A capsule skill is compiled WASM (the marketplace/Commissary path — not authorable
in-app). Per US-P5-1's AC ("backed by **Gemma or the gateway**"), a USER skill is a
lighter **prompt-skill**: a named instruction the member writes, RUN against the
active model (the ModelRouter's Gemma / gateway / local backend). It persists with the
member's local state (like the journal), lists in the Agent surface, and — because a
run just drives the agentic chat — any chain action it leads to still stops at the
SignatureCeremony (Rule 3 holds by construction; nothing in P5 signs).

## Work packages
- **WP5.1 — add/extend a skill.** DONE. `src/agent/userSkills.ts` (pure: validate +
  normalize + dedupe + cap); `store.addUserSkill` persists to local state; the Agent
  "Your skills" card adds one (name + instruction) and runs it (`runUserSkill` → sends
  the instruction to the active model via `sendChat`).
- **WP5.2 — persist + list; ceremony-gate signing.** DONE. `userSkills` is in
  `PERSIST_KEYS` (localStorage, same as the journal); the card lists + removes; a run's
  chain effects route through the existing ceremony (unchanged, verified by design).

## Adversarial coverage (this phase)
Negative/tripwire tests in `userSkills.test.ts`: blank name/instruction rejected;
duplicate name (case-insensitive) rejected — no silent overwrite; over-length fields
rejected rather than truncated (intent preserved, Rule 1); max-skills cap enforced; the
run prompt passes the instruction verbatim as text (no eval / no tool-call injection).

## Notes / follow-ons
- WASM user-capsules (with the cargo-component pipeline) remain the marketplace path,
  out of scope for in-app authoring.
- A run currently sends to the dashboard chat; a future "skills as first-class agent
  tools the model can call by name" is a natural extension.
