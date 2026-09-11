---
created: 2026-09-11
author: Claude Opus 4.8, directed by @SaulBuilds + Luke
status: active
planset: 2026-09-11-hermes-agent
---

# Cadence — the public-build habit

citrate-core is going public. The habit: **plan in the open, spec what matters, and never
skip the narrative.** Every sprint in this planset carries the following artifacts.

## Per sprint (each `sprint-hermes-pN-*`)
1. **SCOPE.md** — what/why, the WPs, the acceptance criteria (BDD given/when/then), and the
   TLA+ spec(s) the sprint must author FIRST (spec-first). Frontmatter: created/branch/author/status.
2. **TLA+ first.** The WP that authors the `.tla` + `.cfg` and gets it TLC-green in the
   pre-push local-CI gate is the sprint's first WP — before the code it constrains (see
   `03_TLA_SPECS.md`). Where a property already holds in an existing spec, reference it.
3. **EVIDENCE.md** — the proof each WP landed (test output, TLC result, screenshots via the
   docs-screenshot harness, real chain reads). Rule 1: evidence is real, never asserted.
4. **RETRO.md** — a retrospective at sprint close: what shipped, what regressed, what the
   next sprint inherits. (Matches the existing `sprint-*/RETRO.md` house pattern.)

## Journals (narrative field-log)
- Location: `../citrate-journals` (no remote — owner keeps the narrative field-logs there).
- Cadence: a short journal entry at each meaningful step (a hard bug, a design fork, a
  shipped phase). These are the human story of the build, not the spec.

## Essays (the "why", public-facing)
- Location: `docs/essays/` (in-repo, public).
- Cadence: one essay per phase that's worth explaining to the world — e.g. "why a model
  router, not a setting", "why pre-packed memory instead of RAG", "seeding an agent's skills
  from the chain". These become part of the public record when the repo opens.

## Release cadence (ties to the signed-build habit)
- Each shipped phase that changes the app → a signed release via the release-mirror runbook
  (bump → bundle-lite build → notarize → GH release → DGX mirror → parity gate). The DGX
  re-mirror prompt is produced in-chat each time.
- Coverage ratchet (Rule 2) never decreases; `.agentile/coverage/BASELINE.md` records it.

## Definition of done (per sprint)
Spec TLC-green · code + tests local-green (no ratchet drop) · EVIDENCE.md complete ·
RETRO.md written · journal entry + (if phase-worthy) an essay · @rule8 sign-off where the
sprint touches money/keys/identity.
