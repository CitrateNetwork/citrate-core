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

## Follow-ons (see SCOPE)
- WP4.1 upgrade: whisper sidecar / gateway `POST /v1/audio/transcriptions` behind the same
  Mic UI (bundle the sidecar binary + merge the gateway STT branch).
- WP4.2 deeper journal integration (recall journal pages into context; ceremony-gated
  journal writes).

## Suite
- typecheck clean; `npx vitest run` full suite green (see PR).
