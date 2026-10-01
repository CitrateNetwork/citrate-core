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

This suite does not retire `harness.ts` or change the default provider. Retirement is an owner
call after a live run of the sidecar loop.
