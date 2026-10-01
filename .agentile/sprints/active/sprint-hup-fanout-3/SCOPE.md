---
created: 2026-10-01T12:00:00Z
branch: docs/hup-fanout-3
author: Larry Klosowski + Claude Opus 5.5
status: active
type: sprint-scope
planset: 2026-09-30-hermes-upskill
release: 0.5.0 (D-41; no version bumps in this run)
---

# HUP fan-out 3: the 2026-10-01 overnight run

This file is the truth for the third parallel fan-out on the Hermes upskill (HUP) program (Rule 4).
The owner asked for it before going to bed on 2026-10-01: finish what the playbook allows, stack the
PRs so they merge with merge commits (no rebase, no squash), spend CI once, keep the GitHub project
and issues current, and write the Agentile docs.

Nothing in this file is merged. Every WP below is a branch with an open PR in a merge-commit stack.

## Shape of the run

- 15 work packages in 15 lanes: 7 in `citrate-agent-runtime` (base `origin/main` @ 3d75efb), 8 in
  `citrate-core` (base `origin/release/0.5.0-hermes-upskill` @ 525b9ca).
- Every lane had a builder agent and then an adversarial reviewer agent. The reviewer re-ran the gates
  in a fresh worktree, broke guards on purpose, and pushed signed fix commits.
- Two stacking lanes (one per repo) merged the reviewed branches into one chain with signed
  `git merge --no-ff` commits, ran the full gates once at the top, and opened each PR once.
- No PR was opened before the stacking step, so CI ran once per PR.

## Work packages

Tests are after review fixes. Runtime workspace counts are per branch from the 857 baseline and do not
add up; the stack-top run below is the merged number.

### citrate-agent-runtime (stack: #23 → #29)

| WP | Branch | PR | Base | Tests (after review) | Formal | Status |
|---|---|---|---|---|---|---|
| S2.1-rt folder grants | `hup/n3-folder-grants` | [#23](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/23) | main | crate 35; workspace 892 computed, not measured by the reviewer | TLC `FolderGrant` 153,484 distinct (595,915 larger); 11 + 1 mutants caught | open, pass-after-fixes |
| S2.9-rt undo checkpoints | `hup/n3-undo-checkpoints` | [#24](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/24) | #23 | crate 51; workspace 908 | none | open, pass-after-fixes |
| S3.4-rt verified self-learning | `hup/n3-verified-learning` | [#25](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/25) | #24 | crate 44; workspace 901 | TLC `SkillPersistence` 480,480 distinct; 12 + 2 mutants caught | open, pass-after-fixes |
| S7.5-rt + S9.3 metering, trajectories | `hup/n3-metering-trajectories` | [#26](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/26) | #25 | metering 28, trajectory 38; workspace 923 | none | open, pass-after-fixes |
| S7.3-rt nightly anchor batch | `hup/n3-anchor-batch` | [#27](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/27) | #26 | crate 32; workspace 889 | TLC `AnchorBatch` 5,813 distinct (756,731 larger); 7 + 2 mutants caught | open, pass-after-fixes |
| S4.1 MCP host | `hup/n3-mcp-host` | [#28](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/28) | #27 | crate 48, sidecar +7; workspace 912 | none | open, pass-after-fixes |
| S6.3 toolchain verifiers | `hup/n3-toolchain-verifiers` | [#29](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/29) | #28 | +72; workspace 929 + 2 ignored (live forge/slither, pass by hand) | none | open, pass-after-fixes |

### citrate-core (stack: #139 → #146)

| WP | Branch | PR | Base | Tests (after review) | Formal | Status |
|---|---|---|---|---|---|---|
| S11.0 + S11.2 size budget, manual eval workflow | `hup/n3-release-gates` | [#139](https://github.com/CitrateNetwork/citrate-core/pull/139) | release/0.5.0-hermes-upskill | vitest 796 → 840 | none | open, pass-after-fixes |
| S6.2 + S6.9 contract templates, Medusa budgets | `hup/n3-templates` | [#140](https://github.com/CitrateNetwork/citrate-core/pull/140) | #139 | citrate-templates 33; vitest +5; forge build/test green on 6 rendered templates | none | open, pass-after-fixes |
| S0.3b HF token on catalog downloads | `hup/n3-hf-token` | [#141](https://github.com/CitrateNetwork/citrate-core/pull/141) | #140 | lib 533 → 559 | none | open, pass-after-fixes |
| S1.6-rest tier drives serve | `hup/n3-tier-drives-serve` | [#142](https://github.com/CitrateNetwork/citrate-core/pull/142) | #141 | lib 533 → 557 | none | open, pass-after-fixes |
| S10.4 daily entry, encrypted journal export | `hup/n3-journal-export` | [#143](https://github.com/CitrateNetwork/citrate-core/pull/143) | #142 | workspace 745 → 764; vitest 796 → 839 | none | open, pass-after-fixes |
| S6.4 D-4 deploy gate | `hup/n3-deploy-gate` | [#144](https://github.com/CitrateNetwork/citrate-core/pull/144) | #143 | lib 533 → 571; vitest 796 → 815 | TLC `DeployGate` 3,378 distinct; 7/7 scripted mutants + 1 reviewer mutant | open, pass-after-fixes |
| S5.4 + S7.6 pop-outs, Activity monitor | `hup/n3-popouts-monitor` | [#145](https://github.com/CitrateNetwork/citrate-core/pull/145) | #144 | lib +24; vitest 796 → 855 | none | open, pass-after-fixes |
| S6.8 + S7.7 faucet ADR (proposed), literacy pack | `hup/n3-faucet-adr-literacy` | [#146](https://github.com/CitrateNetwork/citrate-core/pull/146) | #145 | vitest 840 (831 + 9 skipped without source repos) | none | open, pass-after-fixes |

## Test baselines

| Repo | Baseline (before) | Stack top (measured once, 2026-10-01) | Delta |
|---|---|---|---|
| citrate-agent-runtime | `cargo test --workspace` 857 passed on main @ 3d75efb | 1212 passed, 0 failed, 2 ignored @ 2b6d192 | +355, equal to the sum of the reviewed lane deltas |
| citrate-core (cargo) | 745 passed, 6 ignored (lib 533 + kit 212) @ 525b9ca | 909 passed, 7 ignored @ b32c0c1 (lib 661, kit 215, citrate-templates 33) | +164 |
| citrate-core (vitest) | 796 passed, 3 skipped | 1003 passed, 9 skipped | +207 |

Also at the stack tops: clippy 1.98.1 `-D warnings` clean on every changed crate; `tsc --noEmit` clean;
main-thread tripwire 3/3; TLC `DeployGate` re-run green with its non-vacuity config violated as
intended; runtime `cargo audit` advisory list identical to main's (nothing new).

CI at the time of writing: all 8 core PRs are 4/4 green. The 7 runtime PRs had their fast checks
(secret scan, HIC terminology guard) green and the rust/audit jobs still pending.

## Merge order

Bottom up, with merge commits: runtime #23, #24, #25, #26, #27, #28, #29; core #139 through #146.
Merging one PR retargets the next onto its base. The owner may choose a different merge method for
individual PRs; that decision is recorded privately.

## Daily log

### 2026-10-01

- Relaunch from a fresh session after the owner said "go". Verified the bases: core integration
  branch @ 525b9ca, runtime main @ 3d75efb, no `hup/n3-*` branches anywhere, about 32 GB free.
- 15 builder lanes ran in parallel on shared cargo target dirs (one per repo). Each pushed a signed
  branch with no PR and commented on its federation sprint issue.
- 15 reviewer lanes followed. All 15 verdicts were pass-after-fixes; every reviewer pushed at least one
  signed fix commit. Two reviewers did not re-run the full workspace because free disk was under 7 GB.
- Free disk fell from about 32 GB to 6.6 GB across the night. The runtime stacking lane deleted only
  `citrate-agent-runtime/target/debug/incremental` (8.8 GB) and built with `CARGO_INCREMENTAL=0`.
- Stacking: runtime 6 signed merge commits, no integration fixes; core 7 signed merge commits plus one
  signed integration fix (e71639f, see the retro). Full gates at both stack tops, then 15 PRs opened
  once each, reviewers BerryManifold and Matr0xshka requested.
- Documentation lane: this file, the retro, a field-log entry in citrate-journals, status comments
  and project status on the federation sprint issues, and one private issue for findings that stay
  out of public repos.
