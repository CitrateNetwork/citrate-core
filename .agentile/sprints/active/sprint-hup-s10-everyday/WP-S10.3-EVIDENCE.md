---
created: 2026-10-01T12:45:00Z
branch: hup/n4-widgets-daemons
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S10
wp: HUP-S10.3
---

# HUP-S10.3 evidence: widgets (sandbox + bridge) and daemons (scheduler, budgets)

Spec: planset `04_FEATURES_BDD.md` US-10.3 (AC1 a Hermes-authored widget renders in a sandbox with
read-only data; AC2 a daemon runs on schedule within budget and reports to the monitor). Design:
[`docs/WIDGETS_AND_DAEMONS.md`](../../../../docs/WIDGETS_AND_DAEMONS.md). Branches
`hup/n4-widgets-daemons` in citrate-core (base `release/0.5.0-hermes-upskill`) and
citrate-agent-runtime (base `hup/n3-folder-grants`). The two must land together: the parity
fixture gains `widget_create` and its sha256 pin moves in both repos.

## Acceptance criteria

| AC | Where it is met | Proof |
|---|---|---|
| AC1 Hermes can author a widget | `widget_create` tool (harness + annotations + `store.handleTool`): the member sees the source and declared queries on an approval card, then it is saved | `agentToolGates` and `toolAnnotations` tripwires include it; eval sets carry a tool-choice task and an injection case for it |
| AC1 renders in a sandbox | `citrate-widget://` scheme (Rust `widgets.rs`) with its own CSP header + `sandbox allow-scripts`; iframe `sandbox="allow-scripts"` | `widgets_tests.rs` (CSP directives, main-window only, bad paths), `WidgetFrame.test.tsx` (sandbox flags) |
| AC1 with read-only data | bridge answers only declared catalog queries from app state; no other message type | `src/widgets/host.test.ts` (escape attempts) |
| AC2 runs on schedule | Rust `DaemonBook::claim_due` (local-time cron), runner ticks every 30 s | `daemons_tests.rs` schedule + claim tests, `runner.test.ts` |
| AC2 within budget | runs/day, tokens/day, tokens/run, spend 0; the runner stops a run at its allowance | budget-exhaustion tests (Rust + runner), TLA+ `DaemonBudget` |
| AC2 reports to the monitor | monitor snapshot `daemons` section, Pause/Resume and Stop over the pop-out bridge | `daemonsMonitor.test.tsx` |
| WP: HIC rules (anything needing a signature asks) | daemon turn marks every effectful call HIC-required; sidecar `unattended` sessions start in the HIC downgrade | `turn.test.ts`, runtime `daemon_session_tests.rs`, TLA+ `EffectsOnlyApproved` |
| WP: can be paused | per daemon and "pause all"; pausing stops a run in flight; resuming does not replay | Rust pause tests, runner pause test, TLA+ `NoStartWhilePaused` |

## Red to green

| Suite | Red observed | Green |
|---|---|---|
| runtime `agent-sidecar/src/daemon_session_tests.rs` (4) | E0425 (`UNATTENDED_TAINT_SOURCE` missing), then 3 failed against the unchanged session | 4 pass |
| `src/popout/daemonsMonitor.test.tsx` (8) | 8 failed (`daemonsSection` missing, no `daemons` in the snapshot) | 8 pass |
| `toolAnnotations.test.ts`, `agentToolGates.test.ts`, eval and parity tripwires | 7 failed when `widget_create` joined `AGENT_TOOLS` before its gate, annotations and datasets | pass |
| `daemons_tests.rs` (31), `widgets_tests.rs` (14), `hermes_daemon_tests.rs` (4), `src/widgets/host.test.ts` (11), `src/daemons/runner.test.ts` (11), `src/daemons/turn.test.ts` (9), `src/daemons/panels.test.tsx` (10), `src/widgets/WidgetFrame.test.tsx` (3) | written before their modules but first run against complete modules, so no separate red run; each guard was instead mutation-checked (below) | pass |

## Mutation checks (break a guard, see a test fail, restore)

Rust (`daemons.rs`, `widgets.rs`), 15 killed: run cap, token cap, per-daemon pause, 24 h
catch-up window, stale-run charge, spend must be "0", run allowance capped by what is left,
one run in flight (the whole guard), resume does not replay (one and all), widget CSP
`connect-src`, CSP `sandbox`, main-window-only loading, catalog-only queries. The first pass left
two survivors: removing only the `continue` of the in-flight branch (equivalent: the same branch
already advances the anchor) and the resume anchor (no test resumed without a tick in between).
A new test, `resuming_without_any_tick_while_paused_does_not_replay`, kills both resume mutants;
the in-flight mutant was widened to the whole guard and is killed.

TypeScript, 14 killed (+1 equivalent control): host source check, declared check, catalog check,
rate limit, no wallet address in `wallet.summary`, iframe sandbox flags, HIC-required for
effectful daemon calls, no `app_navigate`, local model only, sidecar session closed, runner stops
at the token allowance, `canRun` gate, pause stops the run, bridge daemon-id check.

## Formal

`src-tauri/formal/DaemonBudget.tla` + `.cfg` + `_mutants.py`. TLC2 2.19, 2026-10-01:
**no error**, 11,655,489 states generated, 909,376 distinct, depth 23. Mutation check: 10 of 10
killed, every invariant covered (`RunsWithinCap`, `TokensBounded`, `AllowanceWithinDay`,
`NoEmptyRun`, `SpendZero`, `OneInFlight`, `NoStartWhilePaused`, `EffectsOnlyApproved`). Details
and abstractions in `src-tauri/formal/README.md`.

## Gates (this branch)

| Gate | Result |
|---|---|
| `npx tsc --noEmit` | clean |
| `npx vitest run` | 117 files, 1055 passed, 9 skipped (52 new) |
| `cargo test --workspace` (core) | 958 passed, 0 failed, 7 ignored (+49 new Rust tests) |
| `cargo clippy --no-deps -p citrate-core --all-targets -D warnings` (1.98.1) | clean |
| main-thread tripwire, pop-out ACL resolver tests | pass (every new command is async and in `main-window.toml`) |
| runtime `cargo test --workspace` | 1216 passed, 0 failed (+4) |
| runtime `cargo clippy --no-deps -p agent-sidecar --all-targets -D warnings` | clean |

## Not done

- No click-through in the packaged app on any OS (scheme handler, CSP header honoured by
  WKWebView / WebKitGTK / WebView2, iframe sandbox, daemon runs with the real local model).
- Token use is estimated (characters / 4), not measured.
- Daemon runs are not written to the journal or the metering log.
- Widget query counts are not shown in the Activity monitor.
- The budget defaults and the 10-minute run limit are placeholders pending owner sign-off.
