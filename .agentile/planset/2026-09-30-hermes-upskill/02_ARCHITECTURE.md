---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core + federation
companions: 00_OVERVIEW.md, 03_TLA_SPECS.md
---

# Architecture

## 1. Ontology: what Hermes is made of

Every capability belongs to exactly one of these kinds. Mixing them is how agents bloat
context and lose control.

| Kind | What it is | Format | Where it lives | Loaded into context | Can cause effects? | On-chain home |
|---|---|---|---|---|---|---|
| **Tool** | A typed primitive the model calls | JSON schema + MCP annotations | Sidecar tool registry / MCP servers | Only the top-K retrieved for this turn | Yes, gated by its annotation | none |
| **Skill** | Know-how: when and how to use tools for a job | agentskills.io `SKILL.md` + bundled refs/scripts | Memory graph (`skills` tenant) + disk | Name + description always (~30 tok); body on demand | No (instructions only) | SkillRegistry (hash + CID) |
| **Capsule** | A signed, sandboxed side-effecting skill | WASM + `manifest.toml` + `.cps` | Sidecar capsule dir | Via its tool wrapper | Yes, through the ApprovalQueue | CapsuleRegistry |
| **Workflow** | A graph of steps with **verifier gates** between them | YAML (steps, skills, tools, verifiers, exit) | Bundled + user-authored | Current step only | Through its steps | Decision records |
| **Verifier** | An external check that decides pass/fail, never the model | Command + parser (exit code, test report, SARIF, HTTP status, bytecode hash) | Sidecar | Result summary only | No | BenchmarkRegistry (aggregates) |
| **Persona** | A voice + skill allowlist + default workflow + tone | `persona.toml` + system prompt | Bundled + user-authored | System prompt | No | none (local) |
| **Track** | A user goal that selects a workflow family | `track.toml` | Bundled | Via the workflow | No | none |
| **Daemon** | A scheduled or triggered skill/workflow run | cron expr + trigger + budget | Sidecar scheduler | When it fires | Within its budget | Decision records |
| **Widget** | A Hermes-authored UI tile | Sandboxed HTML/JS + declared read-only data needs | Hermes home grid | n/a | **No** (read-only bridge) | none |
| **Memory** | Facts, episodes, claims with Belnap confidence | mem-core nodes/edges | Memory graph tenants | Retrieved by `memory.search` | Writes gated by D-22 | Nightly root (AnchorRegistry) |
| **LoRA** | A weight adapter specializing a base model | GGUF adapter + commitment | Models dir; IPFS CID | n/a | n/a | LoRAFactory + ModelRegistry |

**Rules that follow:**
1. **Few tools, many skills.** Around 15 generic tools. Domain knowledge lives in skills,
   which cost almost nothing until used.
2. **Anything side-effecting is a tool or a capsule, never a skill.** A skill can only
   *tell* the model to call a gated tool.
3. **Only verifiers produce "success".** The model's opinion of its own work is recorded
   as a qualitative note, never as the task outcome.

## 2. The agent loop (sidecar-owned, D-8)

```
user msg / daemon trigger / MCP call / CLI
   │
   ▼
Interviewer ──(unclear goal?)──► ask 3–7 structured questions (elicitation)
   │ goal + constraints
   ▼
Planner (local, or escalated per D-6) ──► selects track → workflow, persona, skills
   │ plan = workflow instance
   ▼
Executor loop, per step:
   tool retrieval (top-K by embedding + keyword over tool descriptions)
   → grammar-constrained tool call (llama-server --jinja)
   → HIC gate (annotation + grant + budget) → execute
   → observe → verifier(s) → pass: next step │ fail: retry ≤N, then re-plan, then ask the user
   │
   ▼
Reporter: outcome = verifier verdicts; metering record; journal entry; decision records
```

- **Bounded:** max steps per workflow, max retries per step, and a wall-clock deadline
  per tool. Stop is always available and takes effect within one tool deadline (TLA+
  `AgentLoop`).
- **Interview first:** for build tasks, Hermes runs the track's question set before
  writing anything, then writes a *task brief* (goal, constraints, chosen skills, gates)
  that the user can edit. This is the "ask and organize a series of questions prior to
  development" requirement.
- **Context budget:** measured with the model's own tokenizer (llama-server `/tokenize`).
  Sections: system+persona, task brief, current step, retrieved tools, retrieved
  skills/memory, recent turns, compacted history. Compaction must shrink the prompt or
  it is rejected. `ctx-size` comes from the tier.
- **Events:** the sidecar emits a typed SSE stream (`plan`, `step_start`, `tool_call`,
  `approval_needed`, `verifier`, `token`, `step_end`, `metering`, `error`). The webview
  renders it. Real token streaming replaces today's simulated replay.

## 3. Models, tiers, escalation (D-5, D-6, D-14, D-17)

| Tier (from sizeup receipt) | Local chat model (example) | ctx | Media | Notes |
|---|---|---|---|---|
| T0 (<12 GB usable) | Gemma 4 E4B Q4 | 16k | registry/endpoint only | Planner escalation recommended |
| T1 (12–24 GB) | Qwen 3.6 27B Q4 / Gemma 4 26B | 32k | small image locally | |
| T2 (≥24 GB or ≥16 GB VRAM) | Qwen 3.6 35B-A3B | 64k | image + short video | |

The concrete picks are finalized by the **HUP-S1 eval gate**: a tool-call reliability
score per tier on our own workflows, not public benchmarks.

**Escalation router:** it extends `modelRouter.ts` sources (local, registry, gateway)
with:

- **User endpoints:** OpenAI-compatible URL + key, sealed in the keyring.
- **Registry CIDs:** InferenceRouter request + x402 payment in SALT.

Every escalation shows its cost and destination and is charged against the HIC-2 budget.

**System-1 slot (D-14):** a `decide(options[], context) → {choice, probs}` primitive used
for routing, ranking, picking the next browser element, and classifying verifier output.
It has two backends:
- **local** (default): grammar-constrained choice on the local model;
- **jev** (opt-in): the TypeSafe API, with an egress notice.

## 4. Human In Control (HIC)

| Level | Applies to | Mechanism |
|---|---|---|
| HIC-0 observe | Reads (chain reads, memory search, file reads inside a grant) | Logged only |
| HIC-1 approve each | Every transaction, key op, deploy, off-allowlist shell, writes outside grants, first use of an egress backend | SignatureCeremony / approval card |
| HIC-2 budgeted | Only kinds listed in the Rule-3 amendment ADR: hardened SIWE per origin (D-12), capped x402 EIP-712 (D-6), faucet requests (no signature), templated sandboxed shell (D-16). The nightly anchor is signed by a no-funds anchor key (D-23) | A budget object (scope, cap, expiry) granted via ceremony, visible, revocable |
| HIC-3 post-hoc | Daemons within budget; widget refresh | Reviewed in the activity monitor + journal |
| HIC-X | Anything ungoverned | Alarm state; the loop halts |

- **Folder grants (D-15):** reuse `CapabilityGrant.allowed_paths` + `sandbox.rs` from
  `agent-legacy`, lifted into the sidecar. Paths are canonicalized before the check, the
  secret denylist applies at any depth, read and write are granted separately, and grants
  can't widen to a parent. Full-disk access is an HIC-1 grant with a 24h TTL. The
  capsule linker gets real WASI preopens scoped to the same grants, which closes the
  current "reject any fs/net manifest" limitation.
- **Web signing (D-12):** a `WebSigningBudget {origin, chainId, maxCount, expiresAt}`.
  Only EIP-4361 messages whose `domain` equals the page origin and whose URI is on that
  origin qualify. Transactions and EIP-712 permits never qualify.
- **Evidence:** every HIC decision becomes a local decision record → Merkle batch → the
  nightly anchor (D-23): `AgentDecisionRegistryV2` + `AnchorRegistry`.

## 5. MCP fabric (E4)

Hermes is an **MCP host** (client), targeting the current MCP spec (2026-07-28:
stateless requests, `_meta` capabilities, tool annotations, URL-mode elicitation for
sensitive steps, Tasks extension for long operations). Because the spec deprecated
Roots, folder scoping is enforced by the host (§4), not delegated to servers.

| Server | Transport | Tools / resources (summary) | Status |
|---|---|---|---|
| **mem-mcp** | Unix socket (existing) | `memory.recall/search/neighbors/as_of/verify/critique/analogy/propose_edge/confirm_edge/assert/merge_diff` | exists; wire into the host |
| **citrate-node MCP** (new, in citrate-core) | stdio + loopback streamable HTTP | Resources: node status, account, balances, ABI registry, deployed addresses, precompile table. Read tools: `chain_call`, `estimate_gas`, `get_logs`, `precompile_call` (0x0107–0x0111, 0x0120, 0x0200–0x0202 helpers), `dag_stats`. Write tools (`destructiveHint`): `tx_propose` (→ ceremony), `deploy_propose`, `pin_add`, `faucet_request`, `anchor_propose`. Cluster: `cluster_status/peers/join/share`. Tasks: deploy-and-confirm, sync, FL round | new |
| **citratescan MCP** | HTTP (existing) | 22 read tools + a new `getVerifiedSource` | exists; add one tool |
| **browser** | in-sidecar | `open`, `snapshot` (ref-indexed accessibility tree, ~300 tok/page), `act(ref, action)`, `screenshot`, `console`, `network`, `siwe_sign` (→ HIC-2) | new |
| **search** | in-sidecar | `web_search` (SearXNG), `read_url` (Jina Reader / local readability) | new |
| **toolchain** | in-sidecar | `forge_build/test`, `anvil_fork`, `slither`, `aderyn`, `medusa_fuzz`, `solc_compile`, `oz_wizard(kind, params)` | new |
| **fs / shell** | in-sidecar | `fs_read/list/search/write/edit`, `shell_run` (allowlist) | lift from `agent-code` |
| **office / media** | in-sidecar | `sheet_read/write`, `calendar_list/create`, `image_gen`, `video_gen` | new |

The citrate-node MCP server is also exposed to *external* agents (Claude Code, Cursor,
other Hermes installs) behind a connect token and the same HIC rules. This is how
"MCP services on top of Citrate" become useful: one policy-enforcing surface over the
node.

## 6. Knowledge (E3, D-19, D-21, D-22)

- **Bundled graph:** built at release time by `mem-ingest` (deterministic IDs) into a
  memdag `SyncBundle`, and imported on first run. Tenants:
  - `citrate-docs`: public `citrate-docs/content` (tier: public only) + gradient papers
    + the chain precompile/RPC reference;
  - `skills`: all SKILL.md files, indexed by description;
  - `refs`: OpenZeppelin, Solady, Foundry, Medusa, Slither docs;
  - `methodology`: agentile;
  - `personal` and `chain-state`: runtime.
- **Skill index:** descriptions embedded with BGE. Per turn, the retriever surfaces
  ≤5 skills and the model loads bodies with `skill_load`.
- **Self-learning:** after a workflow passes its verifiers, Hermes may *propose* a skill
  or memory. The proposal carries the verifier evidence. The user accepts or rejects.
  Accepted skills get hashed; publishing to SkillRegistry is an explicit HIC-1 action.
- **Belnap claims:** memory assertions carry Belnap confidence. A `Both` (contradiction)
  halts reliance on that claim and surfaces it to the user. On-chain escalation goes to
  `ContradictionLedger`.

## 7. The dApp forge and hello mint (E6)

```
interview (name, supply, price, art, allowlist?, chain=40204)
 → template: OZ Wizard ERC-721 (params) | hello-mint (ColorCircles/PatentMint lineage) | Solady variant
 → forge build → forge test (generated + template tests)
 → aderyn + slither (SARIF) → zero High required
 → medusa (≈10 min, template invariants: supply cap, price, ownership, reentrancy)
 → anvil fork of 40204 → deploy + mint dry-run → site preview wired to the fork
 → verdict card: READY / NOT READY (+ why, + fixes)
 → faucet top-up if needed (HIC-2) → deploy_propose → SignatureCeremony (bytecode hash must match the gated artifact)
 → verify (citrate_verifyContract + explorer) → site switches to 40204 → pin to IPFS → optional Vercel export
```

- The **browser pop-out** shows the dev server live throughout. The **contract reader**
  pop-out opens any address and shows verified source + ABI, lets the agent explain it,
  and offers read calls (HIC-0) and write calls (HIC-1).
- **Toolchain bundle:** solc (pinned 0.8.36 to match the chain), foundry
  (forge/anvil/cast), slither (+ Python runtime), aderyn, medusa, node, and a
  vite/wagmi/viem app template, with vendored OpenZeppelin + Solady.

## 8. Chain-native agent (E7)

| Purpose | Contract / precompile | Action |
|---|---|---|
| Agent identity | AgentSBT | Mint at onboarding (after the redeploy, D-24) |
| Self-benchmarks | BenchmarkRegistry | Aggregates in the nightly batch (opt-in, D-27) |
| Skills | SkillRegistry | Read at start; publish via HIC-1 |
| Capsules | CapsuleRegistry | Verify tier/signature before load |
| HIC evidence | AgentDecisionRegistryV2 + AnchorRegistry | Nightly root (D-23) |
| Memory root | AnchorRegistry (kind=memory) | Nightly root |
| Models / LoRAs | ModelRegistry, LoRAFactory, InferenceRouter | Register, route, escalate; after the model/LoRA precompile integration (federation sprint) |
| Paraconsensus | 0x0110 BELNAP_AGGREGATE | FL aggregation, claim reconciliation |
| Contradictions | ContradictionLedger | Escalate unresolved `Both` |
| New precompiles | memory-anchor / agent-ops (D-25) | Specified in the federation sprint (hard fork) |

**Metering (D-27):** a per-task record: verifier verdicts, wall time, TTFT, tokens/sec,
tokens in/out, retries, escalations + SALT spent, gas, CPU/GPU/RAM peak, an energy
estimate, the model tier, and a qualitative self-review (clearly labelled as the model's
opinion). Stored locally; aggregates are anchored if the user opts in. Surfaced in the
activity monitor and the daily journal.

## 9. Fleet and learning (E8, E9)

- **Device identity (D-31):** `device_key = derive(wallet, "citrate/device", index)` plus
  a wallet-signed `DeviceLink {device_pub, index, label, issued_at}`. The cluster roster
  lists devices under a member. The libp2p PeerId comes from the device key.
- **Wizard:**
  1. The sizeup probe runs locally.
  2. Opt-in mDNS lists peers already running Citrate Core.
  3. Other machines install via a link/QR carrying a one-use pairing token.
  4. Pairing issues a DeviceLink.
  5. Tailscale-assisted connectivity: detect or install guidance, tailnet addresses as
     bootstrap.
  6. A group is formed and invites go out through comms.
- **FL (E9):** a cluster round:
  1. The training-coordinator leases work to member devices.
  2. Each worker trains a LoRA delta on verified Hermes trajectories.
  3. Signed zone deltas are submitted.
  4. Belnap aggregation runs through 0x0110 (Q16).
  5. The round is committed via settlement.
  6. The challenge window runs.
  7. The adapter is recorded in LoRAFactory and registered in ModelRegistry.

  Hermes can plan, explain, and (under HIC-1) start rounds.

## 10. UI shell

**Sidebar (D-34):**

```
◉ Hermes            home: chat + pinned widgets + activity strip
─ Build ─   Projects · Files (IPFS + memory) · Models
─ Money ─   Wallet
─ Network ─ Node · Fleet
─ People ─  Groups   (tabs: Chat · Members · Cluster · Training · Alerts)
─ ─ ─       Journal · Commissary · Settings (Connections + AI providers + Grants + Budgets)
```

- Community is hidden until wired. There is one model picker (in chat), one chat
  surface, and one Connections home.
- `people`/`alf` routing + theme gaps are fixed.
- **Pop-outs (D-36):** separate Tauri windows, each subscribed to the sidecar event
  stream: Browser, Contract reader, Activity monitor, Code/diff, Media player.
- **Widgets (D-35):** sandboxed `iframe` (no network, `sandbox="allow-scripts"`), data
  via a typed `postMessage` bridge exposing declared read-only queries only.

## 11. Data sources (Rule 7)

Every new surface names its data source before implementation:

| Surface | Data source |
|---|---|
| Tiering | sizeup receipt (local probe) |
| Tool retrieval | the sidecar tool registry + BGE index |
| Skills | bundled memdag `skills` tenant + SkillRegistry (40204) |
| Chain tools | `rpc.citrate.ai` / local node JSON-RPC |
| Verified source | citrate-explorer `/api/contract/[addr]` + `citrate_getVerification` |
| Faucet | `POST https://faucet.citrate.ai/faucet` |
| Deploy gate | forge/slither/aderyn/medusa outputs (JSON/SARIF) |
| Metering | the sidecar's own measurements + llama-server `/metrics` + OS counters |
| Fleet | sizeup receipts, mDNS, cluster-daemon IPC, comms invites |
| FL | training-coordinator HTTP, settlement intents, 0x0110 |
| Search | local SearXNG |
| Journal | the sidecar event log + metering records + memory `personal` tenant |
