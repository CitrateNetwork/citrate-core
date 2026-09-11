---
created: 2026-09-11
author: Claude Fable 5
status: active
sprint: sprint-hermes-p4-ux
---

# Evidence — Hermes P4 (WP4.1 voice + WP4.3 verify)

## WP4.1 — voice-to-text (on-device)
- `src/agent/dictation.ts`: `createDictation()` over the Web Speech API (continuous +
  interim, auto-restart until stop); `dictationSupported()` honest probe; `appendFinal()`
  pure transcript-accumulation. Unsupported → a safe no-op controller (`supported:false`).
- `Dashboard.tsx`: a Mic toggle on the coach chat — finalized fragments append to the
  input, interim shows as the placeholder hint, permission-denied/absent → honest toast.
- Tests: `dictation.test.ts` 6 passed (appendFinal rules + honest unsupported probe in
  jsdom).

## WP4.3 — streaming + auto-scroll (verified, shipped v0.2.4)
- `harness.ts` `createAgentProvider` streams via `onToken`; `Dashboard.tsx` follows the
  stream with a near-bottom post-commit scroll (gated so a user reading history isn't
  yanked down).

## WP4.2 — journal integration (Hermes reads/writes the journal)
- `journal_append` (write, confirm-gated) already existed in `handleTool`.
- Added `journal_read` (read-only): `src/agent/journalRead.ts` `formatJournalForAgent(pages,
  query, today)` — index of pages + today's note, or a page by title fragment; strips
  `@agent`/`@prompt` markers to a provenance tag; honest-empty / no-match (Rule 1). Declared
  in `AGENT_TOOLS`, wired in `store.handleTool`, and the tool-aware system prompt (`ai.rs`)
  tells Hermes to read the journal to ground answers and never fabricate entries.
- Tests: `journalRead.test.ts` 6 passed; `cargo test --lib ai::` 21 passed (prompt change).

## Follow-on
- WP4.1 upgrade: whisper sidecar / gateway `POST /v1/audio/transcriptions` behind the same
  Mic UI (bundle the sidecar binary + merge the gateway STT branch).

## Suite
- typecheck clean; `npx vitest run` full suite green (see PR).
