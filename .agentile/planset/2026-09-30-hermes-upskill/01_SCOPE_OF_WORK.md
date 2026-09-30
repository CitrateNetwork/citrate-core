---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core (primary) + federation repos
companions: 00_OVERVIEW.md, 05_SPRINTS_AND_WPS.md
---

# Scope of Work

Scoped by work, not time (D-39). A phase closes when its gate in [gates.yaml](gates.yaml)
is `met` with evidence. It never closes on a date.

## Epics

| Epic | Name | Outcome | Sprint(s) |
|---|---|---|---|
| E0 | **Stabilize** | No pinwheel. Downloads finish and resume. Agent text renders correctly with real streaming. Sidebar consolidated | HUP-S0 |
| E1 | **One agent** | Loop lives in the sidecar. Tool retrieval, planner/executor, verifier gates, context budget, tiered model, escalation router | HUP-S1 |
| E2 | **Human in control** | Folder grants, shell allowlist, 24h full access, SIWE HIC-2 budgets, capsule fs/net enforcement, approval cards driven by tool annotations | HUP-S2 |
| E3 | **Knowledge & skills** | SKILL.md canonical, the full corpus bundled as a memory graph, progressive disclosure, verified self-learning, SkillRegistry write, personas + tracks | HUP-S3 |
| E4 | **MCP fabric** | Hermes is an MCP host; the citrate-node MCP server; mem-mcp + citratescan wired; spec 2026-07-28 | HUP-S4 |
| E5 | **Eyes on the web** | Managed browser + CDP attach, SearXNG + Jina, System-1 decision slot (local + Jev), pop-out windows | HUP-S5 |
| E6 | **dApp forge** | Bundled Solidity toolchain, templates (OZ Wizard, Solady, hello mint), the D-4 audit gate, faucet, ceremony deploy, verify, IPFS pin, Vercel export, contract reader | HUP-S6 |
| E7 | **Chain-native agent** | Redeploy set incl. AgentSBT/BenchmarkRegistry/CapsuleRegistry, model/LoRA precompile integration + new precompiles, nightly anchor, metering, activity monitor | HUP-S7 |
| E8 | **Fleet** | Per-device identity, fleet wizard (sizeup + mDNS + link/QR), Tailscale-assisted mesh, CL-S4 sign-off, cluster agent tools, invites | HUP-S8 |
| E9 | **Learn together** | Paraconsensus FL rounds live across clusters; Hermes LoRA trained on verified trajectories; LoRA registered and completed on-chain | HUP-S9 |
| E10 | **Everyday work** | Image/video tiers, media player, spreadsheets, calendar, widgets + daemons, user journal/retro package | HUP-S10 |
| E11 | **Prove it** | hello-mint E2E from a clean install, agent eval suite, red-team, docs pass, release | HUP-S11 |

## Deliverables

- The sidecar-owned agent loop, with a typed event stream to the UI.
- The tool registry with MCP annotations (`readOnlyHint`, `destructiveHint`,
  `idempotentHint`, `openWorldHint`) driving the approval UI.
- The **citrate-node MCP server**, stdio + streamable HTTP on loopback.
- A bundled knowledge graph: public `citrate-docs/content`, gradient papers, agentile,
  Trail of Bits, frontend-skills, OZ/Solady/Foundry refs, and the curated hermes-fork
  skills.
- Personas (≥5) and tracks (5), each with a workflow and a verifier set.
- The Solidity toolchain bundle, templates, and the D-4 gate.
- Pop-out windows ×5 (D-36).
- The fleet wizard.
- A live paraconsensus FL round and a registered Hermes LoRA.
- An on-chain agent identity, the nightly anchor, and self-benchmarks.
- The agent eval suite with a tool-call reliability score per model tier.
- The user journal/retro export.

## Risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-1 | A small local model can't drive long workflows reliably | High | High | Few generic tools + retrieved skills; grammar-constrained tool calls (`--jinja`); planner escalation (D-6); workflows with hard verifier gates, so the model only fills slots |
| R-2 | Context budget (today 8k) is overrun by tool schemas | High | High | Tool retrieval (top-K per turn), measure with the real tokenizer, compaction that must shrink, ctx sized by tier |
| R-3 | The toolchain bundle (solc/forge/slither/aderyn/medusa + Chromium + SearXNG) bloats the installer | High | Med | D-21 says bundle everything; measure; content-addressed first-run unpack; per-OS builds |
| R-4 | The web-signing budget becomes a phishing vector | Med | High | SIWE-only, origin-bound (EIP-4361 `domain` must equal the page origin), allowlist, budget caps, per-signature log, one-click revoke; TLA+ `WebSigningBudget` |
| R-5 | The agent writes an insecure contract and the gate misses it | Med | High | Template-first (fill parameters, not free-form); multi-tool gate; bytecode-hash binding; "not production ready" verdicts are explicit; human ceremony |
| R-6 | Chain redeploy / hard fork (precompiles) delays E7/E9 | Med | High | Federation sprint owns chain work; Hermes-side work builds against anvil + a devnet with the new precompiles first |
| R-7 | FL live depends on the mesh sign-off (CL-S4) and the learning orchestrator being enabled | High | High | Sequence E8 before E9; cluster-local round first, then cross-cluster |
| R-8 | Self-learning poisons skills/memory | Med | High | D-22 double gate; provenance on every skill; Belnap "Both" = stop and ask |
| R-9 | Third-party egress (Jev, gateway, endpoints) leaks user data | Med | Med | Opt-in, labelled, logged; local default for every slot |
| R-10 | Tool-call formats differ across model tiers | High | Med | Per-tier eval gate in E1; `--jinja` native templates; format adapters tested per model |
| R-11 | Pinwheel regressions reappear as commands are added | Med | Med | Tripwire widened to every `#[tauri::command]`; lint: no sync command may do network or process I/O |
| R-12 | Bundling third-party skills drifts from upstream | Low | Low | `skills.lock` with source commit + hash; update job |
