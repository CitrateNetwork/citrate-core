---
created: 2026-08-29T00:00:00Z
branch: feat/cx-agent-resolve-loop
author: Larry Klosowski + Claude Opus 4.8
status: draft (Stage-1) — coverage snapshot after the CX redesign passes
repo: citrate-core
companions:
  - docs/CX_SURFACES_USER_STORIES.md
  - docs/CONNECTIONS_RUNBOOK.md
  - src/bridge/domains.ts
---

# CX coverage map — what's wired, what's mocked, what needs setup

A single clear view of every surface and its network wiring after the redesign passes, so you
know exactly what connects to the network today, what's honestly flagged as pending, and what
you need to set up (via the runbook) to light the rest.

**Legend:** ✅ **WIRED** — calls a real backend today · 🟡 **PARTIAL** — some paths real, some pending
· ⛔ **NOT WIRED** — honest mock, flagged in the UI, needs backend · 🔑 **needs owner setup** (runbook).

---

## ⚠️ Read this first — one dependency gates three surfaces

The **Groups, Cluster, and Agent** surfaces drive real sidecar daemons (`comms-member-daemon`,
`cluster-daemon`, `hermes`). In these CX branches those binaries are still resolved via
`.resource_dir()` — the **buggy path** that produces "binary not bundled" in a packaged build.
**The fix is PR #178** (`resolve_external_bin`, plus the idempotent node-start + onboarding fixes).

> **Merge #178 before (or with) the CX stack**, or Groups/Cluster/Agent will render correctly but
> their daemons won't launch in the packaged app. The UI + bridge wiring in #179–#181 is complete
> and correct; it just needs the daemons to actually spawn.

## Merge order (open PRs)

| PR | Base ← Head | Contents |
|---|---|---|
| **#178** | `main` ← `fix/node-start-idempotent-onboarding` | Daemon binary bundling fix + idempotent node-start + onboarding polish. **Independent — merge first.** |
| **#179** | `main` ← `…pass1…` | Pass 1: Agent suite, Groups/Cluster, Train, Connections, tauri agent bridge |
| **#180** | `#179` ← `…pass2b1…` | Pass 2.1: Files kubo card, Models pending, Community |
| **#181** | `#180` ← `…agent-resolve-loop` | Agent approve→resolve ceremony last-mile |

#179 → #180 → #181 are stacked; merge in order. #178 is independent but is a **hard prerequisite**
for the daemon surfaces to function.

---

## Per-surface coverage

| Surface | Status | Connects to | Notes / gaps |
|---|---|---|---|
| **Dashboard** | 🟡 PARTIAL | live chain height (wagmi → rpc.citrate.ai), node/wallet state | vitals are real; **the chat pane is the local demo agent** ("preview") — real inference isn't routed yet (see Chat below) |
| **Wallet** | ✅ WIRED | `wallet_*` + ceremony → real 40204 txs | send / stake / withdraw / claim / wallet-link all sign+broadcast through the Signature Ceremony |
| **Node** | ✅ WIRED | node-agent supervisor (127.0.0.1:19600) | start/pause/resume/stop, log tail, peers, validator, resources — all real |
| **Storage** | ✅ WIRED | kubo (IPFS) | pin bond is the local marker until the on-chain CommD bond (S2.2) lands |
| **Files** | ✅ WIRED | kubo (IPFS) | + honest kubo-outage card (Pass 2.1). Bonded pin is ceremony-gated |
| **Models** | 🟡 PARTIAL | `modelsCatalog.search/download/select` (HF/GitHub) | `local()` **NOT WIRED** → honest "pending wire · local()" state (Pass 2.2) |
| **Agent** | ✅ WIRED* | `hermes_*` sidecar + ceremony (bridge_pending/resolve) | start/status/skills/run/approvals + real approve→resolve loop (#181). **\*needs #178** for the hermes binary to bundle |
| **Connections** | 🟡 PARTIAL 🔑 | MCP: `bridge.connections` (GitHub/Drive/Notion OAuth ✅) | Social identity / SaaS / Webhooks **NOT WIRED** — need backend + privacy ADR (runbook §1/§4/§5) |
| **Journal** | ✅ WIRED | local encrypted notes (data dir) | agents write only with approval; pinned pages snapshot to the PIN daemon |
| **Groups** | ✅ WIRED* | comms-member-daemon (UDS) | create/join/roster/roles/offboard/messages. **\*needs #178** for the daemon to bundle |
| **Comms** | ✅ WIRED* | comms-member-daemon | same daemon dependency (#178) |
| **Cluster** | ✅ WIRED* | cluster-daemon (UDS + libp2p) | status/join/leave/peers/shareFile. **\*needs #178** |
| **Train** | ⛔ NOT WIRED | `training_*` (settlement) | full round UI, flagged "pending backend · SETL-S3" (citrate-settlement daemon) |
| **Community** | ⛔ NOT WIRED | (indexer) | flagged "pending backend · illustrative"; real referral link; needs an indexer + rewards planset |
| **Commissary** | 🟡 PARTIAL | app catalog | catalog surface; publishing/routing per the Commissary program |
| **Settings** | ✅ WIRED | config store, OS keyring, AI provider keys, connections | account/app/keys/node/AI providers/API keys/billing/connections |
| **ALF** | 🟡 member-gated | ALF cooperative reads via the node | appended to nav only for ALF members |

---

## Per-domain network wiring (`src/bridge`)

| Domain | Status | Backend |
|---|---|---|
| `wallet` / `signing` | ✅ | custody vault + ceremony → 40204 RPC |
| `node` (supervisor) | ✅ | node-agent bearer over loopback |
| `storage` | ✅ | kubo IPFS seam |
| `modelsCatalog` | 🟡 | search/download/select real; `local()` pending |
| `groups` | ✅* | comms-member-daemon (needs #178 bundling) |
| `cluster` | ✅* | cluster-daemon (needs #178 bundling) |
| `training` | ⛔ | citrate-settlement (SETL-S3) |
| `agentHarness` | ✅* | hermes sidecar + ceremony bridge (needs #178 bundling) |
| `connections` (MCP) | ✅ | OAuth loopback-PKCE → OS keyring (GitHub/Drive/Notion) |
| `memories` (MCP) | ✅ | mem-mcp daemon (proven headless — gA-memory) |
| **chat / inference** | 🟡 | **local demo agent today** — neither the inference-gateway nor a selected local model is routed to chat yet |

### Chat / inference — the one cross-cutting gap
The Dashboard + Agent chat runs on the built-in **local demo agent** ("preview"). Selecting a model
in **Models** switches the local `llama-server`, but chat isn't yet routed to it (or to the
inference-gateway). Wiring real inference (Models' active model → chat) is the highest-value
non-runbook follow-up after the daemon-bundling merge.

---

## What needs YOUR setup (runbook — `docs/CONNECTIONS_RUNBOOK.md`)

These are the 🔑 items — none block the app; they light up the Connections surface and the
social/gamification loop. Suggested order:

1. **Privacy ADR for social identity** — blocks all social work (runbook §1).
2. **Register OAuth clients** (PKCE public): X, Discord, GitHub, Google (Desktop), Notion, Linear.
   *(GitHub/Drive/Notion are already wired via `bridge.connections` — confirm the client ids.)*
3. **Token-exchange relay** you host for confidential clients: LinkedIn, Slack (runbook §1/§4).
4. **Google verified-app review** for sensitive Drive/Gmail/Calendar scopes.
5. **Webhook** secret handling + event taps (runbook §5).

Work these one at a time — ping me per service and I'll guide the wiring (`social_*` commands, the
signed IdentityBinding through the ceremony, the resolver for roster faces, etc.).

---

## Known follow-ups (engineering, not owner setup)

| Item | Where | Priority |
|---|---|---|
| Merge **#178** (daemon bundling) | `fix/node-start-idempotent-onboarding` | **P0 — gates Groups/Cluster/Agent** |
| Route real inference (Models → chat, or gateway) | chat/inference seam | P1 |
| `modelsCatalog.local()` resolver | CX-S1 | P2 |
| Train settlement daemon | citrate-settlement (SETL-S3) | P2 (backend) |
| Community indexer + rewards planset | new | P3 (backend) |
| Social / SaaS / Webhook backends | runbook | tracked (owner-gated) |

---

## Bottom line

- **Everything the redesign added is built and honest.** The new surfaces (Agent, Connections,
  Community) and the gap-closers (Groups/Cluster/Train redesigns, Files/Models fixes, the agent
  ceremony loop) are done, tested (372 green), and flagged truthfully where a backend is pending.
- **One P0 dependency:** merge **#178** so the Groups/Cluster/Agent daemons actually bundle.
- **One cross-cutting gap:** route real inference to chat.
- **Everything else pending is either a named backend (Train/Community) or a runbook connection you
  drive.** Nothing is faked; nothing silently broken.
