---
created: 2026-10-04T14:51:38Z
branch: hup/m2-core
author: Larry Klosowski + Claude Opus 5.5
status: active
milestone: M2 = 0.4.3 (HUP-S2 Human in control + HUP-S3 Knowledge and skills)
planset: .agentile/planset/2026-09-30-hermes-upskill
sprint_issues: CitrateNetwork/citrate-federation#279 (S2), #280 (S3)
---

# M2 status (0.4.3), re-verified on the merged branches

This file re-runs the M2 verification from fan-out 5 (30 items: 4 gates, 9 stories, 17 work
packages) against the combined branches after the knowledge lane was merged in:

- citrate-agent-runtime `hup/m2-runtime` (merge of `hup/m2-knowledge`)
- citrate-core `hup/m2-core` (merge of `hup/m2-knowledge`, plus the follow-ups in this file)
- citrate-memories `hup/m2-knowledge`

Verdicts: **met** (the acceptance criteria hold in code with tests, wired for members where the
default allows), **remaining** (with the exact gap and who owns it: buildable, owner or external).
D-41 holds: no version bump and no release here. "Packaged-app click-through" is S11 QA for every
item and is not repeated per row.

## Summary

| | met | remaining |
|---|---:|---:|
| Gates (4) | 3 (g0-rule3-adr, g2-hic, g2-personas) | 1 (g2-knowledge: owner target) |
| Stories (9) | 6 | 3 (US-2.3, US-3.1, US-3.4) |
| Work packages (17) | 14 | 3 (S2.3, S3.1, S3.4) |

Every remaining item is waiting on an owner decision or on external work (chain operator, DGX
team, Linux or Windows hardware). No buildable gap is left open in M2 scope that this lane knows
of; two small hardening items found in review were closed here (see "Closed in this pass").

## Closed in this pass (hup/m2-core)

- **Knowledge lane finish.** `eval/results/SCORECARD.md` regenerated with the 2026-10-02 T0 and
  T1 memory_search runs (the committed-scorecard test failed without it).
  `eval/results/README.md` has the measured run table, the failure groups and a proposed AC2
  target; `eval/QA-v1.md` and `docs/KNOWLEDGE_CORPUS_IMPORT.md` carry the measured numbers. The
  corpus row in `release/budgets.json` already holds the measured staged size (99,669,598 bytes;
  58,498,298 as the tarball), so it is unchanged.
- **Grant refusal parity (S2.1, US-2.1).** Core's early grant refusal now mirrors the folder rules
  of the sidecar's default-deny list (credentials, keychains, browser profiles, wallet keystores,
  Citrate key folders, system secrets), matched at any depth and in any letter case. The Grants
  panel can no longer list a grant the sidecar would ignore. Test
  `folders_on_the_sidecar_deny_list_are_refused_at_any_depth_and_case`; mutant (case fold
  removed) killed.
- **Learned-memory ledger is owner-only (S3.4).** `learned-memories.json` is written 0600 through a
  fresh temporary file, also when it replaces an older world-readable file. Test
  `the_ledger_file_is_private_to_the_member`.

## Gates

| Gate | Verdict | Evidence | Remaining |
|---|---|---|---|
| gate0 g0-rule3-adr | met | ADR-2026-09-30-rule3-budgetable-signatures accepted, 5 of 5 rows; WebSigningBudget TLC green (see below) | Optional independent formal review; A17 ADR amendment |
| gate2 g2-hic | met | Folder grants: FolderGrant TLC green (153,484 distinct states, 2026-10-04 on the merged runtime), agent-grants and traversal fuzz tests. Shell: agent-shell OS sandbox (Seatbelt on macOS, bubblewrap on Linux; deny-default, no network, writes only in the grant and a scratch HOME), `shell_run` with exact argv + cwd HIC approval, shell_run_tests. SIWE budgets: kit web_budget tests, core web_signin.rs live path with core-side origin and taint, WebSigningBudget TLC green. Capsule sandbox: per-session `SessionSandbox` (agent-sidecar sessions.rs) plus 9 guest-capsule tests on a compiled wasm32-wasip2 probe. Taint: TaintDowngrade TLC green (120 distinct states). | External: Linux and Windows proof runs (DGX team / hardware). Owner: `shell_run` and the toolchain templates stay off by default (`CITRATE_HERMES_SHELL_RUN`, `CITRATE_HERMES_TOOLCHAIN`); no member UI binds capsule mounts or egress consent yet, so capsules get no folder and no address by default |
| gate2 g2-knowledge | remaining (owner) | Corpus format 2 staged in all five bundle configs and release.yml behind the BGE gate; first-run import tested. QA through the app's memory_search on 2026-10-02: T1 77.3 % pass / 99.7 % citation validity, T0 65.3 % / 98.8 % (SCORECARD.md) | Owner: the AC2 pass-rate target (proposal in eval/results/README.md, pending owner sign-off). External: release-machine upload of the corpus tarball and the import-corpus mem-mcp, pinned in runtime-deps.sha256 (A45, DGX team) |
| gate2 g2-personas | met | 6 personas, every name owner-approved 2026-10-01 (Graft, Pith, Zest, Trellis, Sprout, Crew; Operator ships), agent-loop/personas/personas.toml + persona_tests.rs. 5 tracks with 10 workflows, each `workflow_available = true`, run by `POST /sessions/:id/track_workflows` (track_workflow_route_tests.rs) and from chat with `/run <workflow>` (core trackWorkflows.ts, vitest) | Shipped defaults pending owner sign-off: TTS voice unset on every persona (system voice), Guide defaults to full-project and Operator to project-management |

## Stories

| Story | Verdict | Evidence | Remaining |
|---|---|---|---|
| US-2.1 Folder grants | met | See S2.1; grant refusal parity closed in this pass | Windows run (hardware) |
| US-2.2 Shell with a leash | met | AC1 OS sandbox (agent-shell sandbox.rs; live forge and slither under Seatbelt). AC2 `shell_run` exact command + cwd card (core approvalCards commandCard, decision bound to argv + cwd). AC3 timeout up to 600 s, 32 KiB capped capture, run report into the Activity log (core f20bef9), decision recorded in the anchored log (runtime aff2600) | Linux bubblewrap proof run (DGX team); Windows has no backend. Owner: shell_run default (off) |
| US-2.3 Sign into dApps safely | remaining (external) | Live path in the managed browser: core reads the request, attests the top-frame origin from its own DevTools read and takes taint from the sidecar, never from the webview; budgets through `request_siwe_budgeted`; "Signed for you" with revoke; records exported into the decision log the nightly anchor batches; budget generation sealed in the keychain (older file fails closed); ceremony tripwires scan all production code | External: AnchorRegistry on 40204 for the anchor transaction (AC3 anchored on chain), a packaged run against a real https dApp. Owner: placeholders listed in WEB_SIGNING_BUDGETS.md |
| US-2.4 Approval cards | met | approvalCards.ts, ApprovalCardView, `hicAware: true`, stale-approval binding; vitest | none beyond S11 QA |
| US-2.5 Capsule sandboxes | met | AC1 per-session sandbox wired (sessions.rs `SessionSandbox::new`), resolved at every call from live grants; guest-capsule tests cover escapes, read-only mounts, sockets and egress consent. AC2 hash + ed25519 + allowlist at load, pins in capsule_pins.rs, capsules in all bundle configs. Egress needs per-address member consent and non-public addresses are refused | Member UI for mount bindings and egress consent (no shipped capsule declares filesystem or network today, so nothing is blocked by its absence) |
| US-3.1 Knows Citrate offline | remaining (owner + external) | AC1 corpus has every named source (Medusa and Slither docs ship by owner decision 2026-10-01); staged in bundle configs and release.yml. AC2 measured 77.3 % T1 / 65.3 % T0 with citations resolving to bundled nodes (99.7 % / 96.9 % of citations name a retrieved node) | Owner: AC2 target. External: A45 release upload |
| US-3.2 Skills load when needed | met | AC1 at most five skills surfaced per turn (agent-loop `SkillLibrary::select`, BM25; skills_select_tests.rs); reviewed third-party skills load through the same strict loader and are checked against skills.lock sha256 pins (skills_lock_tests.rs). AC2 member skills are SKILL.md files the one loader reads (core `skills_local_migrate`); hermes-fork frontmatter rewritten at intake (owner decision) | none in M2 |
| US-3.3 Personas and tracks | met | See g2-personas; the persona's skill allowlist decides which skills a session offers (sessions.rs, `restricted_to`); the sidecar checks a custom persona itself and core's sidecar path sends the persona field | The in-app fallback loop still composes the persona prompt locally (used only when the sidecar is down) |
| US-3.4 Learns only what's proven | remaining (owner + external) | AC1/AC2 reachable from the "Teach Hermes" card; contradiction resolve (ContradictionResolve.tla); AC3 publish path builds calldata, pins SKILL.md to local IPFS and checks its hash, opens a PENDING ceremony | Owner: `SKILL_PUBLISH_ENABLED` stays false pending sign-off. Before enabling: decode the publish calldata against the ABI. External: SkillRegistry address on 40204. Open: recall does not yet hide an unresolved memory; a contradiction with a non-learned memory can be surfaced but not resolved |

## Work packages

| WP | Verdict | Evidence / remaining |
|---|---|---|
| S2.0 | met | ADR accepted; optional formal review and A17 amendment |
| S2.1 | met | runtime agent-grants + sidecar grants.rs; core agent_grants.rs, GrantsPanel, push_grants; FolderGrant TLC; parity closed here. Windows run external |
| S2.2 | met | OS sandbox + shell_run + Activity log + decision record. Linux proof external; default off (owner) |
| S2.3 | remaining (external) | All buildable items built (see US-2.3). AnchorRegistry on 40204 and a packaged https run remain |
| S2.4 | met | See US-2.4 |
| S2.5 | met | See US-2.5 |
| S2.6 | met | Sidecar writes every HIC-1/2 event into the one records dir the anchor batches (`hic_records.rs`, ceremony approvals, browser actions, learn, core events over `/records/core`, web signing); core passes `CITRATE_HERMES_RECORDS_DIR` (hermes.rs). Owner (A16): retention, segment size, fail-closed vs quarantine, device-key MAC; approvals fail closed as a conservative placeholder pending owner sign-off |
| S2.7 | met | TaintDowngrade TLC green; sidecar loop is on by default for members (owner decision 2026-10-01) |
| S2.8 | met | One deny list shared by fs, shell and capsules; core's early check now mirrors it. Windows run external |
| S2.9 | met | file_write and sheet_write go through the checkpointed path; checkpointed fs_* for sessions with core's grant document; core undo cards. Windows symlink restore external |
| S3.1 | remaining (owner + external) | See US-3.1 |
| S3.2 | met | See US-3.2 |
| S3.3 | met | See US-3.3 |
| S3.4 | remaining (owner + external) | See US-3.4; ledger permissions closed here |
| S3.5 | met | QA set + scorecard; T1 run now exists. Owner (A50): cut toolcall/injection v2 or accept appending |
| S3.6 | met | skills.lock + intake review; hermes-fork rewrite at intake (owner decision). Owner (A18): frontend-skills LICENSE, ToB CC BY-SA attribution, merit exclusions |
| S3.7 | met | Names owner-approved, Operator ships; TTS voices and Guide/Operator default tracks are shipped defaults pending owner sign-off |

## Gate runs on the merged branches (2026-10-04)

- TLC, no error in each: FolderGrant (153,484 distinct states), TaintDowngrade (120),
  SkillPersistence (480,480), WebSigningBudget with WebSigningBudget.cfg (7,828,872 distinct,
  37,916,870 generated).
- citrate-agent-runtime `hup/m2-runtime`: `cargo test --workspace --locked` 1,856 passed, 0
  failed, 4 ignored. clippy 1.98.1 `-D warnings` clean on agent-loop and agent-sidecar; rustfmt
  clean for both. The live-Chrome browser tests flaked twice under a load average near 280 from
  parallel lanes and passed serially twice; the crate is untouched by the merge.
- citrate-core `hup/m2-core`: `cargo fmt --all -- --check` clean; clippy 1.98.1 `--workspace
  --all-targets -D warnings` clean; `cargo test --workspace --locked` 1,745 passed, 0 failed, 11
  ignored; `npx tsc --noEmit` clean; `npx vitest run` 1,839 passed, 10 skipped.
- citrate-memories `hup/m2-knowledge`: `cargo test --workspace --locked` 377 passed, 0 failed;
  clippy 1.98.1 workspace `-D warnings` clean. `cargo fmt --check` reports drift that already
  exists on main across ten crates (memories CI does not gate on fmt); not reformatted here.

## Size budget note (gate g5-size, M7)

The corpus (99,669,598 bytes) and the reviewed skills bundle (8,690,842 bytes) take the projected
macos-aarch64 app from the 724,520,985-byte v0.4.2 baseline to about 833 MB, over its 797 MB
budget. Raising that budget is an owner decision under g5-size and is not changed here.
