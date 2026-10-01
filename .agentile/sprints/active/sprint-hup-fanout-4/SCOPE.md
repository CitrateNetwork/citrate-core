---
created: 2026-10-01T22:00:00Z
branch: docs/hup-fanout-4
author: Larry Klosowski + Claude Opus 5.5
status: active
type: sprint-scope
planset: 2026-09-30-hermes-upskill
release: 0.5.0 (D-41; no version bumps in this run)
---

# HUP fan-out 4: the 2026-10-01 day run

This file is the truth for the fourth parallel fan-out on the Hermes upskill (HUP) program (Rule 4).
The owner asked for it on 2026-10-01: "fan out and lets get the rest of this done ... finish whats
here today". For decisions that are his, he chose "build with safe defaults": full machinery,
conservative placeholder values marked "pending owner sign-off", and defaults that change nothing
for members.

Nothing in this file is merged. Every WP below is a branch with an open PR in a stack. The fan-out 3
stack (core #139 to #146 and docs #147) was merged into `release/0.5.0-hermes-upskill` before this
run started; the runtime fan-out 3 work is folded into runtime #23, which is still open.

## Shape of the run

- 26 work packages (some lanes carried two stories). Each lane had a builder agent and then an
  adversarial reviewer agent that re-ran the gates in a fresh worktree, broke guards on purpose, and
  pushed signed fix commits.
- Branches in five repos: `citrate-core` (26), `citrate-agent-runtime` (15), and one each in
  `citrate-memories`, `citrate-explorer` and `citrate-cluster`.
- Stacking lanes merged the reviewed branches into one chain per repo with signed merge commits,
  resolved conflicts (including two semantic ones, see the retro), ran the full gates once at the
  top, and opened each PR once.
- A red-team pass then read the stack tops. Counts only here: 0 Critical, 1 High, 13 Medium,
  25 Low, 1 Info. Details are in the private federation tracker (issue #298).
- Base commits: core `release/0.5.0-hermes-upskill` @ 8b88df5; runtime `hup/n3-folder-grants`
  (PR #23); memories, explorer and cluster `main`.

## Work packages

`Status` is the builder's `status_claim` followed by the review verdict. Tests are the numbers after
review fixes, per branch; they do not add up. The stack-top runs further down are the merged numbers.
"lib" means `cargo test --lib` in `src-tauri`; "workspace" means `cargo test --workspace`.

### citrate-core (stack: #151 → #176, base `release/0.5.0-hermes-upskill`)

| # | WP | Branch | PR | Status | Tests (after review) | Formal |
|---|---|---|---|---|---|---|
| 1 | S10.6 accessibility pass | `hup/n4-a11y` | [#151](https://github.com/CitrateNetwork/citrate-core/pull/151) | complete; pass-after-fixes | vitest 1003 → 1098 (axe-core in vitest) | none |
| 2 | S7.4 AgentSBT at onboarding | `hup/n4-agent-sbt` | [#152](https://github.com/CitrateNetwork/citrate-core/pull/152) | complete (anvil only); pass-after-fixes | lib 661 → 700; vitest 1023; opt-in anvil test | none |
| 3 | S2.5 capsule sandbox + pinned capsules | `hup/n4-capsule-sandbox` | [#153](https://github.com/CitrateNetwork/citrate-core/pull/153) | partial; pass-after-fixes | workspace 918; capsule_pins 9 | none |
| 4 | S10.5 recovery kit, delete local data, offline matrix, telemetry consent | `hup/n4-recovery-privacy` | [#154](https://github.com/CitrateNetwork/citrate-core/pull/154) | complete; pass-after-fixes | workspace 955; vitest 1037 | none |
| 5 | S5.2 + S5.3 web search settings, decide() | `hup/n4-search-decide` | [#155](https://github.com/CitrateNetwork/citrate-core/pull/155) | complete; pass-after-fixes | workspace 919; vitest 1015 | DecideEgress lives in runtime |
| 6 | S4.2 + S8.5 citrate-node MCP server | `hup/n4-node-mcp` | [#156](https://github.com/CitrateNetwork/citrate-core/pull/156) | partial; pass-after-fixes | lib 700; vitest 1015 | none |
| 7 | S7.3 + S7.5 anchor signer, metering report | `hup/n4-anchor-metering` | [#157](https://github.com/CitrateNetwork/citrate-core/pull/157) | partial; pass-after-fixes | kit 230, lib 679; vitest 1013 | TLC `AnchorSettle` 3,146 distinct; 7/7 + 1 reviewer mutant |
| 8 | S8.1 per-device keys + DeviceLink | `hup/n4-devicelink` | [#158](https://github.com/CitrateNetwork/citrate-core/pull/158) | partial; pass-after-fixes | lib 690 (+3 review); vitest 1014 | DeviceLink lives in cluster |
| 9 | S8.2 + S8.3 fleet wizard, Tailscale assist | `hup/n4-fleet-wizard` | [#159](https://github.com/CitrateNetwork/citrate-core/pull/159) | partial; pass-after-fixes | fleet 55 + 1 ignored; vitest 1038 | none |
| 10 | S4.4 user-added MCP servers | `hup/n4-user-mcp` | [#160](https://github.com/CitrateNetwork/citrate-core/pull/160) | complete; pass-after-fixes | lib 690; vitest 1017 | none |
| 11 | S3.1 knowledge corpus import | `hup/n4-corpus` | [#161](https://github.com/CitrateNetwork/citrate-core/pull/161) | partial; pass-after-fixes | knowledge_import 19 + 1 ignored live; vitest 1018 | none |
| 12 | S2.3 SIWE budgets | `hup/n4-siwe-budgets` | [#162](https://github.com/CitrateNetwork/citrate-core/pull/162) | partial; pass-after-fixes | workspace 971; vitest 1016 | TLC `WebSigningBudget` O3 23,191,496 and Siwe 5,077,204 distinct (re-run) |
| 13 | S3.4 verified learning, end to end | `hup/n4-learn-e2e` | [#163](https://github.com/CitrateNetwork/citrate-core/pull/163) | partial; pass-after-fixes | lib 674; vitest 1020 | LearnRestart lives in runtime |
| 14 | S1.9 process split (worker status) | `hup/n4-process-split` | [#164](https://github.com/CitrateNetwork/citrate-core/pull/164) | complete; pass-after-fixes | lib 665; vitest 1011 | none |
| 15 | S5.5 + S6.1 component updater, toolchain bundle | `hup/n4-components` | [#165](https://github.com/CitrateNetwork/citrate-core/pull/165) | complete (key slot empty); pass-after-fixes | workspace 981; vitest 1011 | TLC `ComponentSwap` 130 and 517 distinct; 9/9 + 1 reviewer mutant |
| 16 | S1.5 escalation router + spend budget | `hup/n4-escalation` | [#166](https://github.com/CitrateNetwork/citrate-core/pull/166) | partial; pass-after-fixes | escalation 39; vitest 1040 | TLC `SpendBudget` 6,926,616 and 243,840 distinct; 11/11 + 1 reviewer mutant |
| 17 | S3.3 + S3.7 personas, tracks | `hup/n4-personas` | [#167](https://github.com/CitrateNetwork/citrate-core/pull/167) | partial; pass-after-fixes | personas 7; vitest 1039 | none |
| 18 | S5.1 + S5.6 managed browser, attach to Chrome | `hup/n4-browser` | [#168](https://github.com/CitrateNetwork/citrate-core/pull/168) | complete; pass-after-fixes | lib 670; vitest 1034 | none (TaintDowngrade covers taint) |
| 19 | S2.1-wire folder grants end to end | `hup/n4-grants-e2e` | [#169](https://github.com/CitrateNetwork/citrate-core/pull/169) | complete; pass-after-fixes | workspace 926; vitest 1019 | `FolderGrant` re-run 153,484 distinct |
| 20 | S10.1 + S10.2 media, sheets, Google, schedule | `hup/n4-media-sheets` | [#170](https://github.com/CitrateNetwork/citrate-core/pull/170) | complete claimed; review: Sheets half of US-10.2 AC2 partial | workspace 964; vitest 1041 | none |
| 21 | S2.9-wire undo end to end | `hup/n4-undo-e2e` | [#171](https://github.com/CitrateNetwork/citrate-core/pull/171) | complete; pass-after-fixes | workspace 919; vitest 1034 | none |
| 22 | S6.6 + S6.7 after-deploy steps, contract reader | `hup/n4-postdeploy-reader` | [#172](https://github.com/CitrateNetwork/citrate-core/pull/172) | complete claimed; review: US-6.3 AC2 partial | lib 707; vitest 1051; anvil e2e script | none |
| 23 | S9.4 FL round planner + LoRA eval gate | `hup/n4-fl-rounds` | [#173](https://github.com/CitrateNetwork/citrate-core/pull/173) | partial; pass-after-fixes | fl_rounds 46; vitest 1040 | TLC `FlRoundGate` 881,600 distinct; 13/13 + 2 reviewer mutants |
| 24 | S4.3 mem-mcp + CitrateScan for Hermes | `hup/n4-mem-scan-mcp` | [#174](https://github.com/CitrateNetwork/citrate-core/pull/174) | complete; pass-after-fixes | lib 698 listed; vitest 1023 | none |
| 25 | S10.3 widgets + daemons | `hup/n4-widgets-daemons` | [#175](https://github.com/CitrateNetwork/citrate-core/pull/175) | complete; pass-after-fixes | workspace 958; vitest 1060 | TLC `DaemonBudget` 909,376 distinct; 10/10 + 2 reviewer mutants |
| 26 | S3.5-run eval scorecards (T0) | `hup/n4-scorecards` | [#176](https://github.com/CitrateNetwork/citrate-core/pull/176) | partial (T1 blocked); pass-after-fixes | vitest 1004 | none |

Merge recipe (in every PR body): bottom up, merge commits, into `release/0.5.0-hermes-upskill`.

### citrate-agent-runtime (stack: #30 → #44, base `hup/n3-folder-grants` = PR #23)

| WP | Branch | PR | Main runtime work |
|---|---|---|---|
| S2.5 | `hup/n4-capsule-sandbox` | [#30](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/30) | WASI preopens from live grants, socket allowlist, per-capsule verification |
| S3.3 + S3.7 | `hup/n4-personas` | [#31](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/31) | `personas.toml`, 10 track workflows, `/personas` routes |
| S4.4 | `hup/n4-user-mcp` | [#32](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/32) | user-entry validation, `POST /mcp/probe` dry run |
| S1.5 | `hup/n4-escalation` | [#33](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/33) | `agent-escalation` crate, `POST /escalations` |
| S7.3 + S7.5 | `hup/n4-anchor-metering` | [#34](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/34) | metering, trajectory and anchor wired into sessions |
| S10.3 | `hup/n4-widgets-daemons` | [#35](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/35) | `unattended` session flag (starts tainted) |
| S1.9 | `hup/n4-process-split` | [#36](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/36) | `agent-workers` supervisor; toolchain in a child process |
| S2.1-wire | `hup/n4-grants-e2e` | [#37](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/37) | grants on sessions, `file_list/read/write`, grants-driven toolchain gate |
| S10.1 + S10.2 | `hup/n4-media-sheets` | [#38](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/38) | `agent-office` crate, `sheet_read/write` |
| S2.9-wire | `hup/n4-undo-e2e` | [#39](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/39) | `fs_write/edit/delete/rename` with checkpoints, undo routes |
| S3.4-wire | `hup/n4-learn-e2e` | [#40](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/40) | workflow route, learn routes, `LearnRestart.tla` (183 / 473 distinct) |
| S5.2 + S5.3 | `hup/n4-search-decide` | [#41](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/41) | `agent-search`, `decide()`, `DecideEgress.tla` (37,648 / 1,807,120 states) |
| S5.1 + S5.6 | `hup/n4-browser` | [#42](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/42) | `agent-browser` crate over CDP, approval bound to the page version |
| S4.3 | `hup/n4-mem-scan-mcp` | [#43](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/43) | parity fixture + MCP host README |
| S9.4 | `hup/n4-fl-rounds` | [#44](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/44) | parity fixture |

Merge recipe (in every PR body): merge from the top down (#44 into #43, down to #30), then squash #30
into `main` after #23 lands with one re-approval.

### Single-PR repos

| Repo | WP | Branch | PR | Merge |
|---|---|---|---|---|
| citrate-memories | S3.1 `mem-corpus` + `mem-mcp import-corpus` | `hup/n4-corpus` | [#15](https://github.com/CitrateNetwork/citrate-memories/pull/15) | squash |
| citrate-explorer | S4.3 `/api/contract/{addr}/source` + MCP tool annotations | `hup/n4-mem-scan-mcp` | [#16](https://github.com/CitrateNetwork/citrate-explorer/pull/16) | squash, then deploy CitrateScan |
| citrate-cluster | S8.1 `device` module, DeviceLink admission, `DeviceLink.tla` (8,248 distinct) | `hup/n4-devicelink` | [#9](https://github.com/CitrateNetwork/citrate-cluster/pull/9) | squash |

## Stack-top gates

Run once per repo by the stacking lanes, on the top branch, after touching every changed crate.

| Repo | Top | Before (base) | After (top) | Other gates |
|---|---|---|---|---|
| citrate-core | `hup/n4-scorecards` @ d7badc8 | cargo 909; vitest 1003 | cargo `--workspace --locked` 1,666 passed, 0 failed; vitest 1,644 passed, 10 skipped | tsc clean; clippy 1.98.1 `-D warnings` clean (core, kit, components); TLC green on AnchorSettle, ComponentSwap (2 configs), FlRoundGate, DaemonBudget, SpendBudget (2 configs) |
| citrate-agent-runtime | `hup/n4-fl-rounds` @ bd7e984 | 1,212 | 1,647 passed, 3 ignored, **2 failed** (live headless-Chrome tests under load; both passed 3/3 when run alone) | clippy clean on the 12 changed crates; `cargo audit` unchanged from base; parity tests green |
| citrate-memories | `hup/n4-corpus` @ 26b6025 | 304 | 350; feature suites 94 | clippy clean; audit clean |
| citrate-explorer | `hup/n4-mem-scan-mcp` @ 42031d4 | 561 | 582 passed, 22 skipped | tsc, eslint, build, semgrep, CI ratchets clean; 2 high audit findings already on main |
| citrate-cluster | `hup/n4-devicelink` @ 2eca765 | 60 | 104 | fmt, clippy, release build clean; TLC DeviceLink 8,248 distinct |

The parity fixture is byte-identical in core and runtime after both stacks: sha256
`769a1809b656bfc1ab755926d3e41f4779521dfdff25bd2d63ec960cc889a71a`.

## Out of scope for this run

External or hardware work that no lane could do here: chain redeploy (F-4 / S7.1), precompiles
(S7.2), live FL rounds (S9.1, S9.2), cluster transport sign-off (CL-S4 / S8.4), faucet (S6.5), fork
mode (S6.10), per-OS clean installs (S11.1), Windows (S6.0), identity unlink (S1.11), release mirror
rows. DGX asks were posted on the sprint issues where a lane needed hardware.

## Tracking

Sprint issues in the private `citrate-federation` repo: S1 #278, S2 #279, S3 #280, S4 #281, S5 #282,
S6 #283, S7 #284, S8 #285, S9 #286, S10 #287, S11 #288, federation #289. Private findings from the
lanes and the red-team are kept in the private tracker only.

Retro: [RETRO-2026-10-01-day.md](RETRO-2026-10-01-day.md).
