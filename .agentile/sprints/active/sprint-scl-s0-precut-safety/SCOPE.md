---
created: 2026-10-05T18:30:00Z
branch: scl/s0-precut-safety (PRs into release/0.5.0-hermes-upskill; private items through the #298 route)
author: Larry Klosowski + Claude Opus 5.5
status: active
updated: 2026-10-05 (owner decision: SCL-S0 is in the v0.5.0 cut-blocking subset, D-2 amended; second set: this sprint is the whole SCL content of v0.5.0, with S1.6a, S7.5a, S8.5a)
sprint: SCL-S0
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (+ citrate-agent-runtime for S0.3, S0.4)
release: v0.5.0 (lands before the cut; owner decision 2026-10-05, second set: v0.5.0 ships right after the 40204 reroll and its 2,000-block soak)
tier: T1
companions: ../../../planset/2026-10-05-sidecar-lifecycle/05_SPRINTS_AND_WPS.md
---

# Sprint SCL-S0: Pre-cut safety

The first sprint of the owned sidecar lifecycle program
([planset](../../../planset/2026-10-05-sidecar-lifecycle/00_OVERVIEW.md),
[adoption ADR](../../../../docs/adr/ADR-2026-10-05-owned-sidecar-lifecycle.md)). It lands
the lifecycle fixes that must be in v0.5.0 whatever the state of the rest of the program,
because 0.5.0's own update and exit code is what every member's next update runs.

Credit: the underlying design and evidence are @mfarzanansari's (#240, #241).

## Private matters

citrate-core is public. S0.1, S0.3 and S0.4 are pre-cut safety fixes tracked privately on
federation #298; this file names them only generically. Do not add mechanism, platform or
packaging specifics here, in commit messages, or in public PR text. Their PRs go through the
private remediation route and are merged by the owner.

## Work packages

Each WP is red, then green, then close-with-proof. Definitions and acceptance criteria are in
[05_SPRINTS_AND_WPS §SCL-S0](../../../planset/2026-10-05-sidecar-lifecycle/05_SPRINTS_AND_WPS.md).

| WP | Summary | Stories | Effort | Native run | Status |
|---|---|---|---|---|---|
| S0.1 | Startup cleanup matches exact owned executable paths only (pre-cut safety fix tracked privately) | US-0.1 | S | each OS's installed package | not started |
| S0.2 | Windows update install drains sidecars before the installer runs | US-0.2 | S | Windows team (@RDCTart69, @kurtatwork) | not started |
| S0.3 | Managed-browser cleanup on Hermes stop and at the next start (pre-cut safety fix tracked privately) | US-0.3 | M | macOS, Linux | not started |
| S0.4 | Runtime-side containment correction (pre-cut safety fix tracked privately) | (rt) | S | Linux, macOS CI | not started |
| S0.5 | Shutdown coverage check: embed stops with Hermes, workers exit on stdin close | (QA) | S | all three OSes | not started |
| S0.6 | Genesis-change update check: no old node left holding the chain database or ports; if one is, the UI names it | US-0.4 | M | all three OSes | code in review (PR scl/s0.6-s8.5a, [EVIDENCE](EVIDENCE.md)); native runs owed |
| S0.7 | Windows installer stops this installation's own sidecars before copying files (red-team addition RT-04) | US-0.2 (amended) | M | Windows team: manual 0.4.2 to 0.5.0 install with the old app running | not started |

*Red-team correction (2026-10-05, RT-03):* S0.2 targets the Windows in-app update install path,
which is not live in 0.5.0 (the Windows bundle ships no updater artifacts). It is conditional
on a Windows feed and is not part of `g1-precut`; S0.7 covers the path Windows members use.
S0.6's per-OS update run means the macOS in-app update and the manual Linux and Windows
installs.

*Owner decision (2026-10-05, O-20 accepted, D-2 amended):* every WP of this sprint is in the
v0.5.0 cut-blocking subset, including S0.7. S0.2 stays conditional (no Windows updater feed in
0.5.0), so it has nothing to gate at the cut and is tracked as 0.5.x follow-up until a Windows
feed exists. `g1-precut` is tagged `blocks_cut: true`.

*Owner decision (2026-10-05, second set, D-2 amended again):* v0.5.0 ships right after the
40204 reroll and its 2,000-block soak with only this sprint's SCL content. Three WPs join the
sprint for v0.5.0 (definitions and acceptance in
[05 "Owner decision (2026-10-05, second set)"](../../../planset/2026-10-05-sidecar-lifecycle/05_SPRINTS_AND_WPS.md#owner-decision-2026-10-05-second-set-v050-v051-v05x)):

| WP | Summary | Stories | Effort | Native run | Status |
|---|---|---|---|---|---|
| S1.6a | Minimal hosted Windows CI job that builds the S0.7 NSIS hook and runs its test (no Windows CI lane exists today; the only strict dependency of S0) | (CI) | S | (hosted runner) | not started |
| S8.5a | Startup cleanup barrier before node admission over the S0.1 cleanup; #243 reset also requires the chain database lock; crash-window residual stated | US-7.3 AC1, AC2 | M | with the S0.6 runs | code in review (same PR as S0.6, [EVIDENCE](EVIDENCE.md)); native runs owed |
| S7.5a | Ask first when the local server is not running (restart the local model, or send this message to the gateway this time; per message); bounded wait while the app's own startup start is pending (D-16 extended) | US-6.2 AC5 | M | macOS packaged | not started |

`g1-precut`, `g3-recorded-cleanup-v050`, `g3-provider-routing-v050`, `g4-native-v050` and
`g5-release-coupling` are the criteria tagged `release: v0.5.0`. Estimate: 9 WPs, about 19
agent-days, 9 to 14 working days from 2026-10-06 including native runs and the ceremony.
Book the Windows team's S0.7 run now; it needs a 0.5.0 Windows installer and a machine running
0.4.2.

## Exit criteria

Gate criterion `g1-precut` in the SCL [gates.yaml](../../../planset/2026-10-05-sidecar-lifecycle/gates.yaml):
every WP merged with a regression test that failed first, and the native checks recorded in
this sprint's `EVIDENCE.md` (created with the first WP PR).
*Owner decision (2026-10-05, second set):* plus every other criterion tagged `release: v0.5.0`
(listed above).

## Rule-2 record

Test counts (`cargo test --workspace --locked`, vitest, runtime workspace) are recorded per
PR in `EVIDENCE.md`. Baseline: taken at the first PR from `release/0.5.0-hermes-upskill`
`d16f194` or its successor. Taken 2026-10-06 at `6ab289e`: Rust 2314 passed / 18 ignored,
vitest 2428 passed (see [EVIDENCE](EVIDENCE.md)).

## Dependencies and coordination

- No dependency on SCL-S1 or later sprints. *Owner decision (2026-10-05, second set):* except
  S1.6a, the minimal Windows CI job S0.7's acceptance needs, now carried in this sprint. S8.5a
  depends on S0.1 and shares S0.6's lock-holder fixture; S0.6 and S8.5a touch the same node
  startup and #243 code, so they run in one lane.
- S0.2 is superseded later by SCL-S12.2 (native coordinated update commands); keep it
  minimal.
- S0.6 coordinates with the #243 chain DB reset (it must not reset while an older node
  answers) and with the 40204 reroll timeline, which is independent of SCL.
- The S0.3 and S0.4 runtime PRs need the Hermes binary rebuilt from runtime `main` for the
  cut; record the runtime commit in `EVIDENCE.md`.

## Daily log

- 2026-10-05: sprint opened with the Stage-1 planset (docs only). No code yet.
- 2026-10-05: planset red-teamed (08_RED_TEAM, 29 findings, 3 blocking). S0.7 added, S0.2
  made conditional. Still docs only.
- 2026-10-05: owner decisions recorded (O-20 accepted: SCL-S0 blocks the v0.5.0 cut; O-18 and
  O-19 accepted; red-team pass accepted). No change to the S0 WPs. Still docs only.
- 2026-10-05: owner decisions, second set: v0.5.0 ships after the reroll and soak with only this
  sprint's SCL content; S1.6a, S8.5a and S7.5a join the sprint; the previous cut-blocking subset
  is the v0.5.1 gate (HUP D-41 amended for one v0.5.1). Still docs only.
- 2026-10-06: S0.6 and S8.5a coded in one lane (PR from `scl/s0.6-s8.5a`): startup barrier
  around the existing startup cleanup, holder check (chain database lock, node ports) before
  every node start with the holder named in the UI, and the #243 reset now takes the chain
  database lock before deleting. Red then green; Rust 2333 / vitest 2432 after. Cross-checked
  on macOS against a real node holding its database. Native update runs on the rerolled chain
  still owed (EVIDENCE.md).
