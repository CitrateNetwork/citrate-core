---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core + federation
companions: 04_FEATURES_BDD.md, gates.yaml
---

# Sprints and Work Packages

Effort: S (≤1 day), M (2–4 days), L (1–2 weeks), XL (>2 weeks, split before starting).
Each WP goes red → green → close-with-proof (agentile red-green). Repo tags: **core** =
citrate-core, **rt** = citrate-agent-runtime, **chain**, **mem** = citrate-memories,
**clu** = citrate-cluster, **exp** = citrate-explorer, **pool** = compute-pool,
**setl** = settlement, **sz** = sizeup. Work outside citrate-core is tracked in the
federation sprint (`citrate-federation/.agentile/sprints/active/2026-09-30-hup-hermes-upskill.md`).

## Dependency table

| Sprint | Depends on |
|---|---|
| HUP-S0 Stabilize | — |
| HUP-S1 One agent | S0; S1.5 also needs fed F-1 + F-4 |
| HUP-S2 HIC | S1 (loop in the sidecar) |
| HUP-S3 Knowledge & skills | S1; fed F-4 (registry reads on the redeployed chain) |
| HUP-S4 MCP fabric | S1, S2 |
| HUP-S5 Web | S2, S4 |
| HUP-S6 dApp forge | S2, S3, S4, S5 (browser pop-out + component updater); fed F-4 |
| HUP-S7 Chain-native | S4; federation chain redeploy + precompile work |
| HUP-S8 Fleet | S1; federation cluster work |
| HUP-S9 Learn together | S7, S8; federation FL work incl. the LoRA-precompile hard fork (D-29 reconfirmed) |
| HUP-S10 Everyday work | S2, S4, S5 |
| HUP-S11 Prove it | all |

S2/S3 and S7/S8 can run in parallel. The Rule-3 amendment ADR (S2.0) blocks every budgeted signature (S1.5 x402, S2.3 SIWE, S7.3 anchor).

---

## HUP-S0: Stabilize (bugs folded in, D-37)

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S0.1 | Make `ai_chat*`, `node_status`, `hermes_*`, `model_catalog_search/select`, `model_serve_stop`, `storage_*`, `memory_*`, `social_*` async + `spawn_blocking`; add request timeouts; send `max_tokens`; widen the tripwire to all command files (no network/process I/O in sync commands) | core | M | tripwire test; frame-timing QA |
| S0.2 | Exempt long commands from the 12 s invoke deadline (`UNBOUNDED`), including `model_catalog_download` and chat | core | S | unit test |
| S0.3 | Download robustness: single-flight lock per target; idle-read timeout + auto-resume w/ backoff; require 206 on Range; resume `.part` after restart for catalog models; verify-not-redownload when the final file exists; HF token from Connections | core | M | tests for each case |
| S0.4 | Replace `Markdown.tsx` with a GFM renderer (memoized); remove `**` stripping; golden tests (snake_case, tables, nested lists, fences) | core | S | golden tests |
| S0.5 | llama-server flags: `--jinja`, reasoning format, tier ctx; client guard against template tokens | core | S | test |
| S0.6 | Store: selector subscriptions instead of whole-state; drop the 600 ms global tick in Tauri; per-message render isolation | core | M | render-count test |
| S0.7 | Failed turns render inline with Retry | core | S | test |
| S0.9 | Write-tool approval audit: every chat tool with an effect routes through approval; test enumerates AGENT_TOOLS | core | S | enumeration test |
| S0.8 | Sidebar consolidation (D-34), routing whitelist + theme fixes, one model picker, one chat, Connections into Settings, Community hidden | core | M | nav tests + screenshots |

## HUP-S1: One agent

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S1.1 | Agent loop in the sidecar: session store, event SSE stream, tool registry w/ MCP annotations; webview becomes a view (port `harness.ts` tools) | rt, core | XL→split: loop core (L), event stream + UI view (L) | integration tests |
| S1.2 | Tool retrieval: BGE + keyword top-K over tool descriptions; tokenizer-true budget; compaction-must-shrink | rt | M | budget tests |
| S1.3 | Planner/executor + verifier framework (exit code, test report, SARIF, HTTP, hash); bounded retries; Stop; TLA+ `AgentLoop` | rt | L | TLC + tests |
| S1.4 | Interviewer: per-track question sets, the brief artifact, editable | rt, core | M | BDD |
| S1.5 | Escalation router: user endpoints (keyring), registry CID via InferenceRouter + x402; spend budget; TLA+ `SpendBudget` | rt, core | L | TLC + integration on devnet |
| S1.6 | Sizeup tiering at onboarding; tier → model + ctx; override | core, sz | M | tests on three reference machines |
| S1.7 | HUP eval suite v1: tool-call validity + workflow-step success per tier; pick the tier models | core | L | scorecard committed |
| S1.9 | Process split (loop / browser / toolchain supervised separately) + parity test vs the current `harness.ts` behaviors | rt, core | M | parity suite |
| S1.10 | Injection eval set v1 (malicious pages, skills, MCP output) scored per tier | core | M | scorecard |
| S1.11 | **Wallet unlink** (handed off from the MAC session): the Wallet screen lists linked wallets from the identity registry (`GET /identity/:sub/wallets`), with a two-step Unlink (`DELETE …/wallets/:address`) whose confirmation states the pay-to / custody consequence (HIC); re-reads claim + balances + list afterwards; both commands off the main thread. PR #117. Depends on identity#31 deploy + the private `primary_wallet` follow-up (fed #293) | kit, core, identity | S | cargo +6, vitest +5, tripwire fails on both commands without the async fix |
| S1.8 | CLI: `citrate hermes chat/run/status/stop` against the sidecar | rt | M | CLI tests |

## HUP-S2: Human in control

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S2.0 | **Rule-3 amendment ADR** (@rule8 sign-off): budgetable kinds = hardened SIWE + capped x402 EIP-712; no-funds anchor key; everything else HIC-1 | core | S | signed ADR |
| S2.1 | Folder grants (lift `CapabilityGrant.allowed_paths` + `sandbox.rs`), 24 h full-access toggle, grants UI; TLA+ `FolderGrant`; traversal fuzz | rt, core | L | TLC + fuzz |
| S2.2 | Shell allowlist runner (from `agent-code`) w/ timeouts + capture | rt | M | tests |
| S2.3 | SIWE web-signing budgets (D-12) through the ceremony; Budgets UI; TLA+ `WebSigningBudget` | core, rt | L | TLC + tests |
| S2.4 | Approval cards from annotations (diff, calldata, command) | core | M | BDD |
| S2.5 | Capsule fs/net enforcement via WASI preopens scoped to grants; content-hash + signature verification on every shipped capsule; capsules in all bundle configs | rt, core | L | tests |
| S2.7 | Taint tracking + HIC downgrade (TLA+ `TaintDowngrade`) | rt | M | TLC + tests |
| S2.8 | Central default-deny list (credentials, keychains, browser profiles, wallet extensions, app-data, shell history…) shared by fs/shell/capsules; traversal fuzz | rt | M | fuzz |
| S2.9 | Undo checkpoints for agent file writes (per-step snapshot / git checkpoint in grants) | rt, core | M | tests |
| S2.6 | Local decision records for all HIC-1/2 events | rt | M | tests |

## HUP-S3: Knowledge & skills

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S3.1 | Release-time corpus build: mem-ingest → SyncBundle (docs public-tier, papers, agentile, ToB, frontend-skills, OZ/Solady/Foundry/Medusa/Slither refs, curated hermes-fork skills); first-run import; `skills.lock` | mem, core | L | bundle manifest + import test |
| S3.2 | One skill loader for SKILL.md (+ refs/scripts); description index; `skill_load` tool | rt | M | tests |
| S3.3 | Personas (≥5, names drafted → owner picks) + tracks (5) + workflows per track | rt, core | L | BDD per track |
| S3.4 | Verified self-learning: proposal w/ evidence → accept → persist; SkillRegistry publish (HIC-1); TLA+ `SkillPersistence` | rt, core | M | TLC + tests |
| S3.6 | Skill-intake review: every third-party skill reviewed; scripts stripped or turned into capsules; `skills.lock` with source commit + hash | core | L | review log |
| S3.7 | Persona voice definition (writing style; TTS as an option) + owner picks names from 09_PERSONAS_DRAFT | core | S | owner decision |
| S3.5 | Citrate QA eval set (150 Q) w/ citation checks | core | M | scorecard |

## HUP-S4: MCP fabric

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S4.1 | MCP host in the sidecar (spec 2026-07-28; annotations; Tasks; URL elicitation) | rt | L | conformance tests |
| S4.2 | citrate-node MCP server (resources + read/write tools + cluster + precompile helpers; connect token; ceremony routing) | core | L | tests + an external client (Claude Code) demo |
| S4.3 | Wire mem-mcp and citratescan; add `getVerifiedSource` | core, exp | M | tests |
| S4.4 | User-added MCP servers: allowlist + review screen + env filtering | rt, core | M | tests |

## HUP-S5: Eyes on the web

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S5.1 | Managed Chromium + CDP; ref-indexed snapshots; act; screencast to the Browser pop-out; attach-to-Chrome mode | rt, core | L | tests |
| S5.2 | Bundled SearXNG + read_url (Jina/local readability) | core | M | tests |
| S5.3 | `decide()` System-1 slot: local grammar backend + Jev adapter (opt-in); adopt `typesafe-ai/skills` + system1 browser patterns | rt | M | WebVoyager-subset scores |
| S5.5 | **Signed component updater** (Chromium, toolchain, SearXNG, skills, docs graph) with a CVE SLA (@rule8 updater keys) | core | L | sign-off + update test |
| S5.6 | Attach-to-Chrome origin scoping (per-origin consent; banking/email excluded by default) | core, rt | M | tests |
| S5.4 | Pop-out window framework (Browser, Contract, Monitor, Diff, Media) | core | M | tests |

## HUP-S6: dApp forge

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S6.0 | Windows toolchain spike (slither/medusa/aderyn/foundry) + licence review (AGPL/GPL tools vs BUSL app; source offer) | core | M | spike report + legal note |
| S6.1 | Toolchain bundle (solc 0.8.36, foundry, slither, aderyn, medusa, node) per OS; vendored OZ + Solady | core | L | install tests per OS |
| S6.2 | Templates: OZ Wizard params (20/721/1155/governor), hello-mint (vite+wagmi+viem, mint card), Solady variant; template invariants for Medusa | core | L | template tests |
| S6.3 | Toolchain MCP tools w/ JSON/SARIF verifiers | rt | M | tests |
| S6.4 | D-4 deploy gate + verdict card + bytecode-hash binding into `contract_deploy`; TLA+ `DeployGate` | core | L | TLC + BDD |
| S6.5 | Faucet integration (anti-abuse handled per federation sprint, budget) | core, chain | M | integration |
| S6.6 | Post-deploy: verify, switch the site to 40204, IPFS pin, Vercel export | core | M | BDD |
| S6.8 | Faucet core ADR (supersedes ADR-2026-07-27's "no faucet" for deploy gas) + measured 40204 cost table (deploy, verify, mint, SBT, anchor) | core | S | ADR + measurements |
| S6.9 | Medusa call/coverage budgets per tier | core | S | tests |
| S6.10 | Citrate-aware fork (REVM + Citrate precompiles) for dry-runs of precompile-using contracts | core, chain | L | tests |
| S6.7 | Contract reader pop-out w/ explain + view/write | core | M | BDD |

## HUP-S7: Chain-native agent

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S7.1 | Redeploy set incl. AgentSBT, BenchmarkRegistry, CapsuleRegistry; regenerate 40204.json | chain | M | on-chain reads |
| S7.2 | Precompile work (federation sprint): model/LoRA precompile integration, LoRA precompile, memory/agent precompiles; Hermes `precompile_call` helpers | chain, core | XL→split in fed sprint | devnet + fork tests |
| S7.3 | Nightly anchor batch; inclusion proofs; TLA+ `AnchorBatch` | rt, core | M | TLC + tests |
| S7.4 | AgentSBT mint at onboarding | core | S | BDD |
| S7.5 | Metering records + daily report + opt-in BenchmarkRegistry | rt, core | M | tests |
| S7.6 | Activity monitor pop-out ("why am I waiting", ctx meter, spend, Stop) | core | M | BDD |
| S7.7 | Paraconsensus + precompile literacy pack (skills + eval questions) | core | M | eval |

## HUP-S8: Fleet

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S8.1 | Per-device subkeys + DeviceLink; roster lists devices; PeerId from device key; TLA+ `DeviceLink` | core, clu | L | TLC + two-machine test |
| S8.2 | Fleet wizard UI + sizeup probe + opt-in mDNS + link/QR pairing | core, sz | L | BDD |
| S8.3 | Tailscale-assisted connectivity guidance + detection | core | M | runbook + test |
| S8.4 | CL-S4 transport sign-off; soak + ladder; mesh default on | clu | L | sign-off record |
| S8.5 | Cluster + invite MCP tools | core | S | tests |

## HUP-S9: Learn together

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S9.1 | Enable the learning orchestrator path where required; cluster-scoped round config | chain | M | devnet |
| S9.2 | Round e2e: coordinator ↔ device workers ↔ 0x0110 ↔ settlement ↔ challenge ↔ LoRAFactory/ModelRegistry | pool, setl, chain | XL→split | replay digest match |
| S9.3 | Trajectory export (verified, redacted) → training set | rt | M | redaction tests |
| S9.4 | Hermes-side: plan/explain/start rounds (HIC-1), load resulting LoRA, eval gate | core | M | eval delta |

## HUP-S10: Everyday work

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S10.1 | Image/video gen tiers + Media pop-out | core | L | tests |
| S10.2 | Sheets (local + Google), calendar (Google + Hermes schedule) | core | M | tests |
| S10.3 | Widgets (sandbox + bridge) + daemons (scheduler, budgets) | core, rt | L | BDD |
| S10.5 | Key recovery for journal + device keys; offline matrix; uninstall / data deletion; telemetry consent; default budget values | core | M | tests + docs |
| S10.6 | Accessibility pass on pop-outs + approval cards | core | M | a11y audit |
| S10.4 | User journal/retro daily entry + encrypted export | core | M | tests |

## HUP-S11: Prove it

| WP | Work | Repo | Effort | Proof |
|---|---|---|---|---|
| S11.0 | Installer size budget check per OS (core installer ≤ budget; components measured) | core | S | CI size report |
| S11.1 | hello-mint E2E per OS from a clean install | core | L | recorded runs |
| S11.2 | Eval suite in CI + scorecard | core | M | CI |
| S11.3 | Red-team pass (second model) + remediation (private) | all | L | report |
| S11.4 | Document pass (Almanac pages) + retro + essay | core, docs | M | PRs |

## Rule-12 drift pre-list

Add `[[drift]]` entries to `citrate-federation/manifest.toml` **before** the Cargo edits:

| Consumer | Dependency | WP |
|---|---|---|
| citrate-agent-runtime (sidecar) | `agent-legacy` grants/sandbox lifted into the sidecar crate | S2.1 |
| citrate-agent-runtime (sidecar) | citrate-memories `mem-mcp` client | S4.1 |
| citrate-core | citrate-sizeup `sizeup-core` / probe library | S1.6, S8.2 |
| citrate-core / citrate-cluster | device-key roster types in `cluster-core` | S8.1 |
| citrate-core | citrate-chain `faucet` API types (if shared) | S6.5 |
| citrate-compute-pool / settlement | chain Belnap + LoRA precompile ABI | S9.2 |

New npm dependencies (e.g. the GFM renderer) go through the supply-chain review skill
before they land.

## Eval provenance

The Citrate QA set (S3.5), the tool-call suite (S1.7) and the injection set (S1.10) are
authored by named maintainers, versioned, and **kept disjoint** from the E9 training
trajectories. A leakage check runs before any LoRA eval.
