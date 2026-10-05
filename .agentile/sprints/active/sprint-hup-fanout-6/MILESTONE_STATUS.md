---
created: 2026-10-04T20:10:00Z
branch: docs/hup-fanout-6
author: Larry Klosowski + Claude Opus 5.5
status: active
type: milestone-status
planset: 2026-09-30-hermes-upskill
baseline: fan-out 5 per-milestone verification (2026-10-01), updated by fan-out 6 items_closed and reviews
---

# HUP milestone status after fan-out 6 (M1 to M7)

Every story, work package (WP) and gate the fan-out 5 verifiers checked is listed here once, with
its status after fan-out 6. The baseline is the fan-out 5 verification (verifier verdicts against
core `release/0.5.0-hermes-upskill` @ 00e6353 and runtime `main` @ 01a32ed). I updated each row from
the fan-out 6 builder and reviewer reports, and where a reviewer narrowed a builder's claim I used
the reviewer's wording. I did not re-run tests for this file.

## Status words

| Status | Meaning |
|---|---|
| **met** | Holds on merged code (the integration branch). |
| **met (M2 PR)** | Holds on `hup/m2-core` / `hup/m2-runtime`, open as core #190 / runtime #47. |
| **met on branch** | Holds on a fan-out 6 branch with an open PR (named), reviewed, not merged. Packaged-app QA by a person (A48) is still S11 for every row and is not repeated. |
| **buildable** | An agent on this Mac can close the named gap. |
| **external** | Needs a named team: 📡 DGX team (Linux, Windows, GPU, release machine), chain operator (40204 deploys after owner sign-off), or a person at the packaged app. |
| **owner** | Needs Larry's decision. Placeholders in code are marked "pending owner sign-off". |

Rows can carry more than one word when the remaining gap has more than one owner.

## Summary

| Milestone | Items | Met, only QA left | Met on a branch, with a named gap | Not met | Rows needing buildable / external / owner |
|---|---:|---:|---:|---:|---|
| M1 = 0.4.2 (S1) | 22 | 7 | 5 | 10 | 7 / 7 / 6 |
| M2 = 0.4.3 (S2, S3) | 30 | 24 | 0 | 6 | 2 / 6 / 2 |
| M3 = 0.4.4 (S4, S5) | 19 | 5 | 8 | 6 | 5 / 10 / 9 |
| M4 = 0.4.5 (S6) | 18 | 5 | 7 | 6 | 2 / 9 / 7 |
| M5 = 0.4.6 (S7, S8) | 24 | 4 | 7 | 13 | 6 / 14 / 13 |
| M6 = 0.4.7 (S9, S10) | 18 | 2 | 10 | 6 | 5 / 11 / 5 |
| M7 = 0.5.0 (S11 + every gate) | 31 | 5 | 4 | 22 | 11 / 15 / 9 |

Counted from the status column of the tables below (M2 from M2_STATUS.md, whose 30 rows are grouped
here). The last column overlaps: a row that needs an owner call and an external run counts in both.
M7 repeats the earlier gates because 0.5.0 needs all of them, and "met" in the first columns
includes "met on branch", which is not merged.

The short version: almost everything an agent can build on this Mac is now built and reviewed on a
branch. What stands between the branches and the milestones is, in order: merging the stacks (A53),
owner decisions (A57), packaged-app QA by a person (A48), and external runs on other hardware and on
40204 (A58).

## Gate flips proposed for the doc pass after the stacks merge (A54)

`gates.yaml` is not edited here: the evidence for these rows sits on stack branches, not on this
PR's base.

| Gate | Proposed | Evidence (lands with) |
|---|---|---|
| g0-formal | met | `sprint-hup-s1-one-agent/EVIDENCE-2026-10-04-verify-formal.md`: FolderGrant, SpendBudget (depth 26), WebSigningBudget, DeviceLink, AgentLoop run to completion; mutants caught (#191) |
| g1-approval-audit | met | Same evidence doc; agentToolGates covers every tool added since 0.4.1 (#191) |
| g1-no-block | met | Same evidence doc; main-thread tripwire green (#191) |
| g1-sidebar | met after screenshots | 7 D-34 nav tests re-run; the S0.8 screenshots were not re-taken (buildable) |
| g1-loop | met with one caveat | UI, CLI and MCP drive one session; reattach after reload (#196, #193). Caveat: the idle-view gap (buildable, see M1) |
| g2-mcp | met (already flipped on #200) | `docs/NODE_MCP_SERVER.md` part 3 demo (#200) |
| g3-gate | met (flipped on #202), owner to confirm | `EVIDENCE-n6-hellomint-e2e.md`: injected unbounded mint gives NOT READY; aderyn and medusa came from the measured bundle archives in a test-only config |

## M1 = 0.4.2 (HUP-S1 One agent)

| Item | Status | Evidence | Remaining |
|---|---|---|---|
| gate g0-formal | met on branch | #191 evidence doc (above) | Flip in gates.yaml (A54) |
| gate g1-loop | met on branch; buildable | #196 (session tools on node MCP, reattach, `hermes run`), #193 live parity 21/21 against the packaged sidecar; sidecar loop on by default (M2) | Buildable: an idle open view must drive the core-hosted tool calls of a turn sent from the CLI or MCP (today they wait for the 300 s deadline). External: bundled sidecar rebuilt from merged runtime (A45, 📡 DGX team) |
| gate g1-eval | met on branch (measurement); owner; external | #194: v1 frozen and hash-pinned, v2 fragments, workflow-v1 step success T0 41/41, T1 36/41 (87.8 %) | Owner: T0 guided bar, the 80 % step bar, tier model pick (Qwen3.8-27B stand-in). External: T2 run (📡 DGX team, commands on #278) |
| gate g1-injection | buildable; owner | #194: MCP-output and browser vectors through a real sidecar, T0 12/12, T1 11/12 | Buildable: re-score the browser rows with the revised delivery rule (a browser case counts as delivered only after a snapshot). Owner: thresholds |
| gate g1-wallet-unlink | external | unchanged | Identity service deploys identity#31 and the primary_wallet fix (federation #293), then S1.11 merges |
| US-1.1 | met on branch; buildable | as g1-loop | The idle-view gap; chat history is not saved across a reload (unchanged behaviour) |
| US-1.2 | met | interview + editable brief | none |
| US-1.3 | met on branch | #191 / runtime #53: HTTP-status and hash verifiers, self-review recorded as an `opinion` event that never decides an outcome (mutant-tested) | Buildable follow-up: persist the opinion beyond the in-memory event ring and show it in the app; `ModelPlanner` is implemented but not wired to any route |
| US-1.4 | met on branch (AC1, sidecar path); buildable | #192 / runtime #51: at most 8 tool schemas incl. pinned and in-use; tokenizer-true counts proven live on T0 by the reviewer | Buildable: serve the bundled `bge-base-en-v1.5` on a loopback `/v1/embeddings` and pass `CITRATE_HERMES_EMBED_URL` (no new model needed); schema ceiling and tokenizer counts for the in-app fallback loop; AC2 measurement |
| US-1.5 | external; owner | #195 / runtime #50: InferenceRouter path against the chain ABI, EIP-712 hasher, receipts in metering, expire path; anvil dry run green | Chain operator: deploy and pin InferenceRouter on 40204 (F-4). Owner: O-1 asset; router is native-SALT only, so budgeted x402 needs a chain change or an ADR amendment; max price 10 SALT and 4096-byte input are placeholders |
| US-1.6 | met | tier at onboarding | none |
| S1.1 | met on branch; buildable | as g1-loop | as g1-loop |
| S1.2 | buildable | #192: hybrid ranker implemented and wired, but every production session ranks lexically (no embedding endpoint runs) | Serve the bundled BGE (as US-1.4) |
| S1.3 | met on branch | as US-1.3 | as US-1.3 |
| S1.4 | met | | Packaged QA |
| S1.5 | external; owner | as US-1.5 | as US-1.5; F-1 for ModelRegistry CIDs |
| S1.6 | external | one 32 GB Apple Silicon Mac recorded | 📡 DGX team: Linux x64 GPU and Windows x64 reference runs |
| S1.7 | owner; external | as g1-eval | as g1-eval |
| S1.8 | met on branch | runtime #52: `citrate-agent hermes run <workflow> --follow`, `hermes sessions` | Small: `run --follow` has no idle limit |
| S1.9 | owner | #193: live parity 21/21 against the packaged sidecar binary; process-split check | Owner: turn cap 6 (harness.ts) vs 8 (sidecar); then retire harness.ts as the main loop. `live_context_freshness` |
| S1.10 | buildable | as g1-injection | as g1-injection |
| S1.11 | external | unchanged | as g1-wallet-unlink |

## M2 = 0.4.3 (HUP-S2 Human in control + HUP-S3 Knowledge and skills)

The row-by-row view is [M2_STATUS.md](../sprint-hup-m2/M2_STATUS.md) on `hup/m2-core` (30 items).
One change since it was written: the owner set the US-3.1 AC2 target on 2026-10-04 (T1 pass
>= 75 %, citation hit >= 80 %; commit 77e3bdf), so g2-knowledge is met and US-3.1 / S3.1 no longer
wait on him.

| Item | Status | Remaining |
|---|---|---|
| gate0 g0-rule3-adr | met | Optional independent formal review; A17 amendment |
| gate2 g2-hic | met (M2 PR) | External: Linux and Windows proof runs. Owner: shell_run and toolchain templates off by default |
| gate2 g2-knowledge | met (M2 PR) | Release upload of corpus and import-corpus mem-mcp (A45) |
| gate2 g2-personas | met (M2 PR) | Owner: shipped defaults (TTS voice, Guide and Operator tracks) |
| US-2.1, US-2.2, US-2.4, US-2.5 | met (M2 PR) | Linux/Windows runs; member UI for capsule mounts and egress consent |
| US-2.3 / S2.3 | external | Chain operator: AnchorRegistry on 40204 is live per the chain team; the anchor transaction needs owner O-5 and a member's approval. A packaged run against a real https dApp |
| US-3.1 / S3.1 | external | 📡 DGX team: upload `knowledge-corpus.tar.gz` and the import-corpus mem-mcp to the runtime-deps prerelease and pin both in `runtime-deps.sha256` (A45) |
| US-3.2, US-3.3 | met (M2 PR) | The in-app fallback loop composes the persona prompt locally (sidecar down only) |
| US-3.4 / S3.4 | owner; external; buildable | Owner: `SKILL_PUBLISH_ENABLED`. Buildable before enabling: ABI-decode the publish calldata; runtime `skill_hash` must switch to the registry's `abi.encode` layout (see S7.1). External: SkillRegistry on 40204 |
| S2.0, S2.1, S2.2, S2.4 to S2.9, S3.2, S3.3, S3.5, S3.6, S3.7 | met (M2 PR) | Per M2_STATUS.md |

## M3 = 0.4.4 (HUP-S4 MCP fabric + HUP-S5 Eyes on the web)

| Item | Status | Evidence | Remaining |
|---|---|---|---|
| US-4.1 | met on branch; external; owner | #201 / runtime #55: built-in node entry (off by default), approval cards for effectful MCP calls after taint, 2026-07-28 protocol with Tasks and URL elicitation, reconnect, list_changed, live status. After the stack join Hermes's node token is read-only (`HERMES_NODE_WRITE_TOOLS=false`) | Person at the packaged M3 app: re-record the node-entry demo. Owner: write tools for Hermes, A24 annotation trust. Not built: notifications/tasks, form elicitation, sampling, roots, OAuth; no official conformance suite |
| US-4.2 | met on branch; external | #200: deploy-and-confirm tasks with receipts, ABI resource template, `ed25519_verify`; `faucet_request` from #204 | Person at the packaged app approves one real `tx_propose` and `deploy_propose`. Typed helpers for other precompiles wait on S7.2 activation |
| US-5.1 | met on branch (code); buildable; external; owner | #199: browser switch (off), Chrome for Testing 154 entry measured on macOS arm64, `managed_chromium` | Buildable: zip unpacking in the components updater (or a tar.xz repack). External: @rule8 component key ceremony, signed manifest, other-platform archives (📡 DGX team). Owner: re-host Google's CfT builds or fetch from Google |
| US-5.2 | met on branch (live run); external; owner | #199 / runtime #56: SearXNG 2026.10.4 live through the real sidecar; a settings.yml bug found and fixed | External: pack and sign the hash-locked wheelhouse; non-macOS locks. Owner: AGPL distribution approach |
| US-5.3 | met on branch (T0); buildable; owner | runtime #56: `decide()` in the browser, web-subset-v2 T0 9/10 | Buildable: score T1. Owner: Jev vendor and terms |
| HUP-S4.1 | met on branch | as US-4.1 | as US-4.1 |
| HUP-S4.2 | met on branch; external | as US-4.2 | as US-4.2 |
| HUP-S4.3 | met | live per-server state added on #201 | none |
| HUP-S4.4 | met | | Sidecar rebuild (A45) |
| HUP-S5.1 | met on branch; external; owner | as US-5.1 | as US-5.1 |
| HUP-S5.2 | external; owner | as US-5.2 | as US-5.2 |
| HUP-S5.3 | buildable; owner | as US-5.3 | as US-5.3 |
| HUP-S5.4 | met on branch (buildable gaps) | #199: Code and diff pop-out, `hermes_checkpoint_diff`, a11y tests | Person: packaged click-through. Small buildable: a "truncated" count when a step changes more than 200 files |
| HUP-S5.5 | external | unchanged | @rule8 component key ceremony and a signed manifest |
| HUP-S5.6 | met | | Owner placeholders |
| gate g2-mcp | met on branch (#200) | external-client demo with Claude Code | Flip lands with #200 |
| gate g3-updater | external; owner | unchanged | Key ceremony, CVE SLA, security sign-off |
| gate g3-licence | owner | #210: `docs/LICENCE_REVIEW.md`, `release/licences.json` with a CI check, missing licence texts now bundled | Owner or counsel: sign the review, choose the source-offer form, publish it |
| gate g3-browser | external; buildable | as US-5.1 to US-5.3 | T1 score; packaged pop-out run; key ceremony for the managed browser |

## M4 = 0.4.5 (HUP-S6 dApp forge)

| Item | Status | Evidence | Remaining |
|---|---|---|---|
| US-6.1 | met on branch (local half); buildable; external | #202: Gherkin e2e on a local chain 40204 anvil: NOT READY without tools, READY then deploy, verify, mint, IPFS pin; injected bug NOT READY | Buildable: send `forkInCore` from the hello-mint workflow (A27 join); a Hermes fix proposal after NOT READY; ABI decoding for the template's `mint(uint256)`; Browser pop-out on the fork. External: member approves the deploy on 40204 with funded SALT (g3-e2e) |
| US-6.2 | buildable | #202, #205: the gate refuses and names the failing test or finding | Agent-level BDD: Hermes cites the finding and offers a fix |
| US-6.3 | met on branch | #205: read-only `contract_view` tool (40204 or loopback only) | none |
| US-6.4 | met on branch (wired); external | #205: `template_list` / `template_render` with OZ-Wizard-style forms, templates bundled | External: pinned libraries reach a member's machine only after the S6.1 installer ships (key ceremony), so a rendered template cannot compile there yet |
| US-6.5 | met on branch (built); owner; external | #204, chain #270, runtime #57: in-app challenge window, decision records, e2e against a real faucet binary | Owner: faucet ADR O-1 to O-4. 📡 DGX team: replace the faucet binary behind faucet.citrate.ai after #270 merges |
| S6.0 | external; owner | licence part drafted on #210 | Windows spike (Windows x64 machine); licence sign-off |
| S6.1 | external | unchanged | Per-OS hashes, slither wheelhouse, key ceremony |
| S6.2 | met on branch | #205 | none beyond US-6.4 |
| S6.3 | met on branch; owner | #205 / runtime #58: tool reports bound to the artifact bytecode feed `deploy_gate_submit_toolchain`; `forge test --force` (review fix) | Owner: finish A27 as (a) gate is the only deploy authority with shared fixtures, or (b) one shared parser crate (Rule 12 drift entry); canonical aderyn format |
| S6.4 | met | | none |
| S6.5 | owner; external | as US-6.5 | as US-6.5 |
| S6.6 | met (anvil) | | Live run rolls into g3-e2e |
| S6.7 | met on branch | as US-6.3 | none |
| S6.8 | owner | | Accept or amend the faucet ADR |
| S6.9 | met on branch; owner; external | #205: tier budget from the template lock, gate enforces calls and minimum coverage; calibration on one loaded Mac | Owner: 10k/50k/200k calls and 60/75/80 % coverage. 📡 DGX team: `scripts/forge-gate-proof.sh` on T1 and T2 hardware |
| S6.10 | met on branch; external | chain #271, core #203: REVM fork with Citrate precompiles, node parity test, core-run provenance | External: signed per-OS component; Rule 6 benchmark after merge. Buildable: nothing sends `forkInCore` automatically yet (as US-6.1) |
| gate g3-gate | met on branch (#202); owner | as US-6.1 | Owner: accept the test-only tool config, and Medusa calibration (a T1 campaign misses the unbounded mint; only the template's forge test catches it) |
| gate g3-e2e | external | | Member on a T1 machine approves the hello-mint deploy on 40204, after the explorer deploy (A45) and faucet or manual funding |

## M5 = 0.4.6 (HUP-S7 Chain-native + HUP-S8 Fleet)

| Item | Status | Evidence | Remaining |
|---|---|---|---|
| US-7.1 | owner; external | chain #272 prepares the registry redeploy set | Owner: issuance path, parent org, DID and fingerprint; admin (timelock placeholder). Chain operator: deploy and create the org |
| US-7.2 | met on branch (AC1 to AC3 local); owner; external | #206 / runtime #59: book pins for AnchorRegistry and BenchmarkRegistry (live-checked), persisted in-flight set, HIC records batched, in-app proof of a past decision; anvil e2e on two registry versions | Owner: O-5 (anchor key gas), which registry. Person: one real anchor approval on 40204, then "Prove a past decision" against it |
| US-7.3 | buildable | tokens/s now measured from llama-server usage (#209) | D-27 fields still unknown: TTFT, SALT, gas, CPU/GPU/RAM peaks, energy |
| US-7.4 | met on branch | #209: plan, approvals pending/decided, verifier results, tokens/s, context used | Packaged QA |
| US-7.5 | external; owner | chain #273: agent precompiles and model-precompile wiring, activation gated (default off); benchmark against ecrecover | Owner: gas schedule and height H. Chain operator: activation and the F-4 redeploy |
| US-8.1 | met on branch (buildable parts); external | #207, cluster #11: DeviceLink through pairing, cross-member distribution over the group relay, no restart | 📡 DGX team: 2- and 3-machine runs (scripts on #285) |
| US-8.2 | external | cluster #11 CL-S4 packet | Owner + security lead: CL-S4 sign-off; 50-node ladder on separate machines |
| US-8.3 | met on branch | #207: read-only `cluster_devices` node MCP tool | none |
| S7.1 | external; owner; buildable | chain #272: CREATE2 redeploy script, book tool that refuses to write without on-chain checks or to strand populated contracts | Chain operator: runbook steps 0 to 5 after owner sign-off. Buildable: runtime `agent-learn` `skill_hash` must use `abi.encode`; add InferenceRouter and CapsuleRegistry to core's optional pins |
| S7.2 | external; owner; buildable | chain #273 | Activation and redeploy; core `precompile_call` helpers and the S7.7 pack update after the formats are final |
| S7.3 | met on branch; owner | as US-7.2 | as US-7.2 |
| S7.4 | owner | | as US-7.1 |
| S7.5 | met on branch; owner; buildable | #206: one wallet card per metric, proven on anvil; closed-day only (review) | Owner: submission shape. Buildable: D-27 fields. External: an AgentSBT on 40204 |
| S7.6 | met on branch | as US-7.4 | Packaged QA |
| S7.7 | buildable | skills now bundled in the base config (#192) | Re-run the literacy eval on T0/T1 with the bundled skills (needs the GPU free) |
| S8.1 | met on branch; external | as US-8.1 | as US-8.1 |
| S8.2 | met on branch; external | #207: deep link, install link and QR, issuer list refresh, pairing hardening (#198) | Two-machine run; Windows Tailscale and firewall check |
| S8.3 | met | | Linux/Windows hardware |
| S8.4 | external; owner | | CL-S4 sign-off, soak and ladder, then the mesh default (stays off; a test fails if it flips) |
| S8.5 | met on branch; owner | #200, #207 | Owner: port 47204 and defaults |
| gate g4-identity | owner; external | | as US-7.1 |
| gate g4-anchor | owner; external | local evidence on #206 | O-5, then one member-approved live anchor |
| gate g4-precompiles | external; owner | chain #273 | Activation, redeploy, an end-to-end model/LoRA call on 40204 |
| gate g4-fleet | external | | Two-machine run with distinct peer ids and the CL-S4 record |

## M6 = 0.4.7 (HUP-S9 Learn together + HUP-S10 Everyday)

| Item | Status | Evidence | Remaining |
|---|---|---|---|
| US-9.1 | external; owner | chain #274, compute-pool #27, settlement #10: ledger with on-chain fraud proofs through 0x0110, independent replay, settlement gate; devnet round end to end with a fixture trainer | 📡 DGX team / chain operator: deploy the ledger, run >= 3 GPU devices with a real trainer, replay, settle (ask on #286). Owner: round rules, `enable_learning` at all, slashing |
| US-9.2 | met on branch (AC2); buildable | #194: `belnap_codec` matches the chain layout; skills bundled (#192) | Re-run the AC1 literacy eval with the bundled skills |
| US-10.1 | owner | | Video backend (A43) |
| US-10.2 | met on branch (buildable part); external | #209: Sheets view, `gsheets_read` / `gsheets_append` (card), schedule and calendar tools, untrusted fences | Owner: Google OAuth client for a live run. Check: whether `sheet_write` undo is covered (M2_STATUS S2.9 says checkpointed; the everyday lane did not verify it from the Sheets view) |
| US-10.3 | met on branch (AC2); external | #209: measured tokens with a labelled estimate fallback, run log, journal bullet; run-log read cap (review) | Person: packaged macOS click-through of widgets and daemons |
| US-10.4 | met on branch (AC1); external | #209: metering, run log and memory facts in the daily entry; daily trigger off by default | Person: native save/open dialogs in the packaged app |
| S9.1 | met on branch (round path); owner | chain #274 | Owner: wire the in-node learning orchestrator at all (consensus change) |
| S9.2 | met on branch (buildable sub-parts); external | as US-9.1 | Live round |
| S9.3 | buildable | runtime export and redaction exist | Core opt-in that sets the trajectory directory per round consent, plus a training-set view |
| S9.4 | met on branch (rest); external | #208: named rounds, per-round consent file, round result bound to the adapter, re-apply after restart; FlRoundGate TLC 4.7 M states | 📡 DGX team: a really trained Gemma 4 E4B GGUF LoRA (or a real round) for the eval delta |
| S10.1 | owner | | Video backend |
| S10.2 | met on branch; external | as US-10.2 | as US-10.2 |
| S10.3 | met | | none |
| S10.4 | met on branch | as US-10.4 | Packaged QA |
| S10.5 | met on branch (probes); buildable; external | #209: node-sync and messaging closed-port probes | Buildable: connect `budget-defaults.json` to the grant cards. Person: recovery kit against the real keychain, delete-then-exit |
| S10.6 | met on branch (axe); external | #209: real-browser axe, 40 runs, 0 findings; dated follow-up audit | Person: VoiceOver (macOS) and NVDA (Windows) passes |
| gate g4-fl | external | | as US-9.1 |
| gate g5-everyday | owner; external | | Video backend, OAuth client, packaged-app evidence |

## M7 = 0.5.0 (HUP-S11 Prove it, and every earlier gate)

| Item | Status | Evidence | Remaining |
|---|---|---|---|
| US-11.1 | external | local hello-mint e2e (#202) | Recorded clean-install runs: macOS (member approves), Linux and Windows (📡 DGX team, steps on #288) |
| US-11.2 | external | unchanged | eval.yml onto main, `EVAL_BASE_URL` set, one dispatch |
| US-11.3 | buildable | three red-team review lanes this run (findings fixed on #197, #198, cluster #10, explorer #17, memories #18) | The final multi-repo pass on the merged heads, second model, register hygiene |
| US-11.4 | buildable | | Almanac pages in citrate-docs (Hermes, node MCP, skills, personas, fleet wizard) |
| S11.0 | met on branch (macOS measured); owner; external | #210: bundle-lite macOS arm64 measured; manual-only size gate in release.yml | Owner: the re-set macOS budgets (DMG 455 MB, payload 938 MB) or apply the strip and llama dedup savings first; which flavour CI releases. 📡 DGX team: Linux and Windows installers measured |
| S11.1 | external | | as US-11.1 |
| S11.2 | external | | as US-11.2 |
| S11.3 | buildable | | as US-11.3 |
| S11.4 | buildable | this PR (scope, retro, status) | gates.yaml refresh (A54), Almanac pages, essay |
| g5-size | owner; external | as S11.0 | as S11.0 |
| g5-os | external | | as US-11.1 |
| g5-redteam | buildable | | as US-11.3 |
| g5-docs | buildable | | as S11.4 |
| g5-everyday | owner; external | | as M6 |
| g0-formal | met on branch | #191 | Flip (A54) |
| g1-approval-audit / g1-no-block / g1-sidebar | met on branch; buildable | #191 | g1-sidebar screenshots; flip (A54) |
| g1-render | buildable; owner | streaming built and proven live (#196, runtime #52) | Buildable: the Markdown golden test. Owner: whether 50 ms / 64-byte batched deltas count as real streaming |
| g1-loop | met on branch; buildable | | Idle-view gap |
| g1-wallet-unlink | external | | identity#31 deploy |
| g1-eval / g1-injection | owner; buildable; external | #194 | Thresholds; browser rows re-score; T2 |
| g2-hic | met (M2 PR) | | |
| g2-knowledge | met (M2 PR) | | Release upload (A45) |
| g2-personas | met (M2 PR) | | |
| g2-mcp | met on branch | #200 | |
| g3-updater | external; owner | | Key ceremony, CVE SLA, security sign-off |
| g3-licence | owner | #210 | Sign-off and published source offer |
| g3-browser | external; buildable | | as M3 |
| g3-gate | met on branch; owner | #202 | Confirm test-only config |
| g3-e2e | external | | Member-approved deploy on 40204 |
| g4-identity / g4-anchor / g4-precompiles | owner; external | chain #272, #273; core #206 | Owner decisions, then chain operator |
| g4-fleet / g4-fl | external | cluster #11, chain #274 | DGX runs and live round |
