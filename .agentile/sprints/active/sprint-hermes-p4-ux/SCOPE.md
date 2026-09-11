---
created: 2026-09-11
branch: feat/hermes-p4-voice
author: Claude Fable 5
status: active
sprint: sprint-hermes-p4-ux
planset: 2026-09-11-hermes-agent
tier: T1
---

# Sprint — Hermes P4: Chat UX

Fourth phase. Voice input, journal integration, and streaming polish for the chat.

## Work packages
- **WP4.1 — Voice-to-text.** THIS SPRINT: a reusable on-device dictation helper
  (`src/agent/dictation.ts`) generalizing the journal's Web Speech loop, wired into the
  Dashboard coach chat with a Mic toggle + interim hint. On-device (Chrome/Edge); honest
  fallback ("typing works everywhere") where the API is absent (Rule 1). Higher-quality
  upgrade (follow-on): the whisper sidecar / gateway `POST /v1/audio/transcriptions`
  (OpenAI-compatible, confirmed live by the DGX team) — slots behind the same Mic UI once
  the sidecar is bundled / the gateway STT branch merges.
- **WP4.2 — Journal integration.** DONE. `journal_append` (write, confirm-gated) already
  existed; this sprint adds `journal_read` — a read-only agent tool over the local journal
  (index of pages + today's note, or a page by title fragment), with `@agent`/`@prompt`
  provenance tags and honest-empty output (never invents entries, Rule 1). Pure formatter
  `src/agent/journalRead.ts` (unit-tested); wired into `store.handleTool`; the tool-aware
  system prompt (`ai.rs`) now tells Hermes to read the journal to ground answers and to
  say a page is empty rather than fabricate.
- **WP4.3 — Streaming + auto-scroll.** DONE (v0.2.4): `createAgentProvider` streams tokens
  (`onToken`); the Dashboard near-bottom post-commit auto-scroll follows the stream.
  Verified this sprint.

## Dependencies / DGX
- Whisper sidecar built + live-tested; gateway `POST /v1/audio/transcriptions` on branch
  `feat/gateway-stt-transcriptions` (owner merge pending). Bundling the whisper sidecar
  binary + confirming the gateway base-URL wiring lights up the higher-quality path.
