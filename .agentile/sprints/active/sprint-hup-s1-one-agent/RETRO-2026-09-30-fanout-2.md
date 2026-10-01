---
created: 2026-09-30T23:59:00Z
branch: hup/s1-one-agent
author: Claude Opus 5.5 (Claude Code), directed by Larry Klosowski (@SaulBuilds)
status: active
type: retrospective
sprint: HUP-S1
planset: 2026-09-30-hermes-upskill
---

# Retro: HUP-S1, the second 2026-09-30 fan-out

This is a mid-sprint retro, not the sprint close. Nothing in it is merged. It follows
[RETRO-2026-09-30-fanout.md](RETRO-2026-09-30-fanout.md) (fan-out 1), and the action numbering
continues from that file's A11.

The run had six work packages, each with a builder agent and then an adversarial reviewer. A seventh
lane ran the eval suites against real local models. I orchestrated it. Numbers below come from the
builder and reviewer reports. I checked that every PR head matches the reviewer's last fix commit
(`gh pr view`, 2026-09-30 late). I did not re-run the test suites myself.

## Between fan-outs

- The S1.9 review's high-severity seam finding was fixed at the loop layer in runtime#20 and filed
  privately. Its core half became the A5-core lane below.
- New wasmtime advisories turned `cargo audit` red on every runtime PR. runtime#21 bumps wasmtime
  46.0.3 → 49.0.1 with no source changes: 626 tests, audit exit 0 on a fresh advisory DB, CI green.
- runtime#18's traversal fuzz failed on Linux CI. Its vacuity minimums had been calibrated on
  case-insensitive APFS. It now batches deterministically until the minimums hold (cap 30,000 bases).
  I reproduced it on a case-sensitive APFS volume, and the Linux rust job is green.
- The playbook page counted only merged items, so it showed no progress. It now shows "% built"
  (in review or done) beside "% merged".
- v0.4.1 turned out to be already published on GitHub with Linux x86_64 and aarch64 assets.
- **Disk reached 2 GB free.** The shared target dirs had grown to 26 GB and 19 GB, anvil temp held
  14 GB, and there was old release scratch. I cleaned it to 45 GB before this run.

## Outcome

| WP | Repo / PR | Base | Tests before → after (after review fixes) | Review verdict |
|---|---|---|---|---|
| A8 + S2.4 tool annotations + approval cards | core#128 | hup/s1-one-agent | vitest 573 → 611; src-tauri `cargo test --lib` 482 → 487 (5 ignored); tripwire 3/3; clippy clean | pass-after-fixes |
| A5-core seam hardening | core#125 | hup/s1-one-agent | vitest 573 → 587 | pass-after-fixes (2 medium fixed) |
| S2.6 decision records | runtime#22 | main | workspace 626 → 670 (crate 44) | pass-after-fixes (2 medium fixed) |
| S3.5 QA eval set v1 | core#127 | release/0.5.0-hermes-upskill | vitest 568 → 600 + 2 skipped (602 with sources) | pass-after-fixes (1 medium fixed) |
| S3.6 third-party skill intake | core#126 | release/0.5.0-hermes-upskill | vitest 568 → 585 + 1 skipped (586 with sources) | pass-after-fixes (1 medium fixed, 3 medium open as follow-ups) |
| S2.3 formal: `WebSigningBudget.tla` | core#129 | hup/s2-rule3-adr | 5 TLC configs green (largest 95,058,700 distinct, depth 31); mutants 22/22 → 23/23 | pass-after-fixes (1 medium fixed) |
| A7 real-model eval | core#120 @ 1d839bb | (existing PR) | results committed under `eval/results/` | not reviewed by a second agent |

Counts are per branch and do not add up. The `cargo test --lib` figure for core#128 measures something
different from the sprint's 688 workspace baseline, so the two can't be compared.

### The first real-model numbers (A7)

Machine: Apple M2 Max, 32 GB, macOS 15.6.1. The app's own llama-server was also running (about 4.7 GB
RSS), and swap use was heavy (28-38 GB). The eval used the app's bundled llama-server binary as a
separate instance on loopback. One run per model, temperature 0.

| Tier | Model | Valid tool call | Correct tool | Args OK | Injection resist | Latency mean / p95 | Wall |
|---|---|---|---|---|---|---|---|
| T0 | gemma-4-E4B-it Q4_0 | 100% | 98.2% (56/57) | 95.0% | 23/23 | 3.7 s / 8.6 s | 5 min |
| T1 | Qwen3.8-27B Q4_0 | 100% | 87.7% (50/57) | 92.5% | 23/23 | 19.4 s / 45.7 s | 28 min |

Caveats that change how these numbers can be used:

- **The T1 pass needed `--reasoning-budget 1024`, which the app does not ship.** With the app's exact
  flags, thinking was unbounded. The run aborted at item 30, where one turn took more than 180 s. By
  then 27 of 29 items had passed.
- **G1-eval is met only on the weaker measure.** `validToolCallRate` checks only that emitted calls are
  schema-valid. T1's correct-tool rate is 87.7%, under the 90% bar.
- Qwen3.8-27B stood in for the planset's named T1 default (Qwen 3.6), which was not on disk. T2 was
  not tested. Throughput is a lower bound because of memory contention.
- Several T1 "wrong tool" results were the model reading before acting (for example `skills_list`
  before `skill_run`). A single-turn eval scores that as wrong; the S6 multi-turn re-score should
  decide whether it really is.
- The S3.5 QA set (core#127) has not been run against any model yet.

## What went well

- **Every reviewer found something real, and every review ended in pass-after-fixes with signed fix
  commits.** Each reviewer broke guards on purpose and confirmed that a test failed every time.
  - S2.6: two pruning edge cases, one in the verifier and one in crash recovery, in a crate that is
    not wired yet. Both are fixed with regression tests (d529b13).
  - S2.3: the SIWE rolling-rate guard could be deleted without TLC noticing, because the bounds made
    the cap unreachable. A third nonce and mutant M23 make it bite.
  - S3.5: substring matching let short key points like "3" or "18" match inside longer tokens. Matching
    now uses token boundaries.
  - S3.6: the reviewer read the shipped body of one skill whose automatic verdict said it did not depend
    on its stripped scripts. It does. The note is corrected and the decision goes to the owner.
  - A5-core: non-string arguments were turned into `{}` and run, and an in-flight id could be held
    across turns. Both are fixed.
  - A8 + S2.4: added a tripwire that only the wallet review's buttons may call its resolvers.
- **Vacuity got caught twice in formal work.** Fan-out 1 hit it in S2.7, and this time both the S2.3
  builder (M09) and its reviewer (the rate guard) found mutants that survived only because the bounds
  made the bad state unreachable. Lesson: a cap is only checked if the model can reach one past it.
- **A shallow stack, on purpose.** runtime#22 is based on main. #126 and #127 are based on the release
  branch. Only #125 and #128 sit on hup/s1-one-agent, and #129 sits on #122's branch. The runtime
  stack did not get deeper.
- **We have real-model numbers for the first time,** with honest limits written into
  `eval/results/README.md`.
- **The disclosures held up.** "Not wired", "not run against a model", "`hicAware` off", "the bundler
  does not read the lock yet": each PR says what it is not.

## What went wrong

- **Claims a little ahead of the code.** core#128 says a `hic:"required"` call resolves only through a
  click. For `contract_deploy` the proposal returns to the sidecar at once, and only the signature
  waits for the click. The safety property holds (nothing signs without a click), but the text claims
  more than the code does. Not yet fixed.
- **One automatic verdict was wrong.** S3.6 gives "include, scripts stripped" by default when a skill
  has no red flags. Nothing makes a person confirm the skill still works without its scripts. A
  recorded decision is also not tied to the SKILL.md hash it reviewed.
- **The T1 shipping configuration fails the eval as shipped.** This is a product finding, not a test
  finding: unbounded thinking on a 27B dense model does not finish on a 32 GB Mac inside 180 s.
- **The cache request was fanned out to every lane.** The relayed request "clear cache and .anvil
  files and make room" reached all lanes, and each one decided for itself:
  - Four lanes deleted something: the VS Code updater cache, `~/.foundry/anvil/tmp`, Chrome HTTP
    caches (~1.8 GB), the pip cache (~180 MB), and npm/go-build/uv/Homebrew/electron caches plus a
    pnpm store prune.
  - The others declined, correctly, because shared caches and target dirs were in use.

  No `.anvil` files existed anywhere, and `~/.foundry/anvil` was empty in every lane's check. The
  14 GB of anvil temp had already gone in my pre-run cleanup. It worked out, but parallel deletion in
  shared space is the wrong shape. One lane, or the orchestrator, should own a housekeeping request.
- **Disk still dipped hard.** Free space fell from 45 GB to 31 GB during the S2.3 x402 TLC run (95M
  distinct states, using TLC's on-disk queue) while other lanes built. It was 47 GB at close.

## What surprised us

- The sidecar names id-less model calls `call_0`, `call_1`, … per reply, so call ids repeat across
  steps. Deduplicating by "id already handled" would silently drop real calls. The A5-core builder
  scoped the guard to calls still in flight and drops exact replays by seq instead.
- The loop dispatches any valid JSON as arguments, including arrays, strings and null. Core's
  non-object guard is what keeps those away from the handlers.
- `WalletReviewModal` never rendered `spendSummary`. The chain card now fills that gap.
- All 174 hermes-fork skills fail the strict S3.2 loader, mostly on a nested metadata map.
- The vendored Trail of Bits tree had no recorded upstream commit. The S3.6 builder recovered it
  (a56045e9) by comparing blobs.
- The public docs contradict themselves in three places (finality depth, Q16.16 width, devnet chain
  id). S3.5 wrote no questions on those points.
- A stray `refs/remotes/origin/hup` ref in the citrate-core clone blocks every fetch of
  `origin/hup/*`. Three lanes worked around it and none removed it, which is the right call for a
  shared clone.

## Process notes

- **The stale-artifact rule: no false failures this time.** Both Rust lanes (core#128 and runtime#22)
  and their reviewers touched every `src/lib.rs` before recording counts, and no failure named another
  worktree's path. That is weaker evidence than it sounds, because only two lanes touched cargo
  (fan-out 1 had five). Keep the rule.
- **Slug-prefixed scratch files: no cross-lane collisions.** The one lost backup (S3.6's mutation
  check) was a lane overwriting its own file. The S3.6 builder restored the three mutated lines by hand
  and the tests passed. One reviewer saw another lane's `tlc-taint*` directories still in the
  scratchpad and left them alone.
- **Stack depth:** see "What went well". Open merge-order notes:
  - core#129 and core#119 both add `scripts/run-tlc.sh` and append to the formal README, so expect a
    trivial add/add conflict.
  - core#127's `qaCliArgs.ts` duplicates #120's `cliArgs.ts`; merge them once both land.
  - runtime#21 should land before anything that needs a green audit.
- **TLC needs a disk budget.** Large configs belong in the brief with an expected state count, and
  CI should get a smaller x402 bound (the builder suggested `MaxReq 3`).
- **An eval lane without a reviewer is a weaker kind of evidence.** The numbers are reproducible from
  the committed command and flags, but no second agent re-ran them.

## Actions

| # | Action | Owner |
|---|---|---|
| A12 | Decide when core sends `hicAware: true` (core#128 has the path ready); confirm trust labels, including agent-written journal/memory content coming back as trusted; `contract_deploy` as sign; one-click overwrite on `skill_write` | Larry (owner) |
| A13 | Correct core#128's PR text: for `contract_deploy` the proposal returns at once and only the signature is held | agent |
| A14 | T1 serving: a per-tier `--reasoning-budget` in serve.rs, or a longer eval timeout; and decide whether G1-eval is judged on valid-call rate or correct-tool rate (T1 87.7%) | owner decision, then core WP |
| A15 | Eval follow-ups: record latency and tokens/sec in the Scorecard; run T2 and the named Qwen 3.6 T1 default; repeat runs for variance; run the S3.5 QA set; set the AC2 pass-rate target | agent on a real machine; owner for the target |
| A16 | S2.6 decisions: records directory, segment size, retention; fail closed vs quarantine-and-new-chain; a local device-key MAC before S7.3 anchoring. Docs: state that front truncation via a forged checkpoint is not detected locally. Then the wiring WP | owner, then rt WP |
| A17 | Fold S2.3 ambiguities A-1 to A-8 into an ADR amendment (nonce ledger retention, re-read before signing, atomic reservation and record, cross-generation windows, grant takes the lock) | @rule8 reviewer + federation lead |
| A18 | S3.6 owner calls: hermes frontmatter (widen the loader or rewrite at intake); a LICENSE file for frontend-skills; ToB CC BY-SA attribution; confirm the six merit exclusions and the corrected skill. Follow-ups: bind decisions to the SKILL.md hash; require a decision for every stripped verdict; bundler copies exactly the lock's refs | owner, then core WP |
| A19 | A5-core follow-ups: `lastSeq` advances before a page is processed; a `tool_call` with no call object throws; an end-to-end test through the real `store.handleTool` | core WP |
| A20 | File the three public-doc contradictions as citrate-docs errata | agent |
| A21 | Prune the stray `refs/remotes/origin/hup` ref in the citrate-core clone | Larry (human, shared clone) |
| A22 | Next fan-out brief: send user housekeeping requests to one owner, not to every lane; give TLC runs a disk budget; give the eval lane a reviewer | orchestrator |

A5 from fan-out 1 is now done on the core side (core#125), pending review and merge. A7 is partly
done (see A15). A8 is in review (core#128); `hicAware` is A12.

## Claim honesty

Fan-out 1's table still applies. These rows are new or changed.

| Claim | Implemented | Wired | Runtime-proven |
|---|---|---|---|
| Tool annotations + approval cards (A8 + S2.4) | yes | annotations sent on the sidecar session; cards in the existing ceremony and wallet review; `hicAware` **off**, so the core HIC path runs only in tests | component and render tests; no packaged-app click-through |
| Seam hardening (A5-core) | yes | yes, in the sidecar provider | recorded-loop fixture from a scripted model; no live llama-server session (A6) |
| Decision records (S2.6) | yes | **no** | tests, 29 mutants, crash-recovery tests; no Windows |
| Web signing budgets, formal (S2.3) | model only | n/a | TLC, 5 configs, 23/23 mutants; no code exists |
| QA eval set (S3.5) | yes | n/a | provenance checked against pinned public commits; **never run against a model** |
| Skill intake (S3.6) | yes | **no**: the bundler does not read `skills.lock` | loader parity on 296 of 296 skills against the Rust loader |
| Tool-call + injection eval (S1.7 / S1.10) | yes | n/a | **yes, first real-model run**: T0 98.2% correct tool, T1 87.7% (with a non-shipping reasoning budget), injection 23/23 on both; one machine, one run each |

The eval row is the first runtime-proven number in this sprint, and it is narrow: one Mac, one run per
model, a stand-in T1 model, and a flag the app does not ship. Nothing here is merged, and none of it
is live in a shipped binary.
