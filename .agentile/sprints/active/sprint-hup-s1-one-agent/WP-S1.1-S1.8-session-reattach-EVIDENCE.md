---
created: 2026-10-04
branch: hup/n6-session-reattach
author: Larry Klosowski + Claude Opus 5.5
status: review
---

# HUP-S1.1 / S1.8: one session from every client, reattach after reload, streamed text

Gates in scope: g1-loop ("Loop in sidecar; UI/CLI/MCP share a session") and the streaming half of
g1-render. `gates.yaml` is not edited here; the retro step decides the flips.

Branches: `hup/n6-session-reattach` in citrate-agent-runtime (base `hup/m2-runtime` @ 1181a51)
and citrate-core (base `hup/m2-core` @ 7581e88).

## What was built

| Item | Where | What it does |
|---|---|---|
| Streamed assistant text | runtime `agent-loop` (`Event::AssistantDelta`, `LlmClient::complete_streaming`, `DeltaCoalescer`), `agent-sidecar/src/llm_http.rs` (`StreamAssembler`, `read_streamed`) | The sidecar asks llama-server for `stream: true` (usage on the last chunk) and puts batched `assistant_delta` events on `/sessions/:id/events`. The `final` event is unchanged, so existing clients keep working. A server that ignores `stream` is read as one answer. A stream that stops early is an error, never a short answer. `CITRATE_HERMES_LLM_STREAM=0` turns streaming off. |
| `GET /sessions` | runtime `agent-sidecar` | The open sessions (id, model, busy, last seq, persona, waiting core calls). No prompt, history or keys. The events page also carries `pendingCoreCalls`. |
| `citrate-agent hermes run`, `sessions` | runtime `agent/cli/src/hermes_cmd.rs`, doc `agent/cli/HERMES_CLI.md` | `run <workflow> (--session <id> \| --model <m>) [--follow]` over `POST /sessions/:id/workflows` (a JSON file) or `/track_workflows` (a catalog id) and `GET /sessions/:id/workflows/:run`. Exit code 0 only for a verified run. `events --follow` prints streamed text once. The binary name `citrate-agent hermes` is recorded in the CLI doc. |
| Node MCP session tools | core `src-tauri/src/node_mcp_hermes.rs` | `hermes_session_list` and `hermes_session_events` (readOnlyHint; bounded long-poll, at most 8 s; at most 100 events per answer with `nextAfter`); `hermes_session_send` and `hermes_session_stop` (not read-only) queue a pending request in the approval inbox exactly like the other node MCP writes. Registration in `node_mcp_tools.rs` is one changed line plus `all_tools()`. |
| Saved session and reattach | core `src/agent/sidecarProvider.ts`, `src/shell/store.ts` | The provider saves `{id, lastSeq, inFlight}` (one localStorage entry, written at once; no prompt, no messages, no keys). After a reload, the first sidecar provider of the app load reads events from the saved seq. A core call the old view had started is closed with an honest "interrupted, outcome unknown" result and is never re-run; a core call announced after the saved point runs through the same gated handler; a session Hermes no longer has is let go with a plain notice. A turn run by the CLI or an MCP client while the view was idle is skipped, not taken as the next turn. |
| Streamed text in chat | core `src/agent/sidecarProvider.ts` | Deltas render as they arrive; the final event adds only what the stream has not shown. Text that led into a tool call stays and the answer starts a new paragraph. `harness.ts` behaviour and `ai.rs` (`stream: false` for the webview loop) are unchanged. |

## Proof run on this Mac (2026-10-04)

Real processes: the sidecar binary built from the runtime branch above (`citrate-agent-sidecar`,
debug build, loopback 127.0.0.1:19791, fresh bearer file), the app's bundled llama-server
(build 8640) serving `gemma-4-E4B-it-Q4_0.gguf` with `--jinja` on 127.0.0.1:18391, and the
public 40204 RPC for the one core-hosted tool (`chain_head`, `eth_blockNumber`). One session,
`s1-18db5b1e7d3f21a0`, driven by three clients:

1. **UI provider** (`src/agent/sidecarProvider.live.test.ts`, vitest, the production
   `createSidecarProvider` over HTTP). Transcript: [`evidence-n6-session/ui-provider.json`](evidence-n6-session/ui-provider.json).
   - Turn 1: the model called `chain_head`, the result came from the public RPC (height 132335),
     and the answer streamed as 7 `assistant_delta` events (seq 6 to 12) before `final` (seq 13).
     The text the view showed equals the final answer exactly.
   - Turn 2: view A was torn down while its `chain_head` call was in progress. Saved state at
     teardown: `lastSeq 16`, one call in flight.
   - View B (a new provider over the saved state) read from seq 16 on (its first event is 17),
     posted the interrupted result for the in-flight call, followed the turn to `done` (seq 32),
     and showed the answer once. No seq was seen twice; the call ran once in total (view A).
2. **CLI** (`citrate-agent hermes …` from the same branch). Transcript: [`evidence-n6-session/cli.txt`](evidence-n6-session/cli.txt).
   `sessions` lists the session; `events --after 14` replays turn 2; `send` plus
   `events --after 32 --follow` runs and prints a third turn (streamed, printed once);
   `run wf-say-ready.json --session … --follow` runs a workflow in the same session to
   `[say-ready: verified]` (both verifiers passed); `run hello-mint` is refused with HTTP 422
   naming the toolchain tools the session does not offer.
3. **MCP client** (`live_mcp_client_reads_the_shared_session`, an ignored test run by hand): a
   real HTTP MCP client against the node MCP server (connect token, `initialize`, MCP session),
   backed by core's Hermes client. Transcript: [`evidence-n6-session/mcp.json`](evidence-n6-session/mcp.json).
   `hermes_session_list` shows the session at seq 48; `hermes_session_events` returns all 48
   events of the three turns and the workflow with tool arguments and results withheld;
   `hermes_session_send` returns a pending request (`mcpr-1`) that waits for the member.

## Tests (red-green)

- runtime: `agent-loop/tests/stream_tests.rs` (5), `agent-sidecar/src/llm_stream_tests.rs` (11),
  `agent-sidecar/src/session_reattach_tests.rs` (4), `agent/cli/src/hermes_cmd_tests.rs` (+9).
- core Rust: `node_mcp_hermes_tests.rs` (14, plus the ignored live proof); `node_mcp_tests.rs`
  write-tool list extended by the two new write tools.
- core vitest: `sidecarProvider.reattach.test.ts` (18, including the teardown-mid-turn case and the
  stale-session case), `sidecarProvider.live.test.ts` (skipped without a live sidecar).
- The recorded-session parity fixture did not change: a model client that cannot stream emits no
  deltas, so the event stream is exactly as before.

## Owner decisions (nothing changes for members until decided)

- The task text says the `hermesSidecarLoop` default stays false; the owner's 2026-10-01 decision
  turned it on (on `hup/m2-core`). This branch does not touch the default either way.
- Turn cap 6 (harness.ts) vs 8 (sidecar default) stays open.
- Whether long-polled deltas (batched every 50 ms or 64 bytes) count as "real streaming" for
  g1-render. The webview loop (`ai.rs`, `stream: false`) is unchanged.
- Pending owner sign-off: over MCP, tool arguments and tool results in session events are withheld
  (they can carry personal memory, which the node MCP server does not expose). Final answers and
  streamed text are shown.

## Not done here

- Markdown golden tests (the other half of g1-render).
- A turn sent from the CLI or an MCP client into the app's session while the app view is open but
  idle: its core-hosted tool calls wait for the sidecar's deadline (the view only drives turns it
  started or picks up after a reload). The proof used tool-free prompts for those turns.
- The chat history itself is not saved across a reload (unchanged); a resumed turn appears as a
  new message.
