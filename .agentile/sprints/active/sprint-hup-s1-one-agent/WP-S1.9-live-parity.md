---
created: 2026-10-04
branch: hup/n6-split-proof
author: Larry Klosowski + Claude Opus 5.5
status: review
---

# HUP-S1.9 (finish): live parity against the packaged app

The parity suite (#124) proved the TypeScript loop, the Rust loop and the sidecar's wire parser
against the same 21 scenarios. It did not run the sidecar process or `sidecarProvider.ts`, so it
was loop parity, not end-to-end parity. This WP runs it end to end against a packaged build.

## What the live run does

`src/agent/parity/live.test.ts` drives every parity-v1 scenario through:

```text
createSidecarProvider (the chat view the app uses)
  -> HTTP control API (the routes and bodies of src-tauri/src/hermes.rs HermesManager)
    -> Citrate Core.app/Contents/MacOS/hermes (the bundled sidecar)
      -> a scripted OpenAI-compatible model server on loopback (one per scenario)
```

- Sessions open with the body `build_session_body` makes. `session-body-v1.json` pins that body,
  and the Rust test `build_session_body_matches_the_live_parity_fixture` checks the same bytes, so
  the live runner cannot drift from what the app sends. Tools are the app's
  `annotatedAgentTools()`. No `maxSteps` is sent, as in the app.
- Earlier conversation turns (`history`) are played as real turns of the same session.
- Stop goes through the provider's abort signal, the way the chat's Stop button does.
- A second live test runs the process split on the same binary: the toolchain worker is its own
  process, `kill -9` of it is reported (`restarts: 1`, `killed by signal 9`) and restarted, the
  control plane keeps answering, and the browser entry reads `not_built`.
- `scripts/hermes-live-parity.sh "<app>" results.json` runs both against a built app. Without
  `CITRATE_HERMES_LIVE_BIN` the live tests are skipped and the body, scripted-model, control-API and
  override checks still run (7 always-on tests).

## The packaged build

- Built 2026-10-04 on this Mac: `npx tauri build --config src-tauri/tauri.bundle-lite.conf.json`
  plus an overlay that turns off signing, updater artifacts and the DMG (`targets: ["app"]`).
- Core: `hup/n6-split-proof` (base `hup/m2-core` @ 7581e88). Sidecar: rebuilt from
  `hup/n6-split-proof` in citrate-agent-runtime @ 543209d (base `hup/m2-runtime` @ 1181a51),
  `cargo build --release --locked -p agent-sidecar`, bundled as `binaries/hermes`.
  Other sidecars were the existing local binaries (provenance files beside them).
- Bundled sidecar sha256 `c7144115f1114a048dbec70452c57342aaf7ddac5e681af1011f329730a4fc4b`
  (ad hoc, linker signed). The bundle output was deleted after recording the sizes below.

| Part | Size (KiB) |
|---|---|
| Citrate Core.app | 725,156 |
| Contents/MacOS (all executables) | 238,156 |
| citrate-core (app) | 34,784 |
| hermes (sidecar) | 28,624 |
| citrate (node) | 51,932 |
| ipfs | 84,464 |
| mem-mcp | 17,080 |
| comms-member-daemon | 11,236 |
| node-agent | 5,432 |
| cluster-daemon | 4,552 |
| llama-server (launcher) | 52 |
| Contents/Resources | 486,996 |
| models (bge-base-en-v1.5) | 428,392 |
| llama (dylibs) | 58,320 |
| docs-corpus / capsules / icon | 56 / 52 / 176 |

## Results

Recorded in `src/agent/parity/results/2026-10-04-live-packaged.json`: **21 of 21 scenarios pass;
the process-split check passes** (toolchain pid killed with SIGKILL, restarted as a new pid,
`restarts 1`, `last exit "killed by signal 9"`). The recorded JSON says "/health 200 throughout";
in that run the control plane answered `/workers` with 200 on every poll during the restart and
`/health` with 200 once after it. Since review, the test checks `/health` on every poll as well, and
it still passes on the same binary (sha256 `c7144115...a4fc4b`, rebuilt reproducibly by the reviewer).

Live overrides, each with its cause (the test refuses an override without one):

| Scenario | Live expectation | Cause |
|---|---|---|
| `turn_cap_exhausted` | 8 model requests, `step budget of 8 exhausted` | `config:default_turn_cap`: the app opens sessions without `maxSteps`, so the sidecar default of 8 applies; harness.ts stops at 6. Owner decision. |
| `provider_error`, `provider_error_after_tool` | error names `HTTP 503` | `transport`: over real HTTP the sidecar's client reports the provider's HTTP status, not its text |

Every other scenario matches the sidecar column of parity-v1 exactly, including the seven
`rust_correct` divergences (empty reply, malformed and object arguments, unknown and nameless
tool calls, the per-reply tool bound, stop).

Mutation check of the live runner (each mutant run against the bundled binary, all killed):
turn cap set to 6 (1 fail), transport override dropped (1 fail), Stop never pressed (1 fail),
`maxSteps: 6` added to the body (3 fail), sidecar overrides ignored (1 fail).

## Parity gaps found and fixed

1. **Chat header showed the sidecar loop as the demo agent.** `reflectProvider` mapped any
   provider kind other than local/real/agent to `demo`, so with the sidecar loop on by default
   the header showed the yellow demo dot and the label "Hermes (sidecar loop · preview)". The
   sidecar loop now maps to `local` (it runs on the local model) and its label is "local model ·
   Hermes sidecar · agentic", next to the app loop's "local model · llama-server · agentic".
   Test: `src/shell/chatProviderKind.test.ts` (red first).
2. **The workers report said there are no browser tools.** The bundled sidecar's `GET /workers`
   browser entry read "no browser tools exist yet". The HUP-S5.1 browser tools run in the sidecar
   and drive the managed Chromium as its own process; only a separate browser worker is not
   built. Fixed in citrate-agent-runtime (543209d); the live process-split check failed on the
   first packaged build for exactly this and passes on the second.

## Proof

- core vitest: 1763 passed, 32 skipped, measured against 1754 passed, 10 skipped on `hup/m2-core`
  @ 7581e88. The 22 new skipped tests are the live ones (21 scenarios plus the process split),
  which run only with a binary; with the bundled binary all 29 tests in `live.test.ts` pass. The
  new always-on tests are 7 live-support tests and 2 header tests (8 live-support after review: the
  reply cap on a small window). `npx tsc --noEmit` clean.
- core cargo: `hermes::` 106 passed, including `build_session_body_matches_the_live_parity_fixture`.
- runtime: `process_split_tests` 13 passed (the report test is stricter; red on the old text).

## Not done / owner calls

- The turn cap (6 in harness.ts vs 8 sidecar default) stays an owner call, pending owner
  sign-off. It is the only scenario where the shipped path and harness.ts disagree on a
  configured value. When it is decided, `LIVE_SESSION_MAX_STEPS` follows it.
- `harness.ts` stays. It is still the fallback loop when the sidecar is down.
- `live_context_freshness` (sidecar sessions see the session-start context snapshot) is still an
  owner decision; the live run does not change it.
- The live run drives the packaged sidecar binary, not the GUI process: the app itself was not
  launched (it would start the node and use this Mac's real app data). Core's spawn of the
  sidecar is covered by the existing supervisor tests, and the control calls are mirrored from
  `hermes.rs` and pinned by the body fixture.
- g1-loop is still not met: no MCP client can reach a Hermes session yet, and a webview reload
  does not reattach to a running session (see gates.yaml note).
