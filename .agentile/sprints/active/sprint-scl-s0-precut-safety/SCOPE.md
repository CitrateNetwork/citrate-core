---
created: 2026-10-05T18:30:00Z
branch: scl/s0-precut-safety (PRs into release/0.5.0-hermes-upskill; private items through the #298 route)
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: SCL-S0
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (+ citrate-agent-runtime for S0.3, S0.4)
release: v0.5.0 (lands before the cut)
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
| S0.6 | Genesis-change update check: no old node left holding the chain database or ports; if one is, the UI names it | US-0.4 | M | all three OSes | not started |

## Exit criteria

Gate criterion `g1-precut` in the SCL [gates.yaml](../../../planset/2026-10-05-sidecar-lifecycle/gates.yaml):
every WP merged with a regression test that failed first, and the native checks recorded in
this sprint's `EVIDENCE.md` (created with the first WP PR).

## Rule-2 record

Test counts (`cargo test --workspace --locked`, vitest, runtime workspace) are recorded per
PR in `EVIDENCE.md`. Baseline: taken at the first PR from `release/0.5.0-hermes-upskill`
`d16f194` or its successor.

## Dependencies and coordination

- No dependency on SCL-S1 or later sprints.
- S0.2 is superseded later by SCL-S12.2 (native coordinated update commands); keep it
  minimal.
- S0.6 coordinates with the #243 chain DB reset (it must not reset while an older node
  answers) and with the 40204 reroll timeline, which is independent of SCL.
- The S0.3 and S0.4 runtime PRs need the Hermes binary rebuilt from runtime `main` for the
  cut; record the runtime commit in `EVIDENCE.md`.

## Daily log

- 2026-10-05: sprint opened with the Stage-1 planset (docs only). No code yet.
