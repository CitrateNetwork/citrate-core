---
created: 2026-09-30T23:00:00Z
branch: hup/s1-one-agent
author: Claude Opus 5.5 (Claude Code), directed by Larry Klosowski (@SaulBuilds)
status: active
type: retrospective
sprint: HUP-S1
planset: 2026-09-30-hermes-upskill
---

# Retro: HUP-S1, the 2026-09-30 parallel fan-out

This is a mid-sprint retro, not the sprint close. HUP-S1 is still open, and nothing below has been merged.
It covers one day. Before the fan-out, I did the sequential S1 work myself in this session. Then I ran
seven parallel work packages. Each one had a builder agent, followed by an adversarial reviewer agent
that re-ran the gates and pushed its own fixes. I am writing as the orchestrating agent. I did not
re-run every reviewer's numbers myself. Where a number below comes from a builder or reviewer report,
this file says so.

## Outcome

### Before the fan-out (same session, sequential)

| WP | PR | Result |
|---|---|---|
| S1.1a/b/c loop into the sidecar | runtime#10, core#119 | Loop crate, sidecar sessions with long-poll, core session commands behind a preview toggle. ADR loop-in-sidecar with the long-poll amendment. TLA+ `AgentLoop` TLC-green at 1,258 states. |
| S1.2 tool retrieval + context budget | runtime#11 | Keyword top-K. The BGE retriever is not built yet. |
| S1.3 verifier-judged workflows | runtime#12 | Planner/executor + verifier framework. |
| S1.7 / S1.10 eval suites | core#120 | 57 tool-call tasks + 23 injection cases. **Not yet run against a real model.** |
| S1.8 `citrate-agent hermes` CLI | runtime#13 | The end-to-end smoke found a sidecar panic (see below). |
| S1.4 interviewer, runtime half | runtime#14 | 5 bundled tracks, a brief whose gates can't be edited away, and the workflow status "not available yet (ships in S6)". |
| S1.6 hardware tiering | core#121 | Background agent. 7 GiB node reserve. Verified live on one 32 GB Mac only. |
| S1.11 WP plan | #118 | Merged into the integration branch. Unlink itself is blocked (identity#31, fed#293). |

Runtime 660 → 680 over the day before the fan-out.

### The fan-out

| WP | Repo / PR | Base | Tests before → after (final, after review fixes) | Review verdict |
|---|---|---|---|---|
| S1.4 core (InterviewCard, brief commands) | core#123 | hup/s1-one-agent | cargo 694 → 705; vitest 573 → 587; tsc, clippy, tripwire 3/3 clean | pass-after-fixes |
| S1.9 parity suite (parity half) | runtime#15, core#124 | hup/s1-interviewer / hup/s1-one-agent | runtime 680 → 685; vitest 573 → 598 | pass-after-fixes; one HIGH finding open (pre-existing, not in diff) |
| S2.0 Rule-3 budgetable-signatures ADR | core#122 | release/0.5.0-hermes-upskill | n/a (doc) | pass-after-fixes (6 corrections pushed) |
| S2.2 shell allowlist runner | runtime#19 | hup/s1-interviewer | 680 → 714 (crate: 34 tests) | pass-after-fixes |
| S2.7 taint + HIC downgrade + TLA+ | runtime#16 | hup/s1-interviewer | 680 → 699; TLC 21,900 states / 120 distinct (and 80,358 / 399 at larger bounds) | pass-after-fixes |
| S2.8 default-deny path guard | runtime#18 | hup/s1-interviewer | 680 → 718 builder run (guard crate 38 → 39 after review) | pass-after-fixes |
| S3.2 SKILL.md loader + `skill_load` | runtime#17 | hup/s1-interviewer | 680 → 714 | pass-after-fixes |

The runtime counts are **per branch**, each measured from 680. They do not add up, and no merged
total exists yet. All 7 reviews ended in pass-after-fixes, and every reviewer pushed at least one
signed commit. None of the reviews was a rubber stamp.

## What went well

- **Red-first held in every lane.** Each builder recorded a red step: compile failures, an allow-all
  skeleton giving 31 red, or a `__PIN__` placeholder failing the sha pin. Most lanes also mutation-tested.
- **Most of the value came from the reviewers.** They checked the tests by breaking the code on purpose,
  and found surviving mutants in three lanes: the GET transport in S1.4, the read-time containment check
  in S3.2, and the TLA+ properties in S2.7, which the builder caught in its own pass. Each surviving mutant
  became a new test. In two lanes (S2.2, S2.8) the reviewer ran the real system binaries and the real
  filesystem rather than reading the code, and found problems the unit tests could not show.
- **The work was honest about what it is not.** The interview card shows the workflow as "not available
  yet". The skills loader is off by default. The taint layer declines rather than guesses when core cannot
  honour explicit approval. The parity README now says "loop parity, not end-to-end parity". The ADR is
  `proposed` with an empty sign-off block.
- **One source of truth (Rule 9) was kept on purpose.** The core card asks the sidecar for its own track
  suggestion instead of reimplementing it in TS. S2.7 refines core's existing abstract `AgentLoop.tla`
  taint part instead of duplicating it.
- **The S2.7 formal lesson.** The first `TaintDowngrade.tla` stated its properties with the same helper
  predicates as the guard, so two mutants survived TLC. Splitting ground truth from the modelled
  implementation fixed it. A property written in terms of the code's own predicate can't fail.

## What went wrong

- **The end-to-end smoke found what the unit tests missed (before the fan-out).** The sidecar built a
  blocking HTTP client inside an async handler. Creating and dropping it panicked and poisoned the
  sessions lock, so every later request failed. The unit tests used a scripted `LlmClient` and never
  touched the real client. The fix and a regression test through the real routes are in runtime#13.
  Lesson: a scripted seam proves the loop, not the process.
- **An open HIGH review finding (S1.9 review, pre-existing code).** The reviewer found a high-severity issue
  at the seam between the sidecar loop and core's sidecar provider. As a result, two of the parity suite's "Rust is correct" verdicts hold at the loop layer but not
  end to end. Details are held out of this public repo; filing them privately is part of A5. It needs one owner, sequenced
  with S2.7, which edits the same dispatch block, plus an end-to-end test of the core provider against a
  session fixture. **Until then the owner should not confirm those verdicts or retire `harness.ts`.**
- **Review fixes (neutral summaries; specifics stay private):**
  - S1.4: added the missing real-socket test for the GET transport, and corrected stale doc comments.
  - S1.9: reworded the claim that the wire runner is "the live session path". Fixed a test-helper underflow.
  - S2.0: the ADR had quietly relaxed red-team correction 3 by making a same-origin taint exemption the
    default. The strict rule now holds until the owner answers O-3. The ADR had also assumed an on-chain
    anchor-delegate/relayer route that the deployed contract doesn't support, and that is now documented
    as a hidden S7.3 prerequisite. Smaller corrections: the anchor call signature, `outcome_unknown`
    instead of `not_signed`, a dedicated budget lock, and the HIC-2 tier named explicitly.
  - S2.2: tightened the read-only git argument policy (more refused options, refused abbreviations,
    textconv off for blame). Repository-local filter drivers remain open, so git stays behind HIC-1 until
    the OS sandbox exists.
  - S2.7: `record()` and `is_tainted()` now agree when the lock is poisoned, so the guard fails closed as
    documented.
  - S2.8: root-anchored system rules now also match under a macOS volume path alias.
  - S3.2: source precedence now holds when a name is ambiguous in a higher source; symlink aliases of one
    skill dir are deduped; the read-time containment check has a test.
- **Not verified on real hardware or real models:**
  - The S1.7 / S1.10 eval suites have never run against a real model.
  - Parity is loop-only. There was no live run of the sidecar provider in the packaged app.
  - There was no manual click-through of the InterviewCard in the packaged app; coverage is component
    tests plus one real-socket transport test.
  - S1.6 tiering was verified live on one 32 GB Mac.
  - S2.8 had no Windows run; Windows paths were tested as strings on Unix.
  - S2.2 is Unix-only and has no OS sandbox.
  - The hermes binary shipped in the app has not been rebuilt from any of these branches, so nothing in
    the fan-out is live in the app.
- **Gaps that change behaviour later:**
  - Core's `AGENT_TOOLS` carry no effect/trust annotations. Once S2.7 merges, the opt-in sidecar preview
    will decline effectful calls after its first tool result until core annotates its tools and builds
    the S2.4 explicit-approval card.
  - In core#123 the accepted brief goes to the model as an assistant-role message. That can produce two
    assistant turns in a row, which some chat templates reject. This needs an owner decision.

## What surprised us

- **The shared `CARGO_TARGET_DIR` was a correctness hazard, not just a lock wait.** Four lanes (S1.9,
  S2.7, S2.8, S3.2) and three reviewers independently saw test or clippy runs link *another worktree's*
  `citrate-agent-loop` rlib. Cargo keys workspace path packages by relative path, so different worktrees
  overwrite the same artifacts and look fresh. Symptoms: "declined: untrusted content" text in a branch
  without taint, a phantom `tainted` event, and doctests failing on vanished extern locations. A further
  33 agent-core capsule failures came from test binaries that used `env!(CARGO_MANIFEST_DIR)` of a deleted
  worktree, and from worktrees outside the repo dir having no fixture ancestor. The workaround everyone
  converged on: touch your sources, then confirm a `Compiling citrate-agent-loop (<my worktree>)` line.
  **So every workspace count in this fan-out was taken under that hazard.** Where a reviewer re-ran after
  touching sources the count is credible. The S2.8 builder's 718 included 3 cross-lane false failures.
- **The shared scratchpad clobbered a file.** S2.2's `lib.rs.bak` restore pulled in S2.8's guard source,
  because both agents used the same backup filename.
- **Latent transport bug in core.** `UreqControl` turned every non-2xx response into an empty body, so no
  sidecar refusal reason ever reached the UI. S1.4 fixed it because its acceptance criterion needed it.
- **Hidden prerequisites the ADR surfaced:** x402 needs an EIP-3009-style token, but SALT is native.
  `approve()` refuses all TypedData today. And the strict taint rule makes SIWE budgets dead code on any
  real web page (O-3).
- **The TS loop is thinner than it looked.** Most of its robustness lives in `store.handleTool`, which
  quietly coerces bad arguments. Most parity "divergences" are places where the sidecar is stricter.

## Process notes on the fan-out

- **Worktrees per lane worked** for source isolation. Every lane removed its worktree, kept its branch,
  and scrubbed `x-access-token` from `.git/config` (all reported 0).
- **Shared target dir: change it next time.** Either give each lane its own `CARGO_TARGET_DIR` for the
  final workspace run (disk permitting; about 30 GB free today), or make touch-and-verify a written step
  in the brief. Take the counts of record from in-repo checkouts, not from worktrees outside the repo.
- **Scratchpad hygiene:** the brief must tell agents to prefix every scratch file with their WP slug.
- **Stacked PR depth is the main merge-order risk.** The runtime stack is
  `#10 ← #11 ← #12 ← #13 ← #14`, and #15–#19 all sit on `hup/s1-interviewer` (#14's head). That makes 10
  open runtime PRs, with up to six levels below the newest. Specific risks:
  - S3.2 added `TurnOptions.pinned_tools`, so any branch that builds `TurnOptions` without
    `..Default::default()` fails to compile after merge.
  - S2.7 and the S1.9 HIGH fix touch the same dispatch block in `run_turn`.
  - The parity suite has to be re-run after S2.7 lands.

  Suggested merge order: #10–#14 in stack order, then #18 and #19 (standalone crates), then #17, then
  #16, then re-run #15 against the result.
- **Builder → adversarial reviewer per lane is worth its cost.** I would keep it.

## Actions

| # | Action | Owner |
|---|---|---|
| A1 | Review and merge the runtime stack #10–#14, then #15–#19 in the order above; core #119–#124 | Larry (owner) |
| A2 | Pick persona names for the tracks/brief surfaces | Larry (owner) |
| A3 | @rule8 sign-off on the S2.0 ADR (core#122): answer O-1 to O-5. O-3 decides whether SIWE budgets are usable at all | @rule8 reviewer + federation lead |
| A4 | Rebuild the hermes binary from the runtime stack after merge; until then nothing here is live in the app | Larry / DGX for Linux |
| A5 | File the S1.9 HIGH finding privately (security repo), then fix it with an end-to-end core-provider test, sequenced with S2.7 | next runtime+core WP (agent) |
| A6 | Live parity run on the packaged app before retiring `harness.ts`; decide turn cap (6 vs 8) and context freshness | agent run, owner decision |
| A7 | Run S1.7 / S1.10 eval + injection suites against a real local model | agent, on a real machine |
| A8 | Core follow-ups: annotate `AGENT_TOOLS` (effect/trust), S2.4 explicit-approval card, then `hicAware:true`; the brief's message role | core WP + owner decision |
| A9 | Wiring WPs: guard into fs/shell/capsules, shell into the sidecar as `shell_run`, skills env var + core skill migration | S2.x / S3.x |
| A10 | Next fan-out brief: per-lane target dir or touch-and-verify; slug-prefixed scratch files | orchestrator |
| A11 | Reconcile planset text with the ADR (`OnlySiwe` → `OnlyClosedList`; the EIP-712 wording) | retro owner at sprint close |

## Claim honesty

| Claim | Implemented | Wired | Runtime-proven |
|---|---|---|---|
| Loop runs in the sidecar (S1.1) | yes | yes, behind the opt-in preview | partly: CLI + real sidecar binary + scripted model smoke; never with a real model in the packaged app |
| Tool retrieval (S1.2) | keyword yes; BGE no | yes, in the loop | tests only |
| Verifier workflows (S1.3) | yes | loop-level | tests + TLC |
| Interviewer + brief (S1.4) | yes, both halves | core card → sidecar `/tracks`, `/briefs` | tests + one real-socket transport test; no packaged-app run; nothing is built from a brief |
| Hardware tiering (S1.6) | yes | onboarding only; does not drive serve.rs | one 32 GB Mac |
| Eval + injection suites (S1.7/S1.10) | yes | n/a | **never run against a real model** |
| CLI (S1.8) | yes | yes | end-to-end smoke with a scripted model |
| Parity (S1.9) | 21 scenarios, both repos | n/a | loop parity only; end-to-end parity **not** proven (open HIGH) |
| Rule-3 budgets (S2.0) | ADR draft only | no | no |
| Shell runner (S2.2) | yes | no | real binaries in tests; no sandbox |
| Taint downgrade (S2.7) | yes | sidecar yes; core no (no annotations, `hicAware` off) | tests + TLC |
| Default-deny guard (S2.8) | yes | no | real filesystem tests + fuzz on macOS; no Windows |
| Skill loader (S3.2) | yes | sidecar, off by default | tests + an uncommitted smoke run over 86 real skills |

Nothing above is "ready". None of it is merged, and none of it is live in a shipped binary.
