---
created: 2026-10-04T20:05:00Z
branch: docs/hup-fanout-6
author: Larry Klosowski + Claude Opus 5.5
status: active
type: sprint-scope
planset: 2026-09-30-hermes-upskill
release: 0.5.0 (D-41; no version bumps in this run)
sprint_issues: CitrateNetwork/citrate-federation#278 to #289
---

# HUP fan-out 6: the 2026-10-04 run

This file is the truth for the sixth parallel fan-out on the Hermes upskill (HUP) program (Rule 4).
The owner asked for it on 2026-10-04: "ok lets fan back out again and knock out as much of this as
we can." Earlier he had chosen how agents treat his calls: build the full machinery, put conservative
placeholder values marked "pending owner sign-off" wherever a decision is his, and keep defaults
that change nothing for members. For chain work, agents prepare PRs (code, scripts, tests, anvil
dry runs, activation-height gated, default off) and never deploy, sign, broadcast or activate.
Milestones are finished one at a time, M2 first.

Nothing in this file is merged. Every work package below is a branch with an open PR. The retro is
[RETRO-2026-10-04.md](RETRO-2026-10-04.md) and the per-item milestone view is
[MILESTONE_STATUS.md](MILESTONE_STATUS.md). M2's own re-verification lives in
[../sprint-hup-m2/M2_STATUS.md](../sprint-hup-m2/M2_STATUS.md) and is linked, not copied.

I am the documentation lane. I did not re-run any test suite. Every number here comes from a builder
report, a reviewer report or a stacking lane's single run at the top of its stack. I checked that the
core PRs listed below exist and are open (`gh pr list`, 2026-10-04). Security specifics stay out of
this file because citrate-core is public; they are in the private federation tracker.

## Shape of the run

- **25 lanes.** 14 fresh lanes each had a builder agent and then an adversarial reviewer that re-ran
  the gates in a fresh worktree, broke guards on purpose and pushed signed fix commits. 10 lanes
  were "review and finish" lanes for fan-out 5 branches that had never been reviewed (one agent each,
  reviewing, fixing and closing gaps). One lane (M2-finish) merged the knowledge lane into the M2
  branches and opened the M2 PRs.
- **8 stacking lanes**, one per repo, merged the reviewed branches into one chain with signed merge
  commits (no rebase, no force-push), resolved the semantic joins, ran the full gates once at the top
  and opened each PR once.
- **Base commits.** runtime `hup/m2-runtime` @ d10f0ee, core `hup/m2-core` @ 7581e88 to 77e3bdf
  (lanes merged forward as M2 moved), chain `main` @ 0aab474b, other repos `main`.
- **Review verdicts.** All 14 separate reviews returned pass-after-fixes. No lane was excluded from
  a stack. No unfixed High.

## Pull requests

### M2 (opened by the M2-finish lane; merge first)

| Repo | PR | Branch |
|---|---|---|
| citrate-agent-runtime | [#47](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/47) | `hup/m2-runtime` to `main` |
| citrate-core | [#190](https://github.com/CitrateNetwork/citrate-core/pull/190) | `hup/m2-core` to `release/0.5.0-hermes-upskill` |
| citrate-memories | [#17](https://github.com/CitrateNetwork/citrate-memories/pull/17) | `hup/m2-knowledge` to `main` |

### citrate-core stack (on #190, bottom to top)

| PR | Lane | Items | Status (builder, review) |
|---|---|---|---|
| [#191](https://github.com/CitrateNetwork/citrate-core/pull/191) | N5-verify | HUP-S1.3, US-1.3 AC2, g0-formal and g1 evidence | complete; pass-after-fixes |
| [#192](https://github.com/CitrateNetwork/citrate-core/pull/192) | N5-retrieval | HUP-S1.2, US-1.4 AC1, skills bundled | partial; pass-after-fixes |
| [#193](https://github.com/CitrateNetwork/citrate-core/pull/193) | P-split | HUP-S1.9 live parity, g1-loop evidence | partial; pass-after-fixes |
| [#194](https://github.com/CitrateNetwork/citrate-core/pull/194) | N5-eval2 | HUP-S1.7, S1.10, US-9.2 AC2 | partial; pass-after-fixes |
| [#195](https://github.com/CitrateNetwork/citrate-core/pull/195) | P-escalation | HUP-S1.5 | partial; pass-after-fixes |
| [#196](https://github.com/CitrateNetwork/citrate-core/pull/196) | N5-session | HUP-S1.1, S1.8, g1-loop | partial; pass-after-fixes |
| [#197](https://github.com/CitrateNetwork/citrate-core/pull/197) | RT-runtime (core half) | red-team hardening | partial; review-and-finish |
| [#198](https://github.com/CitrateNetwork/citrate-core/pull/198) | RT-core | red-team hardening | complete; review-and-finish |
| [#199](https://github.com/CitrateNetwork/citrate-core/pull/199) | N5-web | HUP-S5.1 to S5.4 | partial; pass-after-fixes |
| [#200](https://github.com/CitrateNetwork/citrate-core/pull/200) | P-nodemcp | HUP-S4.2, S8.5, g2-mcp | partial; review-and-finish |
| [#201](https://github.com/CitrateNetwork/citrate-core/pull/201) | N5-mcphost | HUP-S4.1 | partial; pass-after-fixes |
| [#202](https://github.com/CitrateNetwork/citrate-core/pull/202) | L-hellomint | g3-gate, US-6.1 local half | partial; pass-after-fixes |
| [#203](https://github.com/CitrateNetwork/citrate-core/pull/203) | CH-fork (core) | HUP-S6.10 | partial; review-and-finish |
| [#204](https://github.com/CitrateNetwork/citrate-core/pull/204) | CH-faucet (core) | HUP-S6.5 | partial; review-and-finish |
| [#205](https://github.com/CitrateNetwork/citrate-core/pull/205) | N5-forge | HUP-S6.2, S6.3, S6.7, S6.9, US-6.3 AC2, US-6.4 | partial; pass-after-fixes |
| [#206](https://github.com/CitrateNetwork/citrate-core/pull/206) | P-anchor | HUP-S7.3, S7.5, g4-anchor local evidence | partial; pass-after-fixes |
| [#207](https://github.com/CitrateNetwork/citrate-core/pull/207) | P-fleet | HUP-S8.1 to S8.4 | partial; review-and-finish |
| [#208](https://github.com/CitrateNetwork/citrate-core/pull/208) | P-flrounds | HUP-S9.4 | partial; pass-after-fixes |
| [#209](https://github.com/CitrateNetwork/citrate-core/pull/209) | N5-everyday | HUP-S10.2 to S10.6, S7.6 | partial; pass-after-fixes |
| [#210](https://github.com/CitrateNetwork/citrate-core/pull/210) | L-sizelicence | g5-size macOS, g3-licence draft | partial; pass-after-fixes |

This documentation PR sits on `hup/m2-core` beside #191 and does not depend on the stack.

### Other repos

| Repo | PRs | Lanes |
|---|---|---|
| citrate-agent-runtime | [#48](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/48) to [#60](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/60), stacked on #47 | P-split, N5-eval2, P-escalation, N5-retrieval, N5-session, N5-verify, RT-runtime, N5-mcphost, N5-web, CH-faucet, N5-forge, P-anchor, N5-everyday |
| citrate-chain | [#270](https://github.com/CitrateNetwork/citrate-chain/pull/270) to [#274](https://github.com/CitrateNetwork/citrate-chain/pull/274) | CH-faucet, CH-fork, CH-redeploy, CH-precompiles, CH-fl |
| citrate-cluster | [#10](https://github.com/CitrateNetwork/citrate-cluster/pull/10), [#11](https://github.com/CitrateNetwork/citrate-cluster/pull/11) | RT-other, P-fleet |
| citrate-explorer | [#17](https://github.com/CitrateNetwork/citrate-explorer/pull/17) | RT-other |
| citrate-memories | [#18](https://github.com/CitrateNetwork/citrate-memories/pull/18) (independent of #17) | RT-other |
| citrate-compute-pool | [#27](https://github.com/CitrateNetwork/citrate-compute-pool/pull/27) | CH-fl |
| citrate-settlement | [#10](https://github.com/CitrateNetwork/citrate-settlement/pull/10) | CH-fl |

Merge recipes are in each PR body. Two cross-repo pairs must land together: runtime and core
`hup/n5-rt-runtime`, and runtime `hup/n5-chain-faucet` before core `hup/n5-chain-faucet`. The
parity fixture `parity-v1.json` must stay byte-identical in core and runtime
(sha256 `8a777d62...b31b` at both stack tops).

## Stack-top gates (one run each, by the stacking lanes)

| Repo | Base | Top |
|---|---|---|
| citrate-core | cargo 1,745, vitest 1,839 | cargo 2,142 passed / 0 failed / 16 ignored; vitest 2,186 passed / 33 skipped; tsc, fmt, clippy 1.98.1 clean; release-pin tripwire and licence inventory OK |
| citrate-agent-runtime | 1,856 | 2,119 passed / 0 failed / 8 ignored; clippy 1.98.1 clean on every crate the stack touches |
| citrate-chain | (main) | cargo 4,784 passed; forge 3,192 passed; clippy 1.96.0 clean |
| citrate-cluster | (main) | 121 passed; TLC DeviceLink, ClusterAdmission, ClusterWireAdmission clean |
| citrate-explorer | 588 | 589 passed; `pnpm audit --audit-level=high` exit 0 |
| citrate-memories | 353 | 355 passed |
| citrate-compute-pool | 429 | 512 passed |
| citrate-settlement | 41 | 61 passed; TLC FlChallengeWindow clean, 4 mutant configs violate |

Not run anywhere in this run: Linux or Windows hardware, a packaged-app click-through by a person,
anything on chain 40204.

## Tracking

- Sprint issues (private federation repo): S1 #278, S2 #279, S3 #280, S4 #281, S5 #282, S6 #283,
  S7 #284, S8 #285, S9 #286, S10 #287, S11 #288, federation #289. Each got lane comments and a
  stacking comment with its PR links; this lane adds one status comment each.
- Project 4: every sprint stays In Progress. No sprint has all its work packages met.
- Rule 6: core/execution changed in citrate-chain (#273), so the daily chain benchmark is owed after
  the chain stack merges.
