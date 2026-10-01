---
created: 2026-10-01
branch: hup/n4-personas
author: Larry Klosowski + Claude Opus 5.5
status: implemented (names pending owner sign-off)
wp: HUP-S3.3 + HUP-S3.7
story: US-3.3
repos: citrate-agent-runtime (hup/n4-personas), citrate-core (hup/n4-personas)
---

# WP HUP-S3.3 + S3.7: personas, tracks, workflows, persona voice

Canonical design and BDD: citrate-agent-runtime `agent-loop/PERSONAS.md` (one source of truth;
this note records what landed in core and what is still open).

## What landed

Runtime (agent-loop + agent-sidecar):

- `personas/personas.toml`: six personas (Builder, Auditor, Maker, Steward, Guide, Operator) in
  one data file. Each has voice, tone, style rules, default track and workflow, tool emphasis,
  skill allowlist, optional `tts_voice` (unset on all).
- `src/personas.rs`: validation, a deterministic prompt fragment, custom personas
  (`CustomPersona::check`).
- `tracks/workflows.toml` + `src/workflows.rs`: a verifier-judged workflow family per track
  (10 workflows), default first, built into real `agent-loop` workflows.
- Sidecar routes: `GET /personas`, `POST /personas/check`, `GET /workflows`.

Core:

- `src-tauri/src/hermes_personas.rs` (child module of `hermes`): commands `hermes_personas`,
  `hermes_workflows`, `hermes_persona_check` (async, `off_main`, in the main-window ACL).
- `src/agent/personas.ts`: prompt composition, custom-persona form validation, rename refresh.
- `src/components/PersonaPicker.tsx`: in Settings (App) and as an optional onboarding step.
- Store: `hermesPersona` (default `null`, the default voice) and `customPersonas`, both
  persisted. The fragment rides after the base prompt (sidecar loop) or as one leading system
  message (gateway and local agent paths, folded by `ai.rs` after the base prompt and context).

## Pending owner sign-off (safe defaults in place)

1. Persona names. Placeholders: Graft, Pith, Zest, Trellis, Sprout, Crew (first draft candidate
   per role that does not collide with an existing brand; Ledger and Hive skipped). Renaming is one
   line per persona in `personas.toml`.
2. Voice and style rules per persona: drafted from `09_PERSONAS_DRAFT.md`.
3. Guide and Operator default tracks: no dedicated track exists among the five; mapped to
   full-project / launch-checklist and project-management / status-note.
4. TTS voice per persona: none set; system voice.
5. Whether Operator ships at all (the draft marks it optional).

## Not done

- No session route runs a track workflow yet; tracks keep `workflow_available = false` and the
  picker says so.
- The code track's default workflow is checked by answer shape until a general test-runner tool
  ships.
- The persona's skill allowlist and tool emphasis are prompt text only; they do not filter the
  tool list.
