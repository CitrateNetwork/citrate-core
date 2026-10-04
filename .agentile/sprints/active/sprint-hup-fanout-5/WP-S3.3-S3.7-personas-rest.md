---
created: 2026-10-01
branch: hup/n5-personas-rest
author: Larry Klosowski + Claude Opus 5.5
status: implemented (persona names pending owner sign-off)
wp: HUP-S3.3 + HUP-S3.7 (rest)
story: US-3.3
gate: g2-personas (evidence except the owner's naming)
repos: citrate-agent-runtime (hup/n5-personas-rest), citrate-core (hup/n5-personas-rest)
previous: ../sprint-hup-fanout-4/WP-S3.3-S3.7-personas.md
---

# WP HUP-S3.3 + S3.7 (rest): personas applied end to end, track workflows from chat

Canonical design and BDD: citrate-agent-runtime `agent-loop/PERSONAS.md` (one source of truth).
This note records what fan-out 5 closed from the fan-out 4 not-done list and what is left.

## Fan-out 4 not-done list, item by item

| Item (fan-out 4) | Now |
|---|---|
| No session route runs a track workflow; tracks say `workflow_available = false` | Closed. Runtime `POST /sessions/:id/track_workflows {workflow}` runs a catalog workflow by id; every track says `workflow_available = true`. Core runs it from chat (`/run <workflow>`) and from a saved brief's card. |
| The persona's skill allowlist and tool emphasis are prompt text only | Closed. A sidecar session opened with a persona offers only the allowlisted skills that are installed, and pins up to four of its own emphasised tools into every request. Core sends the persona with the session. |
| `tts_voice` not wired | Closed as an option. "Read replies aloud" (Settings, off by default) speaks answers with the persona's voice through the system's speech engine, or the system voice when the id is unset or not installed. No shipped persona sets a voice (owner decision). |
| The code track's default workflow is checked by answer shape until a general test-runner tool ships | Open. No general test-runner tool exists; the Solidity variant (`solidity-red-green`) is judged by forge and slither. Building one is shell-sandbox scope (HUP-S2.2), not this WP. |
| Owner name picks | Open, by design (owner decision). Names stay placeholders in one data file. |

## What landed

Runtime (`hup/n5-personas-rest`, citrate-agent-runtime):

- `agent-loop`: `WorkflowSpec::required_tools`, `find_workflow`, `WorkflowView.needs_tools`;
  `SkillLibrary::restricted_to`; `SessionPersona` with `pinned_tools` and `session_persona` (shipped
  id or a checked custom persona, never both). Shipped allowlists now name real reviewed skills
  (citrate-core `skills.lock`, verdict `include-*`, plus the bundled `citrate-*` skills).
- `agent-sidecar`: `POST /sessions` takes `persona` or `customPersona` and answers what it did
  (`skills_offered`, `skills_missing`, `skills_restricted`, `pinned_tools`);
  `POST /sessions/:id/track_workflows` (404 unknown workflow, 422 with `missing_tools` when the
  session lacks a tool a pass needs); `GET /workflows` adds `needs_tools` and `unavailable`;
  `GET /personas` adds `skills_installed`.

Core (`hup/n5-personas-rest`, citrate-core):

- `hermes_session_open` takes the persona (shipped id or custom, bounded by core first); with none
  the session body is byte for byte unchanged.
- New command `hermes_track_workflow_run` (ids validated before any URL; refusals read
  `WORKFLOW_REFUSED: <reason>`), in the main-window ACL, 45 s invoke bound.
- `src/agent/sidecarProvider.ts`: one event pump for turns and workflow runs; a run ends only when
  the session is idle and the sidecar's run state is final (each step attempt has its own `done`).
  Core-hosted calls in a workflow go through the same gated `handleTool` as a turn.
- `src/agent/trackWorkflows.ts` (pure): `/run` parsing, the verdict wording ("Verified" only for a
  verified run), the persona-to-session mapping.
- `src/agent/speech.ts` (pure + the browser engine): voice choice, speakable text, `speakReply`.
- Store: `runTrackWorkflow`, `runBriefWorkflow`, `sidecarPersonaChoice`, `setHermesReadAloud`
  (`hermesReadAloud`, persisted, default false). The chat's brief card has "Run <workflow>".
- `PersonaPicker`: the read-aloud voice per persona, allowlisted skills installed, how to run each
  workflow and why one cannot run here; the "Read replies aloud" switch.

## BDD per track (US-3.3 AC2)

The Gherkin is in `agent-loop/PERSONAS.md`. Every scenario now runs twice: in agent-loop
(`tests/track_workflow_bdd_tests.rs`) and through the session route
(`agent-sidecar/src/track_workflow_route_tests.rs`, `track_<id>_...`), where all ten workflows of
the five families run: a satisfying model is verified, a model that only claims success is not,
contract workflows are judged by real forge, slither, aderyn and medusa reports (a High slither
finding fails the run), the project-management and full-project workflows fail when the model
writes to the journal or calls `contract_deploy` early, and core-hosted calls are answered as core
would answer them. The core half is covered by `src/agent/sidecarProvider.workflow.test.ts` and
`src/shell/trackWorkflow.store.test.ts`.

## g2-personas evidence (">= 5 personas (owner-named) + 5 tracks with workflows")

| Part | Evidence | Met |
|---|---|---|
| At least 5 personas, each with voice, tone, skill allowlist, default workflow | `persona_tests.rs` (6 ship); allowlists and emphasis applied in sessions (`persona_session_tests.rs`, `track_workflow_route_tests.rs`) | yes |
| Names chosen by the owner | placeholders, `name_status = "placeholder, pending owner sign-off"` | no (owner) |
| 5 tracks, each a workflow family and an interview | `track_workflow_bdd_tests.rs` (families of 2), `interview_tests.rs` | yes |
| Workflows runnable from chat | route + provider + store tests above | yes, behind the sidecar loop preview (off by default) |
| Custom personas (AC3) | `a_custom_persona_*` tests; custom personas also apply in sessions | yes |

The gate stays `met: false` in `gates.yaml` until the owner picks the names.

## Owner decisions (placeholders in place, pending owner sign-off)

1. Persona names (Graft, Pith, Zest, Trellis, Sprout, Crew are placeholders; rename = one line).
2. Voice, tone and style rules per persona (drafted from `09_PERSONAS_DRAFT.md`).
3. The skill allowlists (drafted from the reviewed corpus; which skills ship depends on S3.1).
4. Tool emphasis per persona (at most four pinned per session, `MAX_PINNED_EMPHASIS`).
5. TTS voice id per persona (none set: system voice).
6. Guide and Operator default tracks, and whether Operator ships.

## Not done

- Owner name picks (above).
- A general test-runner tool for the code track's default workflow.
- Workflows run only with the sidecar loop on (`hermesSidecarLoop`, default false, its own
  owner decision). With it off the chat says how to turn it on and runs nothing.
- Contract and hello-mint workflows need the contract toolchain (`CITRATE_HERMES_TOOLCHAIN`, off by
  default); they are refused with that reason until it is on.
- No packaged-app run: workflows from chat and reading aloud are proven by tests only (WKWebView
  `speechSynthesis` voices not checked by hand).
- The bundled sidecar in the app must be rebuilt from the merged runtime (A45) before core's new
  routes answer in a packaged build.

## Claim honesty

| Claim | Implemented | Wired | Runtime-proven |
|---|---|---|---|
| Persona skill allowlist + tool emphasis in sessions | yes | yes, in sidecar-loop sessions | route tests with a real skills library |
| Track workflows from chat and brief card | yes | yes, behind the sidecar loop preview | route tests (scripted model); provider and store tests |
| Read replies aloud | yes | yes, off by default | jsdom tests only |
