---
created: 2026-10-04
branch: hup/n6-everyday-monitor
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S10 (+ HUP-S7.6)
wp: HUP-S10.2, HUP-S10.3, HUP-S10.4, HUP-S10.5, HUP-S10.6, HUP-S7.6 (US-7.4 AC1)
---

# Fan-out 6, lane N5-everyday: evidence

Base: citrate-core `hup/m2-core`, citrate-agent-runtime `hup/m2-runtime`. Branch
`hup/n6-everyday-monitor` in both repos. Gap list: `verify.M6` and `verify.M5` (S7.6, US-7.4) in
`handoffs/HUP_FANOUT5_PARTIAL_2026-10-01.json`.

## What was built

| Gap | Change | Tests |
|---|---|---|
| US-10.2 AC2: Sheets had commands only, no view, no Hermes tool | Journal > Sheets view (`src/agent/sheets/SheetsPanel.tsx`): read a range, add rows (stored as typed). Hermes tools `gsheets_read` (read, no approval), `gsheets_append` (write, HIC-1 approval card), `schedule_list`, `schedule_add` (approval card), `calendar_list` (`src/agent/everydayTools.ts`). Honest "not set up / not connected" from `google_workspace_status`; shared-sheet cells and invitation titles are fenced as untrusted. Annotations, tripwire, parity fixture (both repos, sha bumped), eval fragments `toolcall-v2.d/everyday.json` and `injection-v2.d/everyday.json` | `everydayTools.test.ts` (21), `sheets.test.tsx` (6), `everydayEval.test.ts` (3), tripwire and annotation suites drive the new tools |
| US-10.3 AC2: daemon runs not metered, tokens chars/4 | Tokens measured from the model server's usage (core `citrate_usage` on the in-app loop, the sidecar's new `usage` event) when every call reports it, otherwise estimated and labelled. Each finished run is appended to `<app data>/hermes/metering/daemon-runs.jsonl` (core is its only writer; no reply text) and written as an `@daemon` bullet into its day's journal entry. Monitor and Daemons card say "last run measured" or "estimated" | `measured.test.ts` (9), Rust `daemons_tests` run log (6) |
| US-7.4 AC1: monitor showed no plan, approvals, verifier results, tokens/s, ctx used | Runtime: `Event::Usage {step, prompt_tokens, completion_tokens, generation_ms}` and `Event::Plan {steps}`; `TokenUsage.generation_ms` from llama-server `timings.predicted_ms`. Core: `ai.rs` attaches `citrate_usage` to the returned message (never sent back to the model); the turn activity slice records usage, plan, approvals (store approval cards and the sidecar's held commands, pending then decided) and verifier verdicts; the monitor shows Speed (tokens/s), Context used (prompt + written), Plan with step states, Approvals and Checks. Stop stays first | `ActivityMonitor.usage.test.tsx` (14), `approvalActivity.test.ts` (2), Rust `ai_tests` (3), runtime `monitor_events_tests` (4), `metering_session_tests` (2) |
| US-10.4 AC1: daily entry read only @agent bullets and wallet rows | `dailyEntry.ts` merges the sidecar's daily metering (sessions, turns, outcomes, checks, reported tokens, top tools, HIC count), the daemon run log, and the most recent personal-memory facts (labelled undated: the memory store keeps no dates), each with an honest line when empty or unreadable. Optional once-a-day trigger on the daemon runner tick, behind the Journal "Daily summary" setting, OFF by default, at or after 23:00 UTC (journal days are UTC days; placeholder) | `daySources.test.ts` (9), `autoSummary.test.ts` (4) |
| S10.5: node-sync and messaging rows not probed | `node_sync_tip_unreachable` (the node status's network-tip read against a closed port, through the extracted `sync_progress`: unknown, never synced) and `messaging_relay_unreachable` (a member-daemon manager pointed at a closed relay port, no daemon: relay not connected, a list-groups request fails plainly, no bearer echo). Unprobed pin now 0 | `privacy_contract_tests` |
| S10.6: no real-browser axe pass | `scripts/a11y-browser.mjs` + `scripts/a11y-browser/scenes.tsx`: Chromium 153, axe 4.13, colour-contrast on, 20 scenes at 100% and 200% zoom: 0 findings. Follow-up audit `docs/A11Y_AUDIT_HUP_S10_6_2026-10-04.md` (the 2026-10-01 audit is not edited) | negative control: the pre-S10.6 `--warn` fails at 3.28:1 |

## Red, green, mutation

Most suites were written in the same step as the code, so their red runs were not separately
recorded. Mutation checks (break the code, see the named suite fail, restore) were run instead:

| Mutant | Killed by |
|---|---|
| in-app loop does not report usage | `ActivityMonitor.usage.test.tsx` |
| sidecar provider skips the pending approval | `ActivityMonitor.usage.test.tsx` |
| meter calls a run measured when any call reported | `measured.test.ts` |
| store does not record a pending approval | `approvalActivity.test.ts` |
| `gsheets_append` runs after a decline | `everydayTools.test.ts` |
| sheet cells reach the model unfenced | `everydayTools.test.ts` |
| automatic summary ignores its off switch | `autoSummary.test.ts` |
| daily entry drops the new sources | `daySources.test.ts` |
| context used stays null with a usage report | `ActivityMonitor.usage.test.tsx` |
| runner drops the token source | `measured.test.ts` |
| `sync_progress` reports 100 with no tip (Rust) | `privacy_contract_tests::each_feature_degrades_honestly_with_no_network` |

## Gates run (2026-10-04)

- core: `cargo fmt --all -- --check` clean; `cargo clippy --no-deps -p citrate-core --all-targets -D warnings` (1.98.1) clean; `cargo test -p citrate-core` 1364 passed, 10 ignored; `npx tsc --noEmit` clean; `npx vitest run` 211 files, 1907 passed, 10 skipped.
- runtime: clippy (1.98.1) clean on citrate-agent-loop, citrate-agent-metering, agent-sidecar, citrate-agent-trajectory; their tests 637 passed, 0 failed; rustfmt clean on the files changed here.
- `scripts/ci/check-release-pins.sh` fails on `hup/m2-core` without this branch's changes too (no file it reads was touched here).

## Not done (and why)

- **Packaged-app click-through** (widgets under `citrate-widget://` CSP, a daemon with Pause/Stop
  in the monitor, journal save/open dialogs, recovery kit against the real keychain): no bundle
  built from this branch exists on this Mac, and the click-through needs a person at the GUI.
- **Live Google run**: needs the owner's Google OAuth client id (`GOOGLE_CLIENT_ID` /
  `GOOGLE_CLIENT_SECRET`) and a connected account. No refresh-token flow yet.
- **VoiceOver / NVDA passes**: human; exact steps are in the follow-up audit.
- **Linux WebKitGTK / Windows WebView2 runs**: external (DGX team).
- **Chat-turn plans**: only workflow runs announce a plan; an ordinary chat turn says it has none.
- **Merge note for the eval-v2 lane** (`hup/n6-eval-v2`): it freezes v1 and checks tool coverage on
  v2. Its `datasets.test.ts` and `runner.test.ts` replace this branch's versions on merge; the two
  everyday fragments then load through its fragment loader, and `AGENT_TOOLS.length` becomes 28
  with `belnap_codec`. The parity fixture gains five tools here and one there: merge by hand and
  re-pin the sha256 in both repos.
