---
created: 2026-09-30
branch: hup/s1-parity
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Hermes loop parity suite (HUP-S1.9)

`parity-v1.json` is the versioned contract between the TypeScript agent loop
(`../harness.ts`, `createAgentProvider`) and the sidecar loop in citrate-agent-runtime
(`agent-loop` `run_turn`, fed by `agent-sidecar` `llm_http::parse_turn`). Each scenario is a
scripted model + scripted tool host + the expected outcome, statuses/events, host calls and the
transcript the model sees.

| Implementation | Runner | Scenarios |
|---|---|---|
| `ts` | `src/agent/parity/parity.test.ts` (this repo) | all |
| `loop` | citrate-agent-runtime `agent-loop/tests/parity_tests.rs` | `layer: "loop"` |
| `sidecar` | citrate-agent-runtime `agent-sidecar/tests/parity_wire_tests.rs` | all |

The same bytes live at citrate-agent-runtime `agent-loop/tests/fixtures/parity-v1.json`; both
repos pin the file's sha256, so an edit on one side fails the other until both are bumped.
To change the contract: edit the fixture, copy it byte-for-byte to the other repo, update
`PARITY_V1_SHA256` in both runners, and bump `version` (and the file name) when an existing
expectation changes meaning.

## What a known divergence means

A scenario with `known_divergence` carries per-implementation overrides and a `verdict`.
`rust_correct` means the sidecar behavior is the intended one and the TS behavior goes away when
`harness.ts` is retired. v1 records seven: empty reply (sidecar fails honestly), malformed
arguments, unknown tool, nameless tool call, object arguments (sidecar refuses or normalizes;
TS dispatches and core's handler hides the mistake), the per-reply tool bound, and stop (the TS
loop has none). `config_divergences` lists wiring differences outside the loop (default turn cap,
live-context freshness, cross-turn history) that need an owner decision.

## Scope

The `loop` and `sidecar` runners exercise the wire parser and `run_turn`. They do not exercise
the sidecar session layer (its own config, such as the default `max_steps`, and the core-host
`tool_results` round trip) or this repo's `../sidecarProvider.ts`, which turns the loop's events
into calls on the store's gated handlers. A green run is loop parity, not end-to-end parity of the
live core + sidecar path; that needs its own test before `harness.ts` is retired.

This suite does not retire `harness.ts` or change the default provider. Retirement is an owner
call after a live run of the sidecar loop.

## Live run (end to end, HUP-S1.9)

`live/` runs the same scenarios end to end: this repo's `../sidecarProvider.ts` drives a real
sidecar process (the `hermes` binary inside a packaged `.app`) over its HTTP control API, and the
session's model is a scripted OpenAI-compatible endpoint (`scripts/parity-live/`) on a real socket.
It covers what the loop runners above cannot: the session layer's own config, the `tool_results`
round trip, the event log the provider long-polls, and the model client's HTTP behaviour.

```sh
npx tauri build --config src-tauri/tauri.bundle-lite.conf.json --no-sign --bundles app
scripts/parity-live.sh                # newest .app under the cargo target dir; or --app / --bin
```

The session body is built from `live/session-config.json`, which `hermes_live_parity_tests.rs`
pins to `build_session_body`, so the run cannot drift from what the app sends. Two adaptations,
both in the report: cap-bound expectations use the live step cap (core sends no `maxSteps`, so the
sidecar default of 8 applies, versus 6 in `harness.ts`: the `default_turn_cap` owner call), and a
scripted model error is checked as its HTTP status, because the sidecar (like core's own `ai.rs`
client) reports only the status of a failed model call, never its body.

`live/liveModel.live.test.ts` is a behaviour record on a real local model (set
`CITRATE_PARITY_LIVE_LLM` to a running llama-server): steps used, tools chosen, honest endings.

One more wiring difference the live run made visible, outside the loop: the local `harness.ts`
path sends Rust's `AGENT_SYSTEM_PROMPT_TOOLS` (src-tauri/src/ai.rs), while the sidecar path sends
this repo's `AGENT_SYSTEM_PROMPT` (the prompt the tool-call eval in `../eval/` scores). Retiring
`harness.ts` therefore also retires the Rust prompt for local chat.
