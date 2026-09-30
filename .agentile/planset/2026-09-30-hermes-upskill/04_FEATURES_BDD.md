---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core + federation
companions: 02_ARCHITECTURE.md, 05_SPRINTS_AND_WPS.md
---

# User Stories, Acceptance Criteria, BDD

## How to read this

- Stories are `US-<epic>.<n>`. Each has acceptance criteria (AC) with a **data source**
  (Rule 7).
- Gherkin scenarios are given for the flows that gate the program.
- A story is done only when every AC is proven by a test or a recorded run named in
  gates.yaml.
- Personas used below: **Dev** = devrel demoer; **Member** = a new paying member;
  **Operator** = someone running several machines; **Builder** = a developer extending
  Hermes.

---

## E0 Stabilize

**US-0.1: No beachball.** As a Member, I can chat with Hermes while the node syncs
without the app freezing.
- AC1: No `#[tauri::command]` performs network, process, or model I/O synchronously.
  The tripwire test scans every command-bearing file. *Source: `lib.rs` tripwire.*
- AC2: The UI main thread never blocks >100 ms during a 2k-token local reply on a T0
  machine. *Source: frame-timing probe in the QA run.*
- AC3: Every chat request sends `max_tokens` and has a deadline; Stop cancels generation
  on llama-server within 1 s.

**US-0.2: Downloads finish and survive restarts.** As a Member, a model download
continues after a timeout, Wi-Fi change, sleep, or app restart, and never restarts from
zero unnecessarily.
- AC1: Long-running commands are exempt from the 12 s invoke deadline. Progress events
  keep flowing.
- AC2: A single-flight lock per target file; a second click attaches to the running
  download.
- AC3: An idle-read timeout (e.g. 60 s) triggers an automatic resume with backoff; a
  `Range` response must be 206 or the resume is refused up front.
- AC4: On launch, every `.part` is listed as resumable; a finished-but-unverified file
  goes to verify, not to re-download.
- AC5: Gated HF repos accept a token from Connections. *Source: HF API.*

**US-0.3: Readable agent text.** As a Member, Hermes's replies render as proper
markdown while they stream.
- AC1: snake_case identifiers, file names, and `**bold**` render correctly (golden
  tests).
- AC2: GFM tables, nested lists, numbered lists with blank lines, and fenced code with
  any language tag render.
- AC3: Real streaming via SSE from the sidecar. No simulated replay. No reflow jump at
  completion.
- AC4: Chat-template and reasoning tokens never appear (llama-server `--jinja`
  `--reasoning-format` set; a client-side guard test).
- AC5: A failed or timed-out turn shows an inline error with Retry. It never vanishes.

**US-0.4: A calmer sidebar.** As a Member, I see about 10 navigation items grouped by
intent (D-34).
- AC1: The sidebar matches 02 §10. Community is hidden. People/Cluster live as Groups
  tabs. Deep links for every route work.
- AC2: One model picker, one chat surface, and Connections lives only in Settings.

---

## E1 One agent

**US-1.1: One brain everywhere.** As a Builder, the same Hermes answers in the app, the
CLI, and MCP clients, and runs scheduled daemons.
- AC1: The loop runs in the sidecar. The webview holds no agent logic.
- AC2: `citrate hermes chat`, `citrate hermes run <workflow>`, and an MCP client all
  reach the same session store.
- AC3: Killing the webview does not kill a running workflow; reopening reattaches to the
  event stream.

**US-1.2: Interview before building.** As a Dev, when I say "help me make an NFT
project", Hermes asks a short, structured set of questions and writes a brief I can
edit before it builds anything.
- AC1: Each track defines a question set (3–7 questions, with defaults).
- AC2: The brief lists the goal, constraints, chosen persona/skills/workflow, and the
  gates that will apply.
- AC3: "Just use defaults" skips straight to the brief.

**US-1.3: Only verifiers say "done".** As a Member, Hermes never claims success unless
an external check passed.
- AC1: Each workflow step declares ≥1 verifier. The step outcome is the verifier verdict.
- AC2: The model's self-review is stored separately, labelled "opinion".
- AC3: TLA+ `AgentLoop` `OnlyVerifierSucceeds` is green.

**US-1.4: Small model, big toolbox.** As a Member on a T0 laptop, tool calls stay
reliable.
- AC1: ≤8 tool schemas in context per turn (retrieved top-K). Token counts come from
  the model's tokenizer.
- AC2: Per-tier eval: ≥90% valid tool calls and ≥80% workflow-step success on the HUP
  eval suite. *Source: `eval/` runs.*
- AC3: llama-server runs with `--jinja` and the tier's ctx size.

**US-1.5: Escalate when needed, at a price I see.** As a Member, Hermes can hand a hard
planning step to my own endpoint or a registry model, and pay for it in SALT within my
budget.
- AC1: Endpoints are added in Settings (URL + key sealed in the keyring). Registry CIDs
  come from ModelRegistry.
- AC2: Each escalation shows the destination and cost before it runs, and is charged to
  the HIC-2 budget. Over budget ⇒ ask.
- AC3: Registry escalation settles via InferenceRouter + x402. The receipt is stored in
  metering.

**US-1.6: The right model for my machine.** As a Member, onboarding picks a model tier
from my hardware.
- AC1: The sizeup probe runs locally at onboarding; the tier and its rationale are shown.
- AC2: The user can override. The choice persists.

---

## E2 Human in control

**US-2.1: Folder grants.** As a Member, I grant Hermes a folder and it can't reach
outside it.
- AC1: Grant descendants only. Symlinks are resolved before the check. Read and write
  are separate.
- AC2: `.ssh`, `.aws`, `.gnupg`, `.kube`, `.env*`, and OS keychains are denied even
  under full access.
- AC3: Full-disk access is an HIC-1 toggle that is **read-only** and expires in 24 h, with a countdown shown. Writes only through folder grants (D-15 amended).
- AC4: TLA+ `FolderGrant` is green; fuzz tests on path traversal pass.

**US-2.2: Shell with a leash.** AC1: Only fixed argv templates run without a prompt,
inside an OS sandbox (no network, scratch HOME) in a grant (D-16 amended). AC2: Anything else shows the exact command + cwd for approval. AC3: Every command
has a timeout and its output is captured in the activity log.

**US-2.3: Sign into dApps for me, safely.** As a Dev, Hermes can "Sign-In with Ethereum"
on sites I've allowlisted, without a click per login.
- AC1: Only EIP-4361 messages. Transactions, `eth_sign`, and EIP-712 permits always go
  to HIC-1.
- AC2: `domain` must equal the page origin. Per-origin count + expiry budget.
- AC3: Every signature is logged (origin, statement, nonce, time) and included in the
  nightly anchor.
- AC4: One-click revoke in Settings → Budgets. TLA+ `WebSigningBudget` is green.

**US-2.4: Approval cards I can understand.** AC1: Cards derive from tool annotations and
show decoded calldata, file diffs, or the command. AC2: Approve/reject is bound to a
specific ceremony id.

**US-2.5: Capsules with real sandboxes.** AC1: Capsule manifests may declare
filesystem/network, enforced via WASI preopens scoped to live grants and a socket
allowlist. AC2: Every shipped capsule is content-hashed and signed; load verifies both.

---

## E3 Knowledge and skills

**US-3.1: It knows Citrate out of the box.** As a Member, offline on first launch,
Hermes answers chain, SDK, precompile, and paraconsensus questions with citations to
bundled docs.
- AC1: The bundled graph includes the public docs, gradient papers, agentile, Trail of
  Bits, frontend-skills, and OZ/Solady/Foundry/Medusa/Slither refs.
- AC2: The answer-eval harness (`src/agent/eval.ts`, extended) scores ≥ the target on a
  150-question Citrate QA set, with citations resolving to bundled nodes.

**US-3.2: Skills load when needed.** AC1: Skill descriptions are indexed. ≤5 skills are
surfaced per turn. Bodies load on demand. AC2: Skills from all sources share one format
and one loader.

**US-3.3: Personas and tracks.** As a Member, I choose a persona (voice) and a track
(goal).
- AC1: ≥5 personas, each with voice, tone, skill allowlist, and default workflow. Names
  are chosen by the owner from drafted candidates.
- AC2: Tracks: Creative, Code, Smart contract + business logic, Project management,
  Full project. Each maps to a workflow family and an interview.
- AC3: Users can create custom personas (the P5 user-skills mechanism, extended).

**US-3.4: Learns only what's proven.** AC1: A skill/memory proposal shows the verifier
evidence. AC2: Persisting requires user accept. AC3: Publishing to SkillRegistry is
HIC-1. AC4: Contradicting memories (`Both`) are surfaced, not silently merged.

---

## E4 MCP fabric

**US-4.1: Hermes speaks MCP.** AC1: Hermes connects to mem-mcp, citrate-node MCP,
citratescan MCP, and user-added servers (allowlisted, with a review step). AC2: Tool
annotations drive HIC. AC3: Long operations use Tasks.

**US-4.2: My node is an MCP server.** As a Builder, I can point Claude Code or another
agent at my node's MCP server and it gets the same tools under the same HIC rules.
- AC1: Connect token; loopback by default. Writes always route to the ceremony in the
  app.
- AC2: Resources for status, balances, addresses, and the precompile table. Read and
  write tools as in 02 §5.

---

## E5 Eyes on the web

**US-5.1: Watch it browse.** AC1: The managed Chromium is visible in the Browser pop-out
(screencast), with element refs highlighted as Hermes acts. AC2: "Attach to my Chrome"
works via CDP with the user's explicit consent per session.

**US-5.2: Search privately.** AC1: `web_search` hits the bundled SearXNG. `read_url`
returns clean markdown. AC2: No third-party search without opt-in.

**US-5.3: Figure out a website.** As a Member, I ask Hermes to do a task on a site it
has never seen, and it completes it by picking elements step by step.
- AC1: The `decide()` slot picks actions over snapshot refs, with the local backend by
  default. AC2: The Jev backend is opt-in with an egress notice. AC3: The
  WebVoyager-style subset task success rate is recorded per backend in metering.

---

## E6 dApp forge

**US-6.1: hello mint in 1–2 shots.** (The program's headline story.)

```gherkin
Feature: hello mint
  Scenario: Dev builds and ships an NFT mint page from a prompt
    Given a fresh install on a T1 machine with the bundled toolchain
    And the user holds a Citrate membership
    When the Dev says "help me make an NFT project called Lemon Drops, 500 supply, 5 SALT each"
    # the dry-run test mint on the fork is priced at 0 (RT-18)
    Then Hermes asks at most 5 interview questions with defaults
    And writes a brief naming the hello-mint workflow and the D-4 gates
    When the Dev accepts the brief
    Then a vite+wagmi app and an ERC-721 contract are generated from a template
    And the Browser pop-out shows the site running against an anvil fork of 40204
    And forge test, slither, aderyn and medusa run with results in the activity monitor
    And the verdict card reads READY only if tests pass and no High findings exist
    When the verdict is READY and the Dev clicks Deploy
    Then a faucet top-up is requested if the balance is below the deploy estimate
    And a SignatureCeremony shows the decoded deploy whose bytecode hash matches the gated artifact
    And after approval the contract is deployed to 40204 and verified
    And the site switches to 40204 and a test mint succeeds through the page
    And the site is pinned to IPFS and a CID + gateway link are shown
```

- AC1: From prompt to READY takes ≤2 user prompts beyond interview answers.
- AC2: The same run with an injected bug (e.g. an unbounded mint) yields NOT READY with
  the finding and a proposed fix.
- AC3: Vercel export produces a deployable project directory.

**US-6.2: Refuses unready code.**

```gherkin
  Scenario: Gate blocks deploy
    Given the contract has a High slither finding
    When the Dev asks Hermes to deploy anyway
    Then Hermes refuses, cites the finding, and offers a fix
    And no ceremony is created
```

**US-6.3: Read any contract.** AC1: The contract reader pop-out opens any 40204 address
and shows verified source/ABI (new explorer tool `getVerifiedSource`). AC2: Hermes
explains functions and risks and can run view calls; writes go through the ceremony.

**US-6.4: Everyday tokens.** AC1: ERC-20 / ERC-721 / ERC-1155 / governor via OZ Wizard
parameters, the same gate, the same deploy path.

**US-6.5: Faucet in the node.** AC1: `faucet_request` calls the faucet (anti-abuse challenge handled
in-app, origin allowlisted). AC2: It respects the 24 h per-address limit and shows the
next eligible time. AC3: It sits under an HIC-2 budget.

---

## E7 Chain-native agent

**US-7.1: Hermes has an identity.** AC1: An AgentSBT is minted at onboarding (HIC-1),
bound to the member. AC2: Visible in Wallet + explorer.

**US-7.2: Every decision leaves a trail.** AC1: Local decision records for every
HIC-1/2 event. AC2: The nightly Merkle root is anchored (HIC-2 budget). AC3: The app can
prove inclusion of any past decision.

**US-7.3: Hermes measures itself.** AC1: A metering record per task (02 §8). AC2: The
daily report in the journal. AC3: Opt-in aggregates go to BenchmarkRegistry.

**US-7.4: Watch Hermes work.** AC1: The activity monitor pop-out shows the live plan,
steps, tool calls, approvals, verifier results, tokens/s, ctx usage, spend, and the
"why am I waiting" state (prompt processing / generating / tool running + timer). AC2:
A Stop button is always visible.

**US-7.5: Precompile literacy.** AC1: Hermes can explain and call every live precompile
through `precompile_call` helpers with correct encodings. AC2: Generated contracts can
use them (tested on the devnet). AC3: After the federation precompile work, the
model/LoRA calls succeed end to end.

---

## E8 Fleet

**US-8.1: Fleet wizard.** As an Operator, at first launch Hermes helps me connect all my
machines.

```gherkin
Feature: fleet wizard
  Scenario: Operator connects a laptop and a Linux box on the same network
    Given Citrate Core is installed on the laptop
    When the Operator chooses "Connect my machines"
    Then the sizeup probe runs and shows this machine's role recommendation
    And with consent, mDNS lists other machines running Citrate Core
    And a link/QR is offered for machines without it
    When the Linux box installs from the link and scans the pairing QR
    Then a wallet-signed DeviceLink is issued for the Linux box
    And both devices appear under the member in the cluster roster with distinct peer ids
    And Hermes guides Tailscale setup if the machines are not directly reachable
    And a group can be created and invites sent to other people
```

- AC1: Distinct peer ids per device (TLA+ `DeviceLink`). AC2: Revoking a device evicts
  it within one admission cycle.

**US-8.2: Mesh on by default.** AC1: The CL-S4 sign-off is recorded. AC2: A two-machine
soak and a ≥50-node ladder step pass.

**US-8.3: Cluster tools for Hermes.** AC1: `cluster_status/peers/join/share`, device
list, and invite create, as MCP tools with annotations.

---

## E9 Learn together

**US-9.1: A live federated round.** As an Operator with ≥3 devices, I can run a
paraconsensus round that trains a Hermes LoRA.
- AC1: Workers train on verified trajectories only (D-22 records, redacted).
- AC2: Aggregation via 0x0110 Belnap (Q16). An independent replay matches the digest.
- AC3: The round is committed and settled on-chain; the challenge window elapses.
- AC4: The adapter is recorded in LoRAFactory and registered in ModelRegistry.
- AC5: The resulting LoRA is loadable by llama-server and improves the eval score on the
  HUP suite, or is rejected by the eval gate.

**US-9.2: Hermes understands paraconsensus.** AC1: It explains FOUR, the knowledge/truth
orders, the classifier, and the aggregation, with citations. AC2: It can prepare inputs
for 0x0110 and interpret `states[]`.

---

## E10 Everyday work

**US-10.1: Media.** AC1: Image/video generation is tiered local vs registry/endpoint.
AC2: Outputs open in the Media pop-out. AC3: Cost is shown.

**US-10.2: Spreadsheets and calendar.** AC1: Read/write xlsx/csv in grants. AC2: Google
Sheets/Calendar via Connections when linked. AC3: Hermes's own schedule is visible as a
calendar.

**US-10.3: Widgets and daemons.** AC1: Hermes can author a widget and it renders in a
sandbox with read-only data. AC2: A daemon runs on schedule within budget and reports
to the monitor.

**US-10.4: My journal.** As a Member, each day Hermes writes an analysis + retro of my
work, and I can export it privately.
- AC1: A daily entry from the event log + metering + memory. AC2: Export is an encrypted
  bundle (passphrase or wallet-derived key). AC3: Nothing leaves the machine unless I
  export it.

---

## E11 Prove it

**US-11.1:** The hello-mint Gherkin passes on a clean install per OS (macOS arm64,
Linux x64, Windows x64).
**US-11.2:** The eval suite runs in CI with a per-tier scorecard.
**US-11.3:** A red-team pass (prompt injection via web pages, malicious skills,
path-traversal, SIWE phishing, budget exhaustion) finds no open High.
**US-11.4:** Document pass: docs/Almanac pages for Hermes, the node MCP, skills,
personas, and the fleet wizard.
