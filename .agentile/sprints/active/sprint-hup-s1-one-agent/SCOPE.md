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
| S1.1a | `agent-loop` crate: types, `LlmClient`/`ToolHost` traits, bounded loop, stop, event stream — pure + offline-tested | rt | review (runtime#10) |
| S1.1b | Sidecar HTTP: sessions, long-poll events, tool_results, stop; core-hosted tool suspension | rt | review (runtime#10) |
| S1.1c | citrate-core: session start (endpoint + bearer from serve state), event long-poll, core tool host = existing `handleTool` gates; opt-in preview flag | core | review |
| S1.9 | Parity suite vs `harness.ts`; then retire the TS loop; one chat surface + one model picker (deferred from S0.8) | core, rt | review, parity half only (runtime#15 + core#124: 21-scenario fixture, sha-pinned in both repos; loop parity, not end-to-end; harness.ts NOT retired; process split not started) |
| S1.2 | Tool retrieval (BGE + keyword top-K) + tokenizer-true budget + compaction-must-shrink | rt | review (runtime#11; keyword selector — BGE retriever follows) |
| S1.3 | Planner/executor + verifier framework + TLA+ `AgentLoop` | rt | review (runtime#12; TLA+ in core #119) |
| S1.4 | Interviewer + editable brief | rt, core | review (rt: runtime#14; core: core#123 InterviewCard + hermes_tracks/brief_create/brief_check) |
| S1.6 | sizeup tiering at onboarding | core, sz | review (core#121) |
| S1.7 / S1.10 | Eval suite v1 (synthetic tool calls) + injection eval v1 | core | review (core#120) |
| S1.8 | `citrate hermes chat/run/status/stop` CLI | rt | review (runtime#13) |
| S1.5 | Escalation router (user endpoints + registry CID via x402) — blocked on S2.0 Rule-3 ADR + fed F-1/F-4 | rt, core | blocked |
| S1.11 | Wallet unlink (#117) — blocked on identity#31 deploy + fed #293 | kit, core | blocked |

### Pulled forward (groundwork, reversible)

Started early in the 2026-09-30 fan-out. None is wired into a session or the app; each is a
standalone crate, module or draft that can be dropped without touching S1 work.

| WP | Summary | Repo | Status |
|---|---|---|---|
| S2.0 | Rule-3 budgetable-signatures ADR (hardened SIWE, capped x402, no-funds anchor key) | core | review (core#122, status proposed, @rule8 sign-off block empty) |
| S2.2 | `citrate-agent-shell` allowlist runner (argv-only, scrubbed env, group kill) | rt | review (runtime#19; not wired; no OS sandbox) |
| S2.7 | Taint tracking + HIC downgrade in agent-loop, TLA+ `TaintDowngrade` | rt | review (runtime#16; core annotations + `hicAware` not done) |
| S2.8 | `citrate-agent-guard` default-deny path list | rt | review (runtime#18; not wired into callers) |
| S3.2 | SKILL.md loader + `skill_load` tool, off by default | rt | review (runtime#17; BGE per-turn retrieval not done) |

Retro: [RETRO-2026-09-30-fanout.md](RETRO-2026-09-30-fanout.md).

## Exit (gate1 remainder for 0.4.2)

`g1-loop`, `g1-eval`, `g1-injection` met with evidence; S1.11 merged; parity suite green.

## Test baseline (Rule 2)

citrate-core at `f4daf14` (v0.4.1): cargo 688 passed (6 ignored), vitest 568. Counts only go up.

## Daily log

- 2026-09-30 (late): runtime#20 (stacked on #14): tool_call events now name a host only for calls the loop dispatches; found by the S1.9 parity review; runtime 680→681. Merge before #16. Fan-out note: the shared target dir left stale binaries with removed-worktree paths (33 false capsule failures until agent-core was rebuilt).
- 2026-09-30 (fan-out, 7 parallel WPs, each with an adversarial review that pushed fixes): core#123 (S1.4 core) cargo 694→705, vitest 573→587; core#124 (S1.9 parity) vitest 573→598; core#122 (S2.0 ADR, doc only). Runtime from 680 on hup/s1-interviewer, per branch after review: #15 parity 685, #16 taint 699, #17 skills 714, #18 guard 718 (+1 review test; reviewer's worktree count 719 incl. 33 location-caused capsule failures), #19 shell 714. Counts are per branch, not additive; shared target dir means several were re-run after touching sources. One high-severity review finding on the loop/core dispatch seam is open (details held out of public text) and blocks confirming two parity verdicts. See retro.
- 2026-09-30: S1.4 runtime half (runtime#14): five bundled tracks + brief + sidecar /tracks, /briefs, /briefs/check + `hermes brief` CLI; runtime 666→680. S1.6 (core#121): hardware tier at onboarding, cargo 688→719 and vitest 568→590 on its branch; the tier does not drive serve.rs yet (waits on S1.7 model picks).
- 2026-09-30: S1.8 CLI (runtime#13, stacked on #12). An end-to-end smoke with the real sidecar binary + CLI + a scripted model passed after fixing a sidecar panic it exposed: the blocking HTTP client was built in an async handler, which poisoned the sessions lock. Regression test added; runtime 660→666.
- 2026-09-30: S1.1a+b (runtime#10: agent-loop crate + sidecar sessions, runtime 626→646) and S1.1c (core: 5 session commands + sidecar chat provider behind Settings › App preview toggle; cargo 688→694, vitest 568→573). Live use needs the hermes binary rebuilt from runtime#10.
- 2026-09-30: Sprint opened after v0.4.1 froze on `main`. ADR "loop in sidecar" accepted. S1.1a
  started in citrate-agent-runtime.
