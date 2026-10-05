---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed)
updated: 2026-10-05 (owner decision: D-41 amended again, one v0.5.1 for the SCL v0.5.1 gate)
red_teamed: 2026-09-30 (second-model adversarial pass; 26 findings; full detail in the private federation sprint)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core (primary); citrate-agent-runtime, citrate-chain, citrate-memories, citrate-cluster, citrate-sizeup, citrate-explorer, citrate-compute-pool, citrate-settlement (via federation sprint)
tier: T1
supersedes: none (continues 2026-09-11-hermes-agent P0–P5 as P6+)
companions: 01_SCOPE_OF_WORK.md, 02_ARCHITECTURE.md, 03_TLA_SPECS.md, 04_FEATURES_BDD.md, 05_SPRINTS_AND_WPS.md, 06_BUG_TRIAGE.md, 07_BASELINE.md, 08_EXTERNAL_RESEARCH.md, 09_PERSONAS_DRAFT.md, 10_RELEASE_PLAN.md, gates.yaml
---

# Hermes Upskill: Planset Overview

## Why this exists

The [2026-09-11 Hermes planset](../2026-09-11-hermes-agent/00_OVERVIEW.md) (P0–P5)
turned Hermes from a "nothing burger" into a chat agent with a model router, a memory
substrate, a few chain skills and voice input. It still isn't an agent a person would
choose over an out-of-the-box frontier agent. Specifically:

- **Two Hermes brains don't talk to each other.** The chat loop (`src/agent/harness.ts`)
  and the sidecar (`agent-sidecar`, WASM capsules + ApprovalQueue) are separate. The chat
  loop never runs a capsule.
- **It stalls, renders badly, and loses downloads** (see [06_BUG_TRIAGE](06_BUG_TRIAGE.md)).
  All three have code-level root causes.
- **It can't do real work on the machine.** It has no files, no shell, no browser, no
  search, no compile, no test, no audit.
- **It knows little.** The bundle ships 14 docs, while ~99 public docs pages and ~300
  skills sit unused elsewhere in the federation.
- **It doesn't use its own chain.** Memory, HIC decisions, skills, LoRAs, models and
  metering all have on-chain homes (some deployed, some not) that Hermes never touches.

The upstream open-source agent (Nous Research's hermes-agent) shows both the potential
and the failure modes. Its most-cited complaints are self-grading every run as "success"
and baking failures into skills; context bloat from tool schemas; hangs; fragile tool
calls on small models; opaque progress; and unsafe defaults. Sources are in
[08_EXTERNAL_RESEARCH](08_EXTERNAL_RESEARCH.md). We design against each one.

## North star

> **A devrel person opens Citrate Core, types "help me make an NFT project", answers a
> short interview, and within one or two prompts watches a landing page with a working
> mint card come to life in a pop-out browser. Hermes then refuses to deploy until the
> contract passes a real audit gate, and after it does, deploys to chain 40204 through
> the signing ceremony, verifies the source, and pins the site to IPFS.**

The demo template is called **hello mint**. Everything in this planset either makes that
demo real or makes the agent that produces it trustworthy, observable, and useful for
the other everyday tracks (creative, code, project management, fleet ops).

## Core invariant

> **Hermes may propose anything, but every effect on the world (a signature, a
> transaction, a file write outside a granted folder, a shell command off the allowlist,
> a persisted skill or memory) passes through a Human-In-Control gate whose evidence is
> recorded locally and anchored on-chain, and nothing is reported as "done" unless an
> external verifier (not the model) says so.**

## Locked decisions

Locked 2026-09-30 by the owner in an interactive decision pass (43 questions). Reversing
any of these requires a superseding ADR.

| # | Decision | Choice |
|---|---|---|
| D-1 | Demo target | **hello mint**: landing page + ERC-721 mint card on 40204, 1–2-shot from a prompt, visible live in a pop-out browser during development |
| D-2 | Site hosting | Local preview → pin via the bundled kubo → one-click export to Vercel |
| D-3 | Gas | Integrate the existing **faucet** into the node flow (10 SALT per address per 24h). Deploy from the user's SALT via ceremony. 4337 paymaster stays available for minters |
| D-4 | Deploy gate | forge tests pass + Aderyn & Slither zero High + Medusa ≈10 min + anvil fork dry-run → SignatureCeremony. Hermes **refuses** otherwise and explains why |
| D-5 | Default model | Tiered by **citrate-sizeup** hardware receipt (e.g. Qwen 3.6 35B-A3B/27B on capable machines, Gemma 4 E4B on small ones) |
| D-6 | Escalation | To **user-entered API endpoints**, and to **any ModelRegistry CID** via InferenceRouter + x402, paid in SALT. *Amended 2026-09-30 (RT-2):* x402 signatures are a capped EIP-712 domain listed in the Rule-3 amendment ADR |
| D-7 | Runtime | **Hybrid, Rust-owned.** Keep our sidecar + ceremony; adopt the agentskills.io skill format and the good UX ideas from hermes-agent. Do not ship the Python agent |
| D-8 | Loop home | The agent loop **moves into the sidecar**. The UI, the CLI, MCP clients and scheduled daemons all drive one agent; the webview becomes a view |
| D-9 | Persona | A full bundle: voice + skill allowlist + default workflow + tone. Claude drafts ~5 name candidates each; the owner picks |
| D-10 | Tracks | Separate from personas. A track selects a workflow; a persona selects a voice. Launch tracks: Creative, Code, Smart contract + business logic, Project management, Full project |
| D-11 | Browser | App-managed Chromium (default), plus opt-in attach to the user's Chrome via CDP |
| D-12 | Web signing | **SIWE message signatures only** (never transactions), per-origin allowlist, HIC-2 budget, every signature logged. *Amended 2026-09-30 (RT-2/RT-3):* enabled only by a Rule-3 amendment ADR with @rule8 sign-off, and hardened (see Red-team corrections) |
| D-13 | Web search | Bundled **SearXNG** + Jina Reader (page → markdown) |
| D-14 | System-1 decisions | One "typed decision" slot (route/rank/pick-element) with two backends: local grammar-constrained choice (default) and an optional **TypeSafe Jev** adapter (opt-in, data-egress notice). Adopt `typesafe-ai/skills` (MIT) and `system1-agents` browser patterns (Apache-2.0) |
| D-15 | Filesystem | Folder grants cover descendants, never parents; read and write are separate; one central default-deny list (credentials, keychains, browser profiles, wallet extensions, Citrate Core app-data) always applies. *Amended 2026-09-30 (RT-5):* the 24h full-disk toggle is **read-only**; writes only ever go through per-folder grants |
| D-16 | Shell | *Amended 2026-09-30 (RT-1):* **per-call approval by default.** No prompt only for fixed argv templates (e.g. `forge build/test --offline` with ffi off, `npm ci --ignore-scripts`, git with hooks and fsmonitor disabled) run inside an OS sandbox (no network, scratch HOME) in a granted folder |
| D-17 | Media generation | Tiered: local if the GPU fits, else a registry CID / user endpoint |
| D-18 | Office data | Local xlsx/csv in granted folders; optional Google Sheets/Calendar through the existing Connections OAuth. "Calendar" covers the user's calendar and Hermes's own schedule |
| D-19 | Skill format | **agentskills.io SKILL.md** is canonical. Content hash anchored in SkillRegistry. Side-effectful skills compile to signed WASM capsules |
| D-20 | Third-party skills | Trail of Bits + frontend-skills licences cleared for redistribution (owner, 2026-09-30) |
| D-21 | Knowledge delivery | *Amended 2026-09-30 (RT-4/RT-8/RT-9):* the knowledge graph and **curated, reviewed** skills are bundled. Chromium, the Solidity toolchain and SearXNG are **signed, verified first-run components** delivered through an updater with a CVE SLA |
| D-22 | Self-learning | A skill or memory Hermes writes persists only after **an external verifier passes AND the user accepts** |
| D-23 | Anchoring | One **nightly batched root** (HIC decisions + memory root + benchmarks) → AnchorRegistry. *Amended 2026-09-30 (RT-2):* signed by a separate low-value **anchor key that holds no user funds**; user transactions stay HIC-1 |
| D-24 | Agent identity | Deploy AgentSBT + BenchmarkRegistry + CapsuleRegistry in the next redeploy; each install's Hermes mints an AgentSBT at onboarding |
| D-25 | Precompiles | All four: Hermes uses existing precompiles; complete model/LoRA precompile integration; add memory/agent precompiles; Hermes can author precompile PRs (human-reviewed) |
| D-26 | Consensus literacy | Hermes is schooled in **Belnap paraconsistent consensus** ("paraconsensus": Gradient Papers No. II, the 0x0110 precompile, the learning daemon, the memory CRDT) and can extend it |
| D-27 | Metering | Verified task success, speed, SALT cost, resource/energy, plus qualitative and quantitative self-analysis. Full detail local; aggregates on-chain if the user opts in |
| D-28 | Tracking | A live activity monitor in the app **and** the anchored on-chain decision log |
| D-29 | Federated learning | **Goes live in this program, including the hard fork** (reconfirmed 2026-09-30 after RT-21). Tensor aggregation runs off-chain, with Belnap states/digests committed through 0x0110 + settlement. Trajectories are shared only with explicit per-round consent |
| D-30 | Fleet join | Link/QR pairing + opt-in, user-initiated mDNS. No remote push installs |
| D-31 | Device identity | *Amended 2026-09-30 (RT-17):* each device generates its **own random key**, certified by a wallet-signed DeviceLink. Nothing wallet-derived leaves the vault. Faucet eligibility is per member, not per device |
| D-32 | NAT | Tailscale-assisted for v1 |
| D-33 | Mesh default | Clearing the cluster transport security sign-off (CL-S4) is in scope, so the mesh can be on by default |
| D-34 | Sidebar | Consolidation approved (see [02 §UI](02_ARCHITECTURE.md#ui-shell)) |
| D-35 | Widgets | Hermes-authored widgets run in a sandboxed iframe with a typed read-only bridge. Daemons are scheduled skills. Pinned to the Hermes home |
| D-36 | Pop-outs | Browser (dev preview + web), contract reader, activity monitor, code/diff view, media (video/music) player |
| D-37 | Bugs | The three reported bugs are **folded into this program** (HUP-S0), not a separate hotfix |
| D-38 | Journal package | For **app users**: a daily analysis + retro journal of their work, exported as a private encrypted bundle |
| D-39 | Scope discipline | Scoped by **work, not time**. Every feature is completed and tested end to end before the program closes |
| D-40 | Home | This planset (citrate-core) + a federation sprint file for the cross-repo work |
| D-41 | Release cadence (owner, 2026-10-01) | **One release: v0.5.0.** No point releases after 0.4.1. All user stories are finished, the integration branch is tested and QA'd, then it ships once. 0.4.2–0.4.7 are internal milestones only (see 10_RELEASE_PLAN). **Amended 2026-10-01 (owner):** one interim release, **0.4.2**, cut from the integration branch after a packaged QA pass (it carries the downloaded-model fix, the dark-register text fix and the rebuilt Hermes runtime); then nothing until 0.5.0. **Owner decision (2026-10-05): amended again** to allow exactly one **v0.5.1** after v0.5.0, carrying the SCL v0.5.1 release gate (criteria with `release: v0.5.1` in [SCL gates.yaml](../2026-10-05-sidecar-lifecycle/gates.yaml)). v0.5.0 requires only the SCL criteria with `release: v0.5.0` (`g5-scl`) |

## Architecture at a glance

```
┌──────────────────────── citrate-core (Tauri) ────────────────────────┐
│  Webview = VIEW ONLY                                                  │
│  Hermes home · widgets · pop-outs (browser, contract, monitor, diff,  │
│  media) · approval cards · onboarding + fleet wizard                  │
│            ▲ events (SSE)            │ intents                        │
│  Rust shell: SignatureCeremony · grants · keyring · supervisors       │
└────────────┼─────────────────────────┼────────────────────────────────┘
             │ loopback + bearer       ▼
┌──────────── Hermes sidecar (agent-sidecar, Rust) ─────────────────────┐
│ Agent loop (planner/executor) · tool retrieval · verifier gates       │
│ Personas · tracks → workflows · daemons (cron) · metering             │
│ Tool registry (MCP-annotated) · capsules (WASM, signed) · ApprovalQ   │
│ MCP HOST ──┬── mem-mcp (memory graph, Belnap claims)                  │
│            ├── citrate-node MCP (chain read, ceremony-write, pin,     │
│            │   faucet, cluster, precompile helpers)                    │
│            ├── citratescan MCP (explorer, verified source)            │
│            ├── browser (managed Chromium / CDP) · SearXNG · Jina      │
│            └── toolchain (forge/anvil/slither/aderyn/medusa/solc)     │
└───────────────┬─────────────────────────────┬─────────────────────────┘
      llama-server (--jinja, tiered model)    │ escalation: user endpoint │
      Jev adapter (opt-in)                    │ or registry CID via x402  │
                                              ▼
            chain 40204: SkillRegistry · CapsuleRegistry · AgentSBT ·
            BenchmarkRegistry · AgentDecisionRegistryV2 · AnchorRegistry ·
            ModelRegistry · LoRAFactory · InferenceRouter · 0x0110 Belnap
            cluster mesh (libp2p, per-device ids) · compute-pool FL rounds
```

## Surfaces we consume (reuse map)

Nothing on this list gets rebuilt.

| Need | Existing surface | Where |
|---|---|---|
| Sidecar control plane, loopback + bearer | `agent-sidecar` | citrate-agent-runtime |
| Human approval, ceremony bridge | `ApprovalQueue`, `SignatureCeremony`, `bridge_pending` | agent-runtime `hitl/`; core `ceremony.rs`, `hermes.rs` |
| Capsules + signing tiers | capsule manifest/linker/verify/pack | agent-runtime `agent/core/src/capsule/` |
| Folder-scoped grants + secret denylist | `CapabilityGrant.allowed_paths`, `sandbox.rs` | agent-runtime `agent-legacy` |
| Code tools (read/write/edit/search/git/shell allowlist) | `agent-code/src/tools/` | citrate-agent-runtime |
| Memory graph + MCP | `mem-mcp` (11 tools), Belnap confidence, CRDT merge | citrate-memories |
| Docs ingest into memory | `docs_ingest.rs` | citrate-core |
| Model router + registry reads | `modelRouter.ts`, `model_registry.rs`, `model_register.rs` | citrate-core |
| Local skills (markdown) | `skills_local.rs` | citrate-core |
| Contract deploy via ceremony | `contract_deploy.rs` | citrate-core |
| IPFS | bundled kubo, `storage.rs` | citrate-core |
| Explorer MCP (22 read tools) + verification | `/api/mcp`, `/api/verify`, `/api/contract/[addr]` | citrate-explorer |
| Contract templates | CLI `wizard contract` ERC-721, `ColorCirclesNFT.sol`, PatentMint tutorial, radar passport mint | citrate-chain, citrate-docs, citrate-radar |
| Paymaster / AA | ERC-4337 stack (`aaStack`) | citrate-chain, citrate-bundler |
| Faucet | `faucet/` service, `POST /faucet` | citrate-chain |
| Hardware tiering | `sizeup-probe` receipt, `sizeup-core` model fit | citrate-sizeup |
| Mesh + admission | `cluster-core`, `cluster-daemon` | citrate-cluster |
| Group invites | comms `PublishInvite` / `RedeemInvite` | citrate-comms |
| Paraconsensus | `core/learning/belnap.rs`, precompile 0x0110, learning-daemon, ContradictionLedger | citrate-chain |
| FL round | training-coordinator/worker, settlement-core, AggregationChallenge | compute-pool, settlement, chain |
| HIC evidence chain | CapabilityGrant, PolicyBinding, AgentDecisionRegistryV2, AnchorRegistry | citrate-chain `quorum/`, `rbac/` |
| Agent identity + benchmarks | AgentSBT, BenchmarkRegistry, CapsuleRegistry | citrate-chain `cit_agent/` (not yet deployed) |
| Skills corpora | agentile-skills (13), Trail of Bits (74), frontend-skills (36), hermes fork (174) | federation |
| Methodology | Agentile (red-green, document-pass, retro, journal) | agentile-skills |

## Scope

**In (this program):** everything in D-1…D-40, scoped in [01_SCOPE_OF_WORK](01_SCOPE_OF_WORK.md)
and sequenced in [05_SPRINTS_AND_WPS](05_SPRINTS_AND_WPS.md).

**Out (follow-ons):** mobile clients; a hosted Hermes; paid skill marketplace economics;
SSH push installs (D-30); non-SIWE credential autofill (D-12); arbitrary third-party MCP
servers without review (they need an allowlist first); cross-chain committee attestation
(archived research, not the paraconsensus in D-26).

## Safety & compliance gates

Hermes **refuses** to perform an action unless **all** of these hold:

1. **Signing:** every signature goes through the SignatureCeremony, with the decoded
   intent shown. The only exception is SIWE *message* signatures to an allowlisted origin
   inside a live HIC-2 budget (D-12). Transactions are always HIC-1.
2. **Deploy:** the D-4 gate is green for *this exact bytecode* (hash-bound), and the user
   approves the ceremony.
3. **Files:** the path canonicalizes (symlinks resolved) inside a live grant and matches
   no secret-denylist pattern.
4. **Shell:** the binary is allowlisted and the cwd is inside a grant, or the user
   approved this command.
5. **Persistence:** a self-authored skill or memory has a passing verifier record *and*
   a user accept (D-22).
6. **Spend:** escalation, x402 and gas stay inside the session's HIC-2 budget, and the
   budget is visible.
7. **Egress:** any backend that sends user data off-machine (Jev, gateway, user endpoint)
   is opt-in, labelled, and logged.

## Red-team corrections (2026-09-30, supersede the text above)

A second-model adversarial pass produced 26 findings. Security-sensitive specifics are
kept in the private federation sprint. The design corrections below **supersede** any
conflicting text in this planset (00–09):

1. **Rule-3 amendment ADR first** (HUP-S2.0, @rule8 sign-off). No budgeted signature
   ships before it. It lists exactly which signature kinds may be budgeted: hardened
   SIWE and capped x402 EIP-712. The nightly anchor uses a no-funds anchor key.
2. **SIWE hardening:** strict EIP-4361 parse; reject EIP-5573 `resources`
   delegation; `chain-id` allowlisted; `uri` same-origin over https; issued-at
   freshness, expiration cap and not-before enforced; per-origin nonce ledger; origin
   taken from the top-level frame via CDP (never from the page or model); no iframes;
   no budgets in attach-to-Chrome mode; only the user can add allowlist origins.
3. **Taint rule:** once untrusted content (web, search, third-party skills, MCP
   output, verified-source comments, memory from others) is in context, every
   HIC-2/HIC-3 auto-approval downgrades to HIC-1 for the rest of that task.
   Injection eval sets are added to S1.7 and S5.
4. **Verifier purity:** verdicts come from deterministic parsers only. `decide()` never
   classifies verifier output.
5. **Jev egress:** opt-in per origin; never for origins with a session cookie or in
   attach mode.
6. **Node MCP for external agents:** budgets are scoped per client principal (no
   inheritance); Host/Origin checks against DNS rebinding; the token is never readable
   from the managed browser or the workspace; ceremony requests are rate-limited.
7. **Widgets:** `srcdoc` with CSP `default-src 'none'`, no `allow-same-origin`.
   Ceremonies render only in a native window with distinct chrome, never inside
   webview content.
8. **DeployGate binding:** hash(initcode ‖ constructor args ‖ solc version, settings,
   evmVersion).
9. **Fork realism:** anvil lacks Citrate precompiles and GhostDAG semantics, so
   contracts that use Citrate precompiles cannot be READY until a Citrate-aware fork
   (REVM + precompiles) exists (HUP-S6.10).
10. **Medusa budget:** a call/coverage budget (e.g. 50k calls against template
    invariants) with progress streamed, not wall-clock minutes; lower on T0.
11. **Tiers:** T0 is an explicit guided/escalate tier with its own bar. Headline gates
    are measured on T1+.
12. **Sequencing:** federation F-1 (precompile integration) and F-4 (post-reroll
    redeploy) are prerequisites of S1.5, S3 (registry reads) and S6. The AgentSBT is
    removed from the hello-mint preconditions. S1.7 v1 scores synthetic tool-call tasks;
    workflows are re-scored at S6.
13. **Sidecar split:** loop, browser and toolchain run as separate supervised processes.
    S1 includes a parity test against the current harness.
14. **gate0** = specs written + small-bound TLC. Full proofs close with their WPs.
15. **New WPs:** Rule-3 ADR; signed component updater; licence review (AGPL/GPL tools
    vs BUSL app) + source offer; Windows toolchain spike; skill-intake review; central
    denylist; write-tool approval audit; faucet core ADR + measured cost table (test mint
    price 0 on the fork); undo checkpoints for agent file writes; key recovery for
    journal/device keys; offline matrix; accessibility of pop-outs and approval cards;
    telemetry consent; default budget values; uninstall/data deletion; eval provenance
    with train/eval disjointness; persona voice definition; attach-to-Chrome origin
    scoping; installer size budget; Rule-12 drift pre-list (see 05).
16. **Home of this planset:** citrate-core, following the 09-11 Hermes precedent
    (exception to CLAUDE.md rule 6, recorded here). Cross-repo status lives in the
    federation sprint.

Security-sensitive findings discovered while executing this planset are remediated
privately, per owner policy, and tracked in the private federation sprint, not in this
public planset.
