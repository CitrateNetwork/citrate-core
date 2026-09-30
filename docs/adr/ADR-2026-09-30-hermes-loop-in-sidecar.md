---
created: 2026-09-30T00:00:00Z
branch: hup/s1-one-agent
author: Larry Klosowski + Claude Opus 5.5
status: accepted (implements planset D-8)
planset: 2026-09-30-hermes-upskill
wp: HUP-S1.1
---

# ADR — Hermes's agent loop moves into the sidecar ("brain in the sidecar, hands in core")

## Context

Planset decision D-8: the agent loop moves out of the webview (`src/agent/harness.ts`) into the
Rust sidecar (`citrate-agent-runtime/agent-sidecar`), so the app, the CLI, MCP clients, and
scheduled daemons all drive **one** agent, and the webview becomes a view. Today:

- The only LLM turn loop is TypeScript (`harness.ts`, ≤ 6 turns, tools executed by
  `store.handleTool` with the approval UI in-process).
- The sidecar has capsules, the ceremony-grade `ApprovalQueue`, e-stop and a loopback
  bearer-authed control plane — but no loop.
- Most useful tools need things only citrate-core holds: the SignatureCeremony, the custody vault,
  the comms/cluster/memory daemons it supervises, and the approval UI.

## Decision

1. **Brain in the sidecar.** A new pure crate `agent-loop` (in citrate-agent-runtime) owns the
   turn loop: messages, bounded steps, tool retrieval, verifier gates, spend/step budgets, stop,
   and a typed **event stream**. It depends on two injected traits — `LlmClient`
   (OpenAI-compatible chat completions) and `ToolHost` — so it is fully testable offline.
2. **Hands where the capability lives.** Every tool has a `host`:
   - `core` tools (memory, node, wallet/staking, groups/invites, journal, skills, models,
     contract_deploy, …) are executed **by citrate-core**: the loop emits `tool_call` and suspends
     that step; core runs it through its existing gates (`requestSig`, wallet review, the
     ceremony) and posts the result back (`POST /sessions/:id/tool_results`).
   - `sidecar` tools (capsules now; fs/shell/browser/toolchain in S2/S5/S6) execute in the sidecar
     under its own grants and `ApprovalQueue`.
   HIC gates therefore stay exactly where they are today; nothing about who approves changes.
3. **Keys.** The sidecar never holds a wallet key (Rule 3 unchanged). It receives, per session, an
   **inference endpoint + bearer** from core (the loopback llama-server key, or the sealed gateway
   key) over the existing loopback bearer channel. Endpoints are allow-listed by core (loopback or
   the configured https gateway); the webview never supplies them.
4. **Events.** The sidecar streams `plan | step_start | token | tool_call | tool_result |
   approval_needed | verifier | step_end | metering | error | done` over SSE
   (`GET /sessions/:id/events`). citrate-core relays them to the webview as Tauri events; the
   CLI and MCP read the same stream.
5. **Bounded and stoppable.** Every session has `max_steps`, a per-step deadline, and a stop flag
   checked between and during steps (TLA+ `AgentLoop`: `Bounded`, `StopIsLive`,
   `OnlyVerifierSucceeds`, `NoEffectWithoutGate`, `TaintDowngrade`).

> **Amendment (2026-09-30, S1.1b):** v1 serves events by **long-poll**
> (`GET /sessions/:id/events?after=N&wait_ms=M`, sequence-numbered, bounded replay log) rather than
> SSE: no extra stream dependencies, trivially testable, and core's Rust relay reads it with its
> existing HTTP client. SSE can sit on the same log later without changing clients' semantics.

## Consequences

- The webview keeps rendering and approving; it stops deciding. `harness.ts` is retired only after
  the S1.9 parity suite passes against the sidecar loop.
- A lost core connection suspends core-hosted tool calls (never silently skips them); the session
  can be resumed or stopped.
- Tool retrieval and context budgeting (S1.2) live in one place for every client.

## Alternatives rejected

- **Sidecar calls back into core for everything** (core as an RPC server): inverts the trust
  boundary and duplicates the ceremony bridge.
- **Give the sidecar the capabilities** (vault, daemons): breaks "no sidecar holds a key".
- **Keep the loop in TypeScript:** no CLI/MCP/daemon reuse (D-8).
