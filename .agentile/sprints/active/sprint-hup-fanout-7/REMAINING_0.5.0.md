---
created: 2026-10-04T23:58:00Z
branch: docs/hup-fanout-7
author: Larry Klosowski + Claude Opus 5.5
status: active
type: remaining-work
planset: 2026-09-30-hermes-upskill
baseline: fan-out 7 rescope (106 items), updated by fan-out 7 builder, reviewer and stacking reports
---

# What is left for v0.5.0 after fan-out 7

Every one of the 106 playbook items appears here once. The baseline is the fan-out 7 rescope; each
row is updated from the fan-out 7 builder, reviewer and stacking reports, using the reviewer's
wording where it narrowed a claim. I did not re-run tests for this file. The retro is
[RETRO-2026-10-04-b.md](RETRO-2026-10-04-b.md). Security specifics live on private
citrate-federation#298, not here.

## Status words

| Word | Meaning |
|---|---|
| **merge** | Built and reviewed on an open PR; merging it (in stack order) is the next step. |
| **hold** | Built, but must not merge until a named external step happens. |
| **owner** | Needs Larry's decision. The code already ships the recommended default, marked "pending owner sign-off", and changes nothing risky for members. |
| **external** | Needs a named team: 📡 DGX team (Linux, Windows, GPU, release machine), chain operator (40204, after owner sign-off), or the @rule8 key holder. |
| **QA** | Needs a person at the packaged app. Agents may not sign or approve. |
| **buildable** | An agent can do it. "(after restart)" means the release Mac's GPU must be freed first; "(after merge)" means it runs on the merged heads. |
| **decided** | Owner decision is in; only an optional sign-off remains. |

A row can carry several words. PR numbers are in CitrateNetwork repos: core = citrate-core, runtime =
citrate-agent-runtime, chain = citrate-chain, docs = citrate-docs.

## Summary

93 open rows and 13 done or obsolete. Rows carrying each word (they overlap): merge 32, hold 1, owner 65, external 51, QA 21, buildable 13, decided 1.

The short version: almost no row is waiting on agent code that can be written today. Most rows
wait on, in order: merging the fan-out 6 and 7 stacks; owner decisions (listed at the end); the
@rule8 component-key ceremony; DGX and chain-operator runs; and a person at the packaged app. The
agent work left is the final red-team after merge, the T1 evals once the GPU is freed, F-10 now
that the sizeup repo exists, and follow-ups that start after an owner decision.

## G1 (M1 = S1, plus federation rows filed with it)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| fed-F-1 | merge | stacker / owner merge | Docs requirement met. Merge chain #270 to #274 then #275, #276; merge docs #30 after the chain stack reaches main. Re-run the Rule-6 benchmark after merge if a post-merge record is wanted (the stack-head run is benchmarks/2026-10-04-agent-precompile-stack-devnet.md). | chain #273, docs #30 |
| fed-F-4 | owner, external | Larry; chain operator; 📡 DGX team | Owner: admin and timing (default CitAgentTimelock 2-of-3, 2-day delay, today's chain). DGX: make `cast receipt 0x23c717a1...27fd9 --rpc-url https://rpc.citrate.ai` print a receipt (tx and receipt lookups by hash return null today). Operator: runbook steps 0 to 5 from chain `hup/n7-registry-redeploy-prep`, including `hup-provenance-update.py ... --backfill --check` then without `--check`; commit the book. | prep: chain #275, core #222, runtime #63, #67; closes with the operator's book commit |
| fed-fed-293 | owner, external | Larry; 📡 DGX team | Fix built and reviewed, local only (citrate-identity is public). Owner: push `hup/n7-identity-unlink-reelect` @ 8870298 to a private remote or advisory fork and review it with identity#31. DGX: deploy identity#31 and the fix together, smoke test (link 2, unlink canonical, one `is_canonical` row), confirm on fed#278. | private identity PR + identity#31 |
| gate-g1-wallet-unlink | external | 📡 DGX team, then stacker | After the identity deploy: merge core #232 (it replaces core#117), re-run the unlink tests, flip the gate with the evidence. | core #232 |
| ra-1 | buildable (after restart), owner | Larry; eval agent or 📡 DGX team | Restart the release Mac (frozen llama-server pids hold about 21 GB of GPU memory). Then run eval-tools v2 and eval-sidecar for Qwen 3.6 27B and 35B-A3B per the #284 steps; owner picks T1 (default: the variant that clears 90 % valid calls and 80 % steps with the best latency). | tooling in core #217; results commit |
| ra-20 | owner | Larry | Confirm T0 is held to the T1 bars (built as the default in the scorecard, shown as met*), v2 as the default dataset, and the 80 % step bar. Note the default does not yet judge injection resistance as met or not met. | core #229 |
| rel-042 | external | 📡 DGX team | Unchanged: fed#295 Linux updater build, signing on the Mac, assets, latest.json, mirror parity. | release ops |
| story-US-1-1 | merge, owner | Larry | AC3 met on branch. AC1: decide whether gateway-provider turns move into sidecar sessions (retiring the last in-app tool loop) or accept the exception; sign off the 8-turn cap and the 8-schema ceiling now applied to gateway members; and the headless dispatcher running only `node_status` and `groups_list`. If the loop moves, that is agent work. | core #216, runtime #62 |
| story-US-1-4 | external, owner | 📡 DGX team; Larry | DGX: build `bge-base-en-v1.5-f16.gguf` with `scripts/build-bge-gguf.sh` (llama.cpp b8640, BAAI rev a5beb1e3), `gh release upload runtime-deps ... -R CitrateNetwork/citrate-core`; the release workflow fails closed until then. T2 evals. Owner: ra-20 and the BGE pin. | core #216, #229 |
| story-US-1-5 | owner, then buildable | Larry; core agent | Owner: (b) native-SALT HIC-1 per-request approval for 0.5.0 (recommended), the 10 SALT, 4096-byte and USD-cap placeholders, and whether the InferenceRouter already in the book is the one to carry escalations (private note on #298). Then `sync-addresses.py --with-inference-router`, a Settings UI for quote, request, result and claim, and a registry metering receipt. | follow-up PR after the decision |
| wp-S1-11 | hold | stacker | Do not merge until the identity deploy above. Then merge core #232. | core #232 |

## G2 (M2 = S2, S3)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| fed-F-8 | merge, external | 📡 DGX team (A45) | After memories #17 merges: build mem-mcp `--release -p mem-mcp --features rocksdb,transformer`, upload the corpus and the import-corpus mem-mcp, pin both sha256 values in core runtime-deps.sha256. | memories #17, core #190 |
| ra-12 | owner, external | Larry; 📡 DGX team | Owner: put gradient-papers under version control (recommended: its own private repo). DGX: rebuild the corpus from clean checkouts and stage it without `--allow-dirty` (the new gate refuses dirty and unpinned sources), then the A45 upload and pin. | core #230 |
| ra-13 | merge | stacker | Merge core #190 and runtime #47. | core #190, runtime #47 |
| ra-17 | decided | Larry (optional) | Optional sign-off on persona defaults as shipped. | none |
| ra-18 | owner | Larry, @rule8 | Sign the A17 amendment `docs/adr/ADR-2026-10-04-rule3-amendment-1-budget-ambiguities.md` (status proposed). A-1 (nonce uniqueness after pruning) and A-2 (taint snapshot) are real choices; A-3 to A-8 record what the code does. | core #231 |
| ra-19 | owner, then buildable | Larry; core agent | Accept 10 sign-ins per origin with 7-day expiry and the live-path placeholders (ceilings 50 and 30 days). Then an agent renames the `PLACEHOLDER_` constants and removes the placeholder copy. | follow-up PR |
| story-US-2-3 | external, QA | chain operator; a person | AnchorRegistry on 40204 via the runbook after F-4 sign-off and O-5. QA: a packaged https dApp sign-in. | prep chain #275 |
| story-US-3-1 | merge, external, QA | stacker; 📡 DGX team; a person | Merge memories #17 and core #190; A45 upload; packaged offline first-launch QA. | memories #17, core #190 |
| story-US-3-4 | merge, owner, external | Larry; chain operator | Build-complete. Owner: turn on `SKILL_PUBLISH_ENABLED` once the #272 SkillRegistry is live on 40204 and in the book (default off). Operator: deploy it (fed #289). | core #218, runtime #63 |
| wp-S2-3 | external, owner | chain operator; Larry | As US-2.3, plus the ra-19 values. | prep chain #275 |
| wp-S3-1 | merge, owner, external | as ra-12 | As ra-12 and fed-F-8. | core #230 |
| wp-S3-4 | merge, owner, external | Larry; chain operator | As US-3.4. | core #218, runtime #63 |

## G3 (M3 = S4, S5)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| gate-g3-browser | external, QA, owner | @rule8 key holder; 📡 DGX team; a person; Larry | Component-key ceremony; host and sign the SearXNG artifact (`scripts/pack-searxng.sh --url <https> --apply`, check-bundle, sign, manifest-from-bundle) and the CfT 154.0.8037.92 zip; DGX measures CfT for linux-x64 and windows-x64 and resolves the SearXNG locks; a person runs the browser QA on #282 (screencast, attach with consent, excluded origin refused, decide report). Owner: CfT fetch vs re-host; open-web block vs warning (block is the default). | core #219, #220; runtime #64, #65 |
| gate-g3-licence | owner, external | Larry and counsel; 📡 DGX team | Owner: sign LICENCE_REVIEW.md; AGPL offer option A; upstream fetch; Apache vs BUSL for the node and sidecar binaries; frontend-skills LICENSE; offer URL; approve cargo-about plus go-licenses as the notice generator; whether the upstream URL is enough as the MPL-2.0 source pointer (14 packages). DGX at release: re-collect the notices from the bundled revisions (`third-party-notices.mjs collect/render/check`, `licence-inventory.mjs --require-notices`). | core #230 |
| gate-g3-updater | external, owner | @rule8 key holder; Larry | Generate the minisign component key, commit the public key line to `PRODUCTION_COMPONENT_PUBKEY` in a signed commit (and update the test that pins the empty slot), sign and publish the first manifest per COMPONENT_UPDATER.md. Owner: CVE SLA values. | core #219 |
| ra-5 | merge, owner | stacker; Larry | Merge runtime #56 and core #199. Owner: decide step budget 10, dead-control after 2. | runtime #56, core #199 |
| ra-6 | external | @rule8 key holder; 📡 DGX team | Ceremony and signed SearXNG publish; DGX linux-x64 and linux-arm64 locks and the python-build-standalone measurement, then `pack-searxng.sh --platform <plat>`. | core #219, runtime #64 |
| ra-7 | external, owner | @rule8 key holder; Larry | Owner: confirm skills and the docs graph stay in the installer for 0.5.0 (the default written in COMPONENT_UPDATER.md). Ceremony as g3-updater. | core #219 |
| story-US-4-1 | merge, QA, owner | stacker; 📡 DGX team; a person; Larry | Merge core #201 and runtime #55; A45 sidecar rebuild; re-record the demo in the packaged app with one real approved tx_propose; MCP sign-offs (A24 hints only, node entry off, token lifecycle, card timeout). | core #201, runtime #55 |
| story-US-4-2 | merge, QA, owner | stacker; a person; Larry | Merge the core stack; approve one real tx_propose or deploy_propose; owner `HERMES_NODE_WRITE_TOOLS=false`. | core #200 to #212 |
| story-US-5-1 | external, QA, owner | as g3-browser | As g3-browser. Attach-to-Chrome scoping is already merged (runtime #30). | core #219, #220; runtime #64, #65 |
| story-US-5-2 | external, owner | @rule8 key holder; 📡 DGX team; Larry | Ceremony and SearXNG publish; DGX locks; owner AGPL approach and the default 5-engine list (brave, duckduckgo, google cse, wikipedia, wikidata). | core #219, runtime #64 |
| story-US-5-3 | owner, buildable (after restart) | Larry; eval agent | Owner: Jev off and documented as not offered, local as the only backend (recommended). Agent: T1 web-subset-v2 through decide() after the Mac restart (`cargo test -p agent-sidecar --test browse_live -- --ignored`). | core #220, runtime #65 |
| wp-S4-2 | merge, QA | stacker; a person | Merge the core stack; one packaged approval of a real tx_propose. | core #200 to #212 |

## G4 (M4 = S6)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| fed-F-5 | owner, external | Larry; chain operator | Owner: accept the faucet ADR O-1 to O-4. Operator: the #283 steps after chain #270 merges (merge runtime #57, then chain #270, then core #204). | chain #270 |
| gate-g3-e2e | owner, buildable, QA | Larry; core and runtime agent; a person | Owner: may the managed browser open one core-attested loopback origin (the hello-mint page against the anvil fork)? If yes, an agent builds the pop-out. Then a member runs hello-mint in the packaged 0.5.0 app on 40204 with funded SALT and approves the deploy and decoded mint ceremonies. | core #221, runtime #66 |
| ra-10 | external, owner | 📡 DGX team; @rule8 key holder; Larry | DGX: measure linux-x64 and windows-x64 archives into toolchain-bundle.json, host the slither wheelhouse with crytic-compile, run `scripts/e2e-hello-mint.sh --sidecar-bin <sidecar> --fork-bin <citrate-fork> ...` on clean Linux and Windows. Ceremony. Owner: upstream fetch vs mirror. | core #221, runtime #66 |
| story-US-6-1 | owner, QA, external | as g3-e2e | AC1 measured (READY after 1 prompt, scripted model and Gemma 4 E4B) and AC2 pass locally. Pop-out and the live 40204 leg remain. | core #221, runtime #66 |
| story-US-6-2 | merge, owner | stacker; Larry | Closed on branch. Merge runtime #66 before core #221. Owner: the deploy guard, the narrower `medusa.json` rule (reviewer recommends one more restriction first; details on #298), crytic-compile venv readable, forkInCore as default. | runtime #66, core #221 |
| story-US-6-3 | merge, QA | stacker; a person | Merge core #205 and runtime #58; QA the reader against the live explorer after A45. | core #205, runtime #58 |
| story-US-6-5 | owner, external, QA | Larry; chain operator; a person | Faucet ADR; F-5 deploy; QA of the challenge window and Settings. | core #204 |
| wp-S6-0 | external, owner | 📡 DGX team; Larry and counsel | Windows x64 toolchain spike (foundry, solc, medusa, slither, aderyn), zip unpack, `e2e-hello-mint.sh`; licence calls as g3-licence. | spike report |
| wp-S6-5 | owner | Larry | Accept the faucet ADR as drafted (1 per wallet per 24 h, 2,000,000 gas times the live price, membership check, Turnstile, on by default). | ADR status change |

## G5 (M5 = S7, S8)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| fed-F-10 | buildable, owner, external | agent; Larry; 📡 DGX team | The private CitrateNetwork/citrate-sizeup repo now exists. Push and merge sizeup `hup/n7-sizeup-library` (1658eba, d593b8b, f29c3e0); owner extends `CITRATE_CHAIN_READ_TOKEN` to citrate-sizeup; add `[repos.citrate-sizeup]` and the `[[drift]]` row; apply `n7-sizeup-library.patch` with a git rev pin and delete the patch; DGX runs `library_api` and `sizeup-probe` on Linux and Windows. | core #225, a sizeup PR, a federation manifest PR |
| fed-F-3 | merge, owner | stacker; Larry | Merge chain #270 to #274; gas schedule, ADR, H. | chain #270 to #274 |
| fed-F-9 | external, owner | security lead + Larry; 📡 DGX team | Sign CL-S4 including the new HUP-S8.4 section and the mDNS dependency decision (#298); DGX runs DEVICELINK_MULTI_MACHINE.md section 4 (2 machines x 2 groups from seeds, open TCP 4211 to 5234) and a 50+ node ladder on separate machines. | cluster #12, core #226 |
| gate-g4-anchor | owner, QA | Larry; a member | O-5: per-day HIC-1 approval from the member wallet on the next AnchorRegistry (recommended). Then one real anchor and 'Prove a past decision' on 40204. | core #190 to #206 |
| gate-g4-fleet | external, owner | 📡 DGX team + a person; security lead + Larry | Two-machine wizard run with distinct peer ids; CL-S4. | cluster #12, core #226 |
| gate-g4-identity | owner, external | Larry; chain operator; 📡 DGX team | Issuance path (registrar under one Citrate org, timelock admin, recommended), DID format, fingerprint source. Operator creates the org and grants issuer rights; a member mints; A45 deploys CitrateScan and checks `/api/agents`. | explorer #19 |
| gate-g4-precompiles | owner, external | Larry; 📡 DGX team; chain operator | Owner: gas, H, the ADR, the (committer, root) registry. DGX: `devnet-precompile-check.sh` on 3 nodes with the same H (steps on #284). Operator: activation and the F-4 redeploy, then a model or LoRA call on 40204. | chain #276, core #223 |
| ra-16 | buildable (after restart), owner | eval agent; Larry | T1 qa-literacy-v2 run. Owner: should a v3 accept paraphrased key points, should Gradient Paper No. 2 count as an alternative citation, and does the sidecar workflow eval count as the belnap_codec check. | core #217 |
| story-US-7-1 | owner, external | as g4-identity | Explorer AgentSBT pages built (AC2 half). | explorer #19 |
| story-US-7-2 | owner, QA | Larry; a member | O-5 and the registry pick; one real anchor with inclusion proof on 40204. | core #190 to #206 |
| story-US-7-3 | merge, owner, external | Larry; chain operator | AC1 built. Owner: the D-27 defaults (cards per metric, up to 26 a day; 30 W and 30 W energy estimate; sampling on; SALT and gas in shared metrics). AC3 needs an AgentSBT and BenchmarkRegistry pinned on 40204. | core #224, runtime #68 |
| story-US-7-4 | merge, QA | a person | Chat-turn plan rows added. Person runs the 7-step click-through in N7-EVERYDAY-PACKAGED-QA-EVIDENCE.md section 5 on a QA account. | core #228 |
| story-US-7-5 | owner, external, buildable | Larry; 📡 DGX team; core agent | AC1 encode and decode done. A design call on routing `precompile_call` through contract code, then agent work. AC2 and AC3 need H and the multi-node devnet run. | core #223, chain #276 |
| story-US-8-1 | external, owner | 📡 DGX team + a person; Larry | Separate-machine Gherkin run; Windows Tailscale and firewall check; owner shared-link caps and relay as the channel. | core #207, #226 |
| story-US-8-2 | external, owner | security lead + Larry; 📡 DGX team | CL-S4, soak, ladder; then flip `MULTI_GROUP_DAEMON` (after the bundled daemon is rebuilt) and `TRANSPORT_SIGNED_OFF` with owner OK. Owner: port span 1024, 32 groups, mDNS compiled out, seed format v1. | cluster #12, core #226 |
| wp-S7-1 | external, owner | as fed-F-4 | As fed-F-4. | chain #275, core #222, runtime #63, #67 |
| wp-S7-2 | external, owner | as g4-precompiles | As g4-precompiles. | chain #276, core #223 |
| wp-S7-3 | merge | stacker | Merge the stacks; the live anchor is g4-anchor. | core #190 to #206 |
| wp-S7-5 | merge, owner | stacker; Larry | Closed on branch. Owner: the D-27 defaults. | core #224, runtime #68 |
| wp-S8-1 | external | 📡 DGX team | Daemon-level 2-machine (late link) and 3-machine (revocation) runs, recording PeerIds. | cluster #10, #11 |
| wp-S8-2 | external | 📡 DGX team + a person | Mac plus Linux BDD; Windows check; probe source becomes sizeup after F-10. | core #225 |
| wp-S8-4 | external, owner | as US-8.2 | As US-8.2. | cluster #12, core #226 |

## G6 (M6 = S9, S10)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| fed-F-11 | external, owner | 📡 DGX team; chain operator; Larry | DGX (#286): release build of compute-pool `hup/n7-fl-learn-prep`, idle probe at 10,000 tok/s or more, members turn on 'Train on my verified conversations' and build a set, `citrate-fl-dataset`, a real LoRA trainer as `CITRATE_LORA_TRAINER`, the app's bundled llama-server; post receipts and the adapter sha256. Operator: ledger deploy, fork, F-4. Owner: round rules, public data share 0 %, mix ratios, held-out extension, retry defaults. | compute-pool #28, core #227, runtime #69, chain #277 |
| fed-F-2 | owner | Larry | Gas placeholders, H with F-4, accept the ADR, the (committer, root) registry. | ADR status change |
| gate-g4-fl | external, owner | as fed-F-11 | As F-11, then the S9.4 eval gate on the adapter. | as fed-F-11 |
| gate-g5-everyday | owner, QA, external | Larry; a person; 📡 DGX team | Owner: defer video (recommended), media caps and price table, Google OAuth client. Person: the 7-step click-through, including showing the live image in the Media pop-out. DGX: Linux and Windows runs and the headless media test. | core #209, #228 |
| ra-15 | merge, owner | stacker; Larry | Closed on branch. Owner: public data share 0 % and the mix ratios. | compute-pool #28 |
| story-US-10-1 | owner, QA | Larry; a person | Video decision; on-screen pop-out step (the live image run is recorded headless). | core #228 |
| story-US-10-2 | merge, owner | stacker; Larry | Follow-up done. Owner: Google OAuth client for one live Sheets or Calendar run; trust labels and row caps. | core #209, #228 |
| story-US-10-3 | merge, QA, owner | a person; Larry | Packaged click-through; daemon budgets and the 10-minute limit. | core #209 |
| story-US-10-4 | merge, QA, owner | a person; Larry | Native dialogs QA; daily summary off at 23:00 UTC; @daemon bullets. | core #209 |
| story-US-9-1 | external, owner | as fed-F-11 | AC1 (S9.3 opt-in) met on branch. AC4 needs F-2 and F-4 and a live round. | core #227, compute-pool #28 |
| story-US-9-2 | buildable (after restart), owner | eval agent; Larry | AC1 not met on T0 (51.2 % pass, 70.3 % citation hits). T1 run after the restart; owner calls as ra-16. | core #217 |
| wp-S9-1 | merge, owner | stacker; Larry | Owner: do not wire `enable_learning` for 0.5.0 (recommended). Merge chain #274; Rule-6 benchmark. | chain #274 |
| wp-S9-2 | external, then buildable | chain operator; 📡 DGX team; agent | Fork, F-4, ledger; live round; then an agent wires adapter registration against the deployed addresses. | after F-4 |
| wp-S9-4 | external, owner | 📡 DGX team; Larry | A really trained Gemma 4 E4B LoRA with sha256; owner gate thresholds and coordinator URL; then an agent runs the eval gate. | after F-11 |

## G7 (M7 = S11 and every gate)

| Item | Status | Who | Exact ask or work | Closes with |
|---|---|---|---|---|
| gate-g0-formal | merge | stacker | Met on branch (gates.yaml flipped, TLC re-runs recorded). Some specs live on unmerged branches; the note names the commits. | core #231 |
| gate-g5-docs | merge | stacker | Merge docs #27 (npm audit fix), #30, #31 after the core stacks; then flip. | docs #31, core #231 |
| gate-g5-os | external, QA | Larry (member run); 📡 DGX team; chain operator | macOS arm64 clean install, hello-mint on 40204 with funded SALT, approve deploy and mint; Linux x64 and Windows x64 clean installs (steps on #288); 40204 redeploy and live CitrateScan verification. | core #221 |
| gate-g5-redteam | buildable (after merge) | security lane (L18) | Not run this round. Second-model red-team over the merged 0.5.0 code, private report, private remediation, 0 open High. | private |
| gate-g5-size | external, owner | 📡 DGX team; Larry | DGX: `scripts/size-budget.mjs` on the first Linux x64 and Windows x64 installers; re-measure the updater row on the real release build. Owner: DEC21. | core #230 |
| ra-21 | owner | Larry | DEC21: approve the macOS budgets (DMG 455 MB, updater 460 MB; measured DMG is now 420.9 MB, so about 445 MB would also fit), CI gates on bundle-lite, Linux and Windows at first measurement +5 % rounded up to 5 MB. bin/citrate-core is at 97.1 % of its row. | core #230 |
| story-US-11-1 | external, QA | as g5-os | As g5-os. | core #221 |
| story-US-11-2 | merge, external | stacker; 📡 DGX team; Larry | Code done. Proof run 1: the PR job runs once a PR into main or release/** carries core #229. Proof run 2: eval.yml on main, an https T0 and T1 endpoint a runner can reach, and secrets `EVAL_BASE_URL_T0/T1/T2` (and `EVAL_API_KEY_*`); then `gh workflow run eval.yml --ref release/0.5.0-hermes-upskill -f model=<name> -f tier=T0`. | core #229 |
| story-US-11-3 | buildable (after merge) | security lane (L18) | As g5-redteam; close or accept the residual L and M items. | private |
| story-US-11-4 | merge | stacker; doc agent at the cut | Pages, retro draft and essay written. Merge; write the retro's 'at close' section at the v0.5.0 cut. | docs #31, core #231 |
| wp-S11-1 | external, QA | as g5-os | As g5-os. | core #221 |
| wp-S11-3 | buildable (after merge) | security lane (L18) | As g5-redteam; close #298 and #299 with evidence. | private |
| wp-S11-4 | merge | as US-11.4 | As US-11.4. | docs #31, core #231 |

## Done or obsolete (no work left)

| Item | Verdict |
|---|---|
| fed-F-7 | done |
| ra-2 | done |
| fed-F-6 | done |
| ra-11 | done |
| wp-S8-3 | done |
| rel-041-12 | obsolete |
| rel-041-13 | obsolete |
| rel-041-14 | obsolete |
| rel-041-15 | obsolete |
| ra-3 | obsolete |
| ra-4 | obsolete |
| ra-8 | obsolete |
| ra-9 | obsolete |

`gate-g0-formal` moved from done to **merge** above: the paperwork is on core #231.

## New follow-ups found in fan-out 7 (not playbook rows)

| Follow-up | Who | Where it came from |
|---|---|---|
| rpc.citrate.ai returns null for transaction and receipt lookups by hash; blocks the F-4 broadcast and the book tools | 📡 DGX team | L03 |
| Provenance ledger lacks deployer nonces 116 and 117 (the backfill records them) | chain operator | L03 |
| One more restriction on the `medusa.json` allowlist before owner sign-off (details on #298) | runtime agent | L05 review |
| Self-review returns empty content on thinking models (token budget spent on reasoning) | runtime agent (US-1.3) | L06 |
| `memory.neighbors` has no node ids, so the older side of a learned-vs-learned contradiction can still show by title | memories + core agent | L02 review |
| Fleet group links have no UI yet (QR, paste box), and seeds are not sent over the group relay | core + cluster agent | L16 |
| Turning trajectory recording off takes effect at the next Hermes start; restart Hermes on turn-off | core agent | L10 review |
| `citrate-fl-dataset --heldout` replaces the embedded manifest instead of adding to it | compute-pool agent | L10 review |
| A ceremony transaction mined after the 60 s poll is not metered; `meter_anchor` in re-poll has no test | core agent | L06 review |
| `DeployQuorumS6` deploys a legacy AnchorRegistry with no 40204 guard | Larry or the quorum lane | L03 review |
| `devnet-precompile-check.sh` passes the devnet key on the cast command line | chain agent | L04 review |
| ModelAccessControl.sol header comment and `inference_router.rs` module doc are stale | chain + core agent | L03, L04 reviews |
| citrate-docs: merge #27 (npm audit) first; regenerate the addresses page (`npm run docs:addresses`) | docs agent | docs stacker |
| `pack-searxng.sh --apply` records the rebuild's hash, not the hosted file's | core agent | L12 review |
| Base fmt and clippy drift in runtime, chain and memories still fails workspace CI jobs (A59) | runtime + chain agent | stackers |
| Rosetta detection fix in sizeup needs an x86_64 run to prove | 📡 DGX team | L07 review |

## Owner decisions still open

Each is built as the recommended default and marked "pending owner sign-off" in code or docs.

1. **US-1.1:** gateway-provider turns into sidecar sessions, or accept the in-app gateway loop; the 8-turn cap; the 8-schema ceiling for gateway members; headless dispatcher runs only read-only tools.
2. **US-1.5:** registry escalation path (b), its placeholders, and whether the InferenceRouter in the book is the right one before `--with-inference-router`.
3. **ra-20 and US-11.2:** T0 held to the T1 bars; v2 default; 80 % steps.
4. **ra-1:** T1 model pick after the runs.
5. **ra-16 and US-9.2:** qa-literacy-v3 paraphrase acceptance; Gradient Paper No. 2 as an alternative citation; workflow eval as the belnap_codec check; the bar applied to qa-literacy.
6. **ra-19:** SIWE grant defaults.
7. **US-3.4:** `SKILL_PUBLISH_ENABLED` after the SkillRegistry is on 40204.
8. **ra-18 (A17):** the Rule-3 amendment readings, A-1 and A-2 in particular (with @rule8).
9. **g3-licence:** the six licence items, the notice generator, the MPL-2.0 source pointer.
10. **ra-12:** gradient-papers under version control.
11. **ra-7 and g3-updater:** skills and docs graph stay in the installer; CVE SLA.
12. **US-5.2 and US-5.1:** default SearXNG engines; CfT fetch vs re-host; open-web block vs warning.
13. **US-5.3:** Jev off, local only.
14. **US-6.1 and g3-e2e:** a core-attested loopback origin for the pop-out.
15. **US-6.2:** deploy guard, `medusa.json` rule, crytic-compile venv read roots, forkInCore default.
16. **S6.5 and F-5:** faucet ADR O-1 to O-4.
17. **F-2, F-3, g4-precompiles:** gas, H, ADR, registry keyed by (committer, root).
18. **F-4 and S7.1:** redeploy admin and timing; when to regenerate the embedded book.
19. **g4-anchor (O-5):** gas payer, per-day HIC-1, registry; the 400,000 anchor gas limit.
20. **g4-identity and US-7.1:** AgentSBT issuance path, org, DID, admin.
21. **US-7.3 and S7.5:** the four D-27 defaults.
22. **Fleet (US-8.1, US-8.2, S8.4):** port span, 32 groups, mDNS out of release builds, seed format v1, bootstrap ignored in multi-group mode, `MULTI_GROUP_DAEMON` flip; shared-link caps and listen address.
23. **F-10:** role names and mapping in sizeup; sizeup as the tier source of truth.
24. **FL (F-11, g4-fl, S9.4):** round rules, slashing off for round one, public data share 0 %, mix ratios, held-out extension, retry defaults, coordinator URL.
25. **S9.1:** do not wire `enable_learning` for 0.5.0.
26. **g5-everyday and US-10.x:** video deferred, media caps and prices, daemon budgets, daily summary, @daemon bullets, chat-turn plan shape.
27. **DEC21 (ra-21, g5-size):** macOS budgets and CI flavour.
28. **MCP (US-4.x, S4.2):** annotation hints, node entry off, `HERMES_NODE_WRITE_TOOLS=false`, token lifecycle, card timeout.
29. **Private:** where to host the citrate-identity fix (private fork or advisory fork).
