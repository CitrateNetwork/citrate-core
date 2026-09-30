---
created: 2026-09-30T00:00:00Z
branch: hup/s1-one-agent (PRs into release/0.5.0-hermes-upskill); runtime work on citrate-agent-runtime hup/s1-agent-loop
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S1
planset: 2026-09-30-hermes-upskill
release: 0.4.2
tier: T1
---

# Sprint HUP-S1 — One agent → v0.4.2

Hermes's loop moves into the sidecar ([ADR](../../../docs/adr/ADR-2026-09-30-hermes-loop-in-sidecar.md)),
so app, CLI, MCP and daemons drive one agent; plus tiering, escalation, eval, and the wallet-unlink
carry-in. WP definitions: [05 §HUP-S1](../../planset/2026-09-30-hermes-upskill/05_SPRINTS_AND_WPS.md).

## Work packages (sequenced)

| WP | Summary | Repo | Status |
|---|---|---|---|
| S1.1a | `agent-loop` crate: types, `LlmClient`/`ToolHost` traits, bounded loop, stop, event stream — pure + offline-tested | rt | in progress |
| S1.1b | Sidecar HTTP: sessions, SSE events, tool_results, stop; core-hosted tool suspension | rt | todo |
| S1.1c | citrate-core: session start (endpoint + bearer from serve/gateway state), SSE relay to webview, core tool host = existing `handleTool` gates | core | todo |
| S1.9 | Parity suite vs `harness.ts`; then retire the TS loop; one chat surface + one model picker (deferred from S0.8) | core, rt | todo |
| S1.2 | Tool retrieval (BGE + keyword top-K) + tokenizer-true budget + compaction-must-shrink | rt | todo |
| S1.3 | Planner/executor + verifier framework + TLA+ `AgentLoop` | rt | todo |
| S1.4 | Interviewer + editable brief | rt, core | todo |
| S1.6 | sizeup tiering at onboarding | core, sz | todo |
| S1.7 / S1.10 | Eval suite v1 (synthetic tool calls) + injection eval v1 | core | todo |
| S1.8 | `citrate hermes chat/run/status/stop` CLI | rt | todo |
| S1.5 | Escalation router (user endpoints + registry CID via x402) — blocked on S2.0 Rule-3 ADR + fed F-1/F-4 | rt, core | blocked |
| S1.11 | Wallet unlink (#117) — blocked on identity#31 deploy + fed #293 | kit, core | blocked |

## Exit (gate1 remainder for 0.4.2)

`g1-loop`, `g1-eval`, `g1-injection` met with evidence; S1.11 merged; parity suite green.

## Test baseline (Rule 2)

citrate-core at `f4daf14` (v0.4.1): cargo 688 passed (6 ignored), vitest 568. Counts only go up.

## Daily log

- 2026-09-30: Sprint opened after v0.4.1 froze on `main`. ADR "loop in sidecar" accepted. S1.1a
  started in citrate-agent-runtime.
