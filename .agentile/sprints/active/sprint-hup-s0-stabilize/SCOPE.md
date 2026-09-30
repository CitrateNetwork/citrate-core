---
created: 2026-09-30T00:00:00Z
branch: hup/s0-stabilize (PRs into release/0.5.0-hermes-upskill)
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S0
planset: 2026-09-30-hermes-upskill
release: 0.4.1
tier: T1
---

# Sprint HUP-S0: Stabilize → v0.4.1

The first sprint of the 0.4.x → 0.5.0 Hermes Upskill line
([planset](../../planset/2026-09-30-hermes-upskill/00_OVERVIEW.md),
[release plan](../../planset/2026-09-30-hermes-upskill/10_RELEASE_PLAN.md)). It
fixes the three owner-reported bugs (D-37) and consolidates the sidebar (D-34) before any
new agent capability lands. Root causes are in
[06_BUG_TRIAGE](../../planset/2026-09-30-hermes-upskill/06_BUG_TRIAGE.md).

## Work packages

Each WP is red → green → close-with-proof. WP definitions are in
[05_SPRINTS_AND_WPS §HUP-S0](../../planset/2026-09-30-hermes-upskill/05_SPRINTS_AND_WPS.md).

| WP | Summary | Stories | Status |
|---|---|---|---|
| S0.1 | Async commands + timeouts + `max_tokens`; widen the tripwire | US-0.1 | done (hardware QA pending) |
| S0.2 | Long commands exempt from the 12 s invoke deadline | US-0.1, US-0.2 | done |
| S0.3 | Download robustness (lock, idle-resume, 206, restart-resume, verify-not-redownload) | US-0.2 | done |
| S0.3b | HF token for gated repos (custody token reader; auth header only to huggingface.co, never forwarded to the CDN redirect) | US-0.2 AC5 | todo (T1 review) |
| S0.4 | GFM markdown renderer + golden tests; stop stripping `**` | US-0.3 | todo |
| S0.5 | llama-server `--jinja` / reasoning format / tier ctx; template-token guard | US-0.3 | todo |
| S0.6 | Selector subscriptions; drop the global tick; render isolation | US-0.1 | todo |
| S0.7 | Failed turns inline with Retry | US-0.3 | todo |
| S0.8 | Sidebar consolidation + routing/theme fixes | US-0.4 | todo |
| S0.9 | Write-tool approval audit (enumeration test) | gate1 g1-approval-audit | done (already on main) |

## Suggested order

S0.9 (smallest, safety) → S0.1 + S0.2 (the pinwheel) → S0.3 (downloads) → S0.4 + S0.5 +
S0.7 (text) → S0.6 (render perf) → S0.8 (sidebar).

## Exit (gate1 subset for 0.4.1)

`g1-approval-audit`, `g1-no-block`, `g1-downloads`, `g1-render`, `g1-sidebar` all
`met` with evidence in [EVIDENCE.md](EVIDENCE.md). Then the integration branch is PR'd to
`main`, and the owner merges and cuts v0.4.1.

## Test baseline (Rule 2)

Static counts at kickoff (`28a74ba`): **675** Rust `#[test]` (src-tauri + kit) and
**523** TS `it(`/`test(` cases. Exact `cargo test --workspace --locked` / `vitest` pass
counts are recorded in EVIDENCE.md on the first S0 run. Counts only go up.

## Daily log

- 2026-09-30: S0.3 done — segmented, self-resuming downloads + single-flight + restart Resume rows. HF gated-repo token split to S0.3b (custody read is a T1 change). cargo 676→684, vitest 540→543.
- 2026-09-30: S0.9 found already implemented (PBA-L7b-002). S0.1: 58 sync commands reached blocking I/O (not just the 4 chat commands) — all moved off the main thread; also fixed a stray `#[tauri::command]` on `comms::submit_claim`. S0.2 deadlines per command class. cargo 669→676, vitest 536→540.
- 2026-09-30: Sprint opened on `release/0.5.0-hermes-upskill`. Planset Stage-2
  (red-teamed) committed as the kickoff.
