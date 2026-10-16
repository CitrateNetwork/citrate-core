---
created: 2026-10-01T12:00:00Z
branch: docs/hup-fanout-3
author: Larry Klosowski + Claude Opus 5.5
status: active
type: retrospective
sprint: HUP fan-out 3 (spans S0, S1, S2, S3, S4, S5, S6, S7, S9, S10, S11)
planset: 2026-09-30-hermes-upskill
---

# Retro: HUP fan-out 3, the 2026-10-01 overnight run

This is a retro for one run, not a sprint close. Nothing below is merged. It follows
[RETRO-2026-09-30-fanout-2.md](../sprint-hup-s1-one-agent/RETRO-2026-09-30-fanout-2.md), and the
action numbers continue from its A22. The WP table, PR links and baselines live in
[SCOPE.md](SCOPE.md); this file links rather than copies them where it can (Rule 9).

I am the documentation lane, writing for the orchestrating agent. I did not re-run any test suite.
Every number here comes from a builder report, a reviewer report, or a stacking lane's stack-top run,
and the source is named where it matters. I did check that all 15 PR heads match the stacking lanes'
last pushed commits, and I read the CI state of each PR (`gh pr checks`, 2026-10-01).

## Outcome

| Repo | Lanes | Reviews | Stack-top tests | CI |
|---|---|---|---|---|
| citrate-agent-runtime | 7 (runtime #23 to #29) | 7 pass-after-fixes | 857 → 1212 passed, 2 ignored | fast checks green; rust and audit jobs pending at writing |
| citrate-core | 8 (core #139 to #146) | 8 pass-after-fixes | cargo 745 → 909; vitest 796 → 1003 | 8 of 8 PRs 4/4 green |

| WP | Repo / PR | Review findings (severity, fixed?) |
|---|---|---|
| S2.1-rt folder grants | runtime #23 | 1 medium fixed (loaded grants honour `granted_at`); 2 low fixed (public wording, README accuracy); 1 low open (second-look path not testable deterministically) |
| S2.9-rt undo checkpoints | runtime #24 | 2 high fixed (undo re-validates paths; git mode hardening); 1 low fixed (restore copy owner-only); 1 medium and 2 low open |
| S3.4-rt verified learning | runtime #25 | 1 medium fixed (two contradicting memories could both be stored as true); 1 low fixed; 4 low open (run reuse, re-proposal after reject, unbounded map, model scope) |
| S7.5-rt + S9.3 metering, trajectories | runtime #26 | 1 high and 2 medium fixed (redaction gaps); 2 low fixed; 1 low open (paths with spaces, documented) |
| S7.3-rt anchor batch | runtime #27 | 1 medium fixed (a surviving mutant on the body hash check); 3 low fixed (call, address and ledger validation) |
| S4.1 MCP host | runtime #28 | 1 medium fixed (servers connect in parallel at startup); 1 low fixed (3 surviving mutants); 3 low open |
| S6.3 toolchain verifiers | runtime #29 | 1 medium fixed (a parser panic on a malformed summary line); 1 medium partly fixed (free text in trusted results); 1 low fixed; 3 low open |
| S11.0 + S11.2 release gates | core #139 | 2 medium fixed (silent pass with an unknown arch; files over 2 GiB); 1 low fixed (whole-string URL check) |
| S6.2 + S6.9 templates | core #140 | 1 medium fixed (a failed render now leaves nothing behind); 1 medium partly fixed (fork-mode wallet routing: a warning only); 2 low open |
| S0.3b HF token | core #141 | 2 low fixed (one deadline across the redirect chain; a surviving scheme mutant); 2 info fixed |
| S1.6-rest tier drives serve | core #142 | 1 medium fixed (the 8192 floor was skipped for some trained lengths); 1 low fixed (a plan failure never blocks a model start) |
| S10.4 journal export | core #143 | 1 medium and 3 low fixed (file open hardening, mode on overwrite, extension case, summary wording); 2 low open |
| S6.4 deploy gate | core #144 | 2 high fixed (the pushed branch did not compile on a clean checkout; open-ceremony overflow now rejects); 2 low fixed; 2 low open |
| S5.4 + S7.6 pop-outs, monitor | core #145 | 1 high fixed (a stopped sidecar session gave empty replies for the rest of the session); 1 medium fixed (public wording); 1 medium owner decision and 1 medium runtime follow-up open |
| S6.8 + S7.7 faucet ADR, literacy | core #146 | 1 medium fixed (public wording); 2 low fixed (byte count, ADR persistence claim) |

## What went well

- **Fifteen reviewers, fifteen real findings.** No review was a rubber stamp. Every reviewer broke
  guards on purpose and recorded which mutants were caught and which survived, and every surviving
  mutant that mattered became a test. Five findings were rated high, and all five are fixed on the
  branches.
- **Red-first held in every lane.** Each builder recorded a red step (unresolved imports, a panic, a
  timeout, a failing ACL resolver test). The reviewers' fixes were red-first too, and the reports say
  so per fix.
- **TLC caught a design gap that tests and fuzz missed.** In S2.1 the first draft of the folder-grant
  check let one kind of grant widen another; the model checker found it, the builder wrote a red
  regression test and fixed it with a two-phase check before the first commit. In S6.4 writing the
  model showed that "never opens" is weaker than "never signs", which led to revoking open ceremonies.
- **The stacks needed almost no integration work.** Runtime: six merge commits, zero fixes, and the
  stack-top count equals the sum of the reviewed lane deltas exactly (+355). Core: seven merge commits
  and one integration fix. All merges auto-resolved textually; the stacking lanes read the merged
  diffs anyway.
- **CI was spent once.** No branch had a PR until the stacking step. Core CI ran once per PR and came
  back 4/4 green on all eight.
- **Defaults that change nothing for members.** Every new runtime crate is unwired or behind an env
  flag that is off by default (MCP, toolchain), the faucet ADR is `proposed` and its feature off by
  default, and the deploy gate refuses every deploy until something feeds it real evidence. The PRs
  say this in plain words.
- **Real tools where they existed.** forge, slither and anvil fixtures were captured from real runs,
  ABI selectors were pinned against `cast`, three calldata builders were proven once on a local anvil
  chain, and the template gate rendered all six templates and ran forge on each.

## What went wrong

- **A pushed branch that did not build.** The S6.4 builder's counts were real in its own worktree, but
  three fixtures it loaded at compile time were never committed because the repo ignores `*.log`. A
  clean checkout of the pushed head failed to compile. The reviewer caught it by working from a fresh
  worktree. Lesson: a count is only evidence if it comes from the pushed tree. (A30)
- **Public text went past the public-text rule in three lanes.** Two runtime/core docs and one set of
  skill files said more than a public repo should. Reviewers reworded all three in the tree, but the
  earlier commits stay in the per-commit history of those PRs. Rewriting history needs explicit
  authorization (Rule 10), so it is an owner decision. (A23)
- **Two reviewers could not re-run the full workspace.** Free disk was under 7 GB, close to the 6 GB
  stop line. Their workspace numbers were computed, not measured. The stack-top runs did measure
  everything afterwards, so the gap is closed for the merged tree, but not per branch.
- **The same parser was written twice.** The runtime S6.3 lane and the core S6.4 lane each wrote
  their own forge / slither SARIF / Medusa parsers, and each independently discovered that slither
  puts the real severity outside the SARIF `level`. Two sources of truth for a gate verdict is a Rule 9
  problem waiting to diverge. (A27)
- **Much of the night is unwired.** Seven of fifteen WPs are implemented but not wired into a running
  path, and two more sit behind flags that are off by default. That is what the briefs asked for, and it means "runtime-proven" is rare below.
- **A semantic conflict only the stack could see.** The S5.4 app-command ACL lists every registered
  Tauri command, and a test enforces it. Two other core lanes added four commands. Each branch was
  green alone; the stacked tree was not until e71639f added the four. Every future command now has to
  be added in two places. (A24)
- **Flaky tests under load.** The machine load average reached about 32. Tests that failed once and
  passed on rerun: the kit keychain round-trip (shared keychain entry across lanes), a loopback ureq
  refusal test, two kit supervisor timing tests, an agent stub start test, and a vitest file-load
  timeout. None are in files these branches touch. (A31)
- **Tools we did not have.** aderyn and medusa are not installed, so their parsers in both repos are
  tested only against fixtures built from each tool's source, and the Medusa budgets were never run.

## What surprised us

- slither's SARIF marks every result `warning` and carries severity only in `security-severity` and
  the rule-id prefix; aderyn does the opposite. A parser that trusts `level` would pass a high finding.
- The MCP 2026-07-28 revision removes `initialize`, which the WP text asked for, while every Citrate MCP
  server today still uses the handshake. No local copy of the new spec exists, so it was not built
  from memory.
- Tauri 2 lets every local-content window call every app command until the app defines its own command
  ACL; a capability file alone does not restrict app commands.
- ureq 3.3 strips `Authorization` on every redirect by default, including same-origin ones, so the HF
  token path needed a manual redirect follower anyway.
- A TLA+ model that treats a conflict as given ground truth proves the guard but not the code that
  computes the conflict; the S3.4 contradicting-memories bug was invisible to TLC for exactly that
  reason.
- The bundled llama runtime ships each dylib three times as separately signed files; symlinks could
  save roughly 40 MB of the installer.
- With an upper-bound KV estimate, a 27B hybrid-attention model gets only 16k context on a 32 GB Mac.
- The sidecar's per-session stop switch is one-way, so the first real Stop button exposed it.
- citrate-docs drifts from code on the 0x0110 output layout, "weighted mean" vs weighted sum, and
  0x0111 being marked future work while live in code. (A34)

## Process notes

- **Merge-commit stacking.** Each repo's stacking lane fast-forwarded local refs to origin first
  (several were behind the reviewers' fixes), then merged each reviewed branch into the next with a
  signed `--no-ff` merge commit, so every PR's diff against its base is that lane's work plus one merge.
  No rebase, no reset, no force-push; every push was a fast-forward. Order: new-crate lanes first, then
  the lane that adds public API to a shared crate, then the lanes that edit shared sidecar or app
  files. Merging bottom-up with merge commits keeps each PR's CI to one run (retargeting after a merge
  may re-run it, depending on repo settings).
- **CI once.** Builders and reviewers pushed branches only. That worked and cost nothing until the end.
  The trade-off: no lane saw Linux CI during the night, so anything platform-specific (the S2.1 fuzz
  is unix-only and slow in debug builds) first meets CI in the PRs.
- **Shared target dirs.** One cargo target dir per repo, shared by every lane, kept disk use survivable
  but produced the night's most common false failure: test binaries or rlibs built from another lane's
  worktree. Seen as 33 agent-core capsule tests failing on another worktree's path (four times),
  `E0432` for a module another lane had just rebuilt without, a doctest unable to find a new crate,
  and tauri-build `OUT_DIR`s holding another lane's capabilities. The stale-artifact rule (touch every
  `lib.rs` and `main.rs`, rerun) cleared each case. The S5.4 builder moved its ACL test off `OUT_DIR`
  for this reason.
- **Disk.** About 32 GB free at start, 6.6 GB at the low point, back to 11 to 12 GB after the runtime
  stacking lane deleted only its incremental cache and turned incremental builds off. No lane crossed
  the 6 GB stop line. Two reviewers chose not to run a full workspace build near the line, which was
  the right call.
- **Private findings stayed private.** Lane reports carried a separate private field; this run's
  public text describes fixes neutrally, and the specifics went to one private tracker issue.

## Actions

| # | Action | Owner |
|---|---|---|
| A23 | Merge the stacks bottom up with merge commits (runtime #23 to #29, core #139 to #146). Decide the merge method for the three PRs whose early commits carry reworded text (details in the private tracker) | Larry (owner) |
| A24 | Owner decisions listed in each PR body, notably: read and write grants strictly separate; checkpoint caps (512 MiB / 64 MiB); MCP write tools off by default and annotation trust; toolchain tools without per-run HIC until the OS sandbox lands; the anchored value is a domain-separated commitment; metering opt-in encoding; learning conflict policy; Medusa budgets and Governor without a timelock; the app-command ACL maintenance cost; whether Stop closes open approval cards; faucet ADR O-1 to O-4 | Larry (owner) |
| A25 | T1 security review of `hf_auth` / `sealed_access_token`, the kit `derive_passphrase_key` entry point, and the deploy gate | T1 security reviewer |
| A26 | Runtime: reset a session's stop switch at the start of each turn (unless the global e-stop is on); core: a session-close command for the existing sidecar route | runtime WP, then core WP |
| A27 | One source of truth for toolchain verdicts: core's deploy gate should consume the runtime S6.3 envelopes (or the reverse), not keep a second parser; then wire S6.3 → `deploy_gate_submit` and flip `gates.yaml` g3-gate after an end-to-end injected-bug run | S6.4 / S6.3 follow-up WP |
| A28 | Wiring WPs: folder grants into sidecar sessions and file tools (S2.5), checkpoints into the file tools (deny check before snapshot), learning routes, metering and trajectory sinks, the anchor batch through the core ceremony, MCP status UI | runtime and core WPs |
| A29 | Bundle aderyn and medusa (S6.1) and replace the hand-written fixtures in both repos with captured runs; run the Medusa budgets on real hardware | S6.1 WP |
| A30 | Next fan-out brief: a builder reports counts only from a clean checkout of its pushed head (`git status --ignored` clean, or a fresh worktree) | orchestrator |
| A31 | De-flake under parallel load: give the kit keychain test a per-process entry name; widen or restructure the loopback and supervisor timing tests | core WP |
| A32 | Watch the runtime PR CI (rust and audit jobs were pending); in particular the S2.1 traversal fuzz runtime on Linux debug builds | agent |
| A33 | Doc pass: `EVIDENCE.md` g1-downloads still says "Open: S0.3b"; pop-out docs say 173 commands, the stacked tree has 177 | agent |
| A34 | File the citrate-docs drift (0x0110 output layout, weighted sum vs mean, 0x0111 status) and the stale `core/learning/ARCHITECTURE.md` as errata | agent |
| A35 | Hands-on QA that no lane could do: hello-mint in a browser against an anvil fork; a real gated HF repo; pop-out click-through in the packaged app; the -ngl path on Linux NVIDIA (📡 DGX team) | QA, DGX team |
| A36 | Private follow-ups from the lane reports, tracked in the private federation tracker | Larry (owner), chain team |

## Claim honesty

| Claim | Implemented | Wired | Runtime-proven |
|---|---|---|---|
| Folder grants (S2.1-rt) | yes | **no** | tests, traversal fuzz on macOS APFS, TLC; no Windows |
| Undo checkpoints (S2.9-rt) | yes | **no** | tests on a real filesystem and real git; Windows symlink restore untested |
| Verified self-learning (S3.4-rt) | yes | **no** | tests, TLC; registerSkill calldata accepted once on a local anvil chain |
| Metering + trajectory export (S7.5-rt, S9.3) | yes | **no** | tests drive the real `run_turn` / `run_workflow`; benchmark calldata pinned to `cast` |
| Nightly anchor batch (S7.3-rt) | yes | **no** | tests, TLC; calldata accepted once on a local anvil chain |
| MCP host (S4.1) | yes | yes, behind `CITRATE_HERMES_MCP` (unset by default) | tests against a real stdio server and a real HTTP server; no real third-party MCP server |
| Toolchain verifiers (S6.3) | yes | yes, behind `CITRATE_HERMES_TOOLCHAIN=1` (off by default); core does not set it | live forge and slither tests pass by hand; aderyn and medusa never run |
| Size budget + manual eval workflow (S11.0, S11.2) | yes | size gate is a manual release step; eval workflow dispatch-only | size gate run once on a real local v0.4.2 bundle; eval workflow never dispatched |
| Contract templates + Medusa budgets (S6.2, S6.9) | yes | **no** (renderer not in the app) | forge build/test on all 6 rendered templates; hello-mint app type-checks and builds; never run in a browser; Medusa never run |
| HF token on downloads (S0.3b) | yes | yes | real loopback servers on different origins; no live gated repo |
| Tier drives serve (S1.6-rest) | yes | yes | real GGUF headers read on one 32 GB Mac; no Linux or Windows |
| Encrypted journal export (S10.4) | yes | yes | Rust and component tests; native dialogs not tried in the packaged app |
| D-4 deploy gate (S6.4) | yes | yes, and it refuses every deploy because nothing submits evidence yet | tests, TLC; aderyn and Medusa parsers only on hand-written fixtures |
| Pop-outs + Activity monitor (S5.4, S7.6) | yes | yes | unit, component and Tauri ACL-resolver tests; not launched in the packaged app |
| Faucet for deploy gas (S6.8) | ADR only, `proposed` | n/a | gas table measured read-only on 40204 plus a local fork |
| Literacy pack (S7.7) | skills + 30 QA items | **no** (skills not bundled) | citations checked live at pinned commits; QA set never run against a model |

Nothing in this table is merged, and none of it is in a shipped binary. The runtime column is mostly
tests, model checks and a few one-off local-chain proofs; the first member-facing proof for any of it
is still ahead.
