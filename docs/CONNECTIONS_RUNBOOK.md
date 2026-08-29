---
created: 2026-08-29T00:00:00Z
branch: feat/cx-redesign-pass1-agent-groups-connections
author: Larry Klosowski + Claude Opus 4.8
status: draft (Stage-1) — buildable checklist for the Connections surface
repo: citrate-core
companions:
  - src/surfaces/Connections.tsx (the surface this runbook backs)
  - src/bridge/domains.ts (ConnectionsDomain — the wired MCP seam)
  - docs/CX_SURFACES_USER_STORIES.md
---

# Connections runbook — making the native app connect to everything

This is the engineering checklist behind the **Connections** surface. For every integration
the app offers, it names: what it is, how it authenticates, where the secret lives, what the
node/agent must build, and what **you** (the owner) must register externally. Rows marked
**WIRED** work today; **PARTIAL** / **NOT WIRED** rows list exactly what's left.

The strategic reason this matters: once social identity is verifiable and connected, the
referral → signed-roster-assertion → milestone-reward loop (see §7) turns growth into a game
the network can actually settle — that's the engine, and it's only as good as these wires.

## The one pattern everything reuses

Every third-party connection follows the same shape the wired MCP seam already uses. Build it
once, reuse it per service:

1. **OAuth 2.0 + PKCE on a loopback redirect.** The app spins a localhost listener
   (`http://127.0.0.1:<port>/callback`), opens the system browser to the provider's
   authorize URL with `code_challenge`, and catches the `code` on the loopback. No client
   secret ships in the app (PKCE public-client flow). API-key providers skip this and paste a
   key instead.
2. **Token sealed in the OS keyring** (macOS Keychain via the same `keyring` crate custody
   uses) — service id `ai.citrate.core.connections.<service>`. The token **never** enters the
   webview or app state; only connect facts (`connected`, `scope`, `connectedAt`) cross the
   bridge. This is the `ConnectionInfo` contract already in `domains.ts`.
3. **Refresh + revoke** handled node-side; `disconnect(service)` forgets the sealed token.
4. **Every effect still stops at the ceremony.** A connected tool lets the agent *propose*;
   a write (post, create, send) surfaces as an approval. Connections grant reach, never
   autonomy.

> Cross-cutting owner task, once: register a **PKCE public OAuth client** per provider below
> and record the client id in `CONFIG`. No client secrets on the desktop. For providers that
> require a confidential client (some SaaS), the token exchange must run through a thin
> first-party relay you host — noted per row.

---

## 1. Social identity — X, LinkedIn, Discord   ·  **NOT WIRED**

The net-new capability. Goal: a member links a social account, the app **proves they own it**,
and binds that identity to their wallet address under a privacy setting — so rosters show
recognizable people, and invites become "add @handle" instead of "paste 0x…".

**Blocker before any build: a privacy ADR.** The address↔identity binding is sensitive. Decide
and record: where the binding is stored (local-only vs. relay-published-encrypted), who can
resolve it (groups-only default vs. private), and whether it is provable to others without
doxxing. Do not ship self-asserted handles as "verified".

| Service | Auth | Proof of ownership | Owner must register |
|---|---|---|---|
| **X** | OAuth 2.0 PKCE (`users.read`, `tweet.read`) | Post/verify a one-time nonce, or read the OAuth'd `username` and sign a challenge binding it to the wallet | X developer app → OAuth2 client id; callback `http://127.0.0.1:*/callback` |
| **LinkedIn** | OAuth 2.0 (**confidential client** — needs a relay for token exchange) | `r_liteprofile` → verified name/id + wallet-challenge signature | LinkedIn app + a hosted token-exchange relay (client secret stays server-side) |
| **Discord** | OAuth 2.0 PKCE (`identify`) | OAuth'd user id + wallet-challenge signature | Discord app → OAuth2 client id; loopback redirect |

**Node/app to build**
- `social_start(service)` / `social_verify(service, proof)` / `social_disconnect(service)` tauri
  commands, plus a `social` bridge domain mirroring `ConnectionsDomain` (+ a `verified` +
  `visibility` field).
- A `RoleAssertion`-style **signed IdentityBinding** (wallet signs `{service, handle, nonce}`) so
  the binding is provable and revocable — routes through the **Signature Ceremony**.
- Resolver: given a roster address, return the linked handle **iff** the viewer's visibility
  setting permits (groups-only / private). Rosters, peer lists, and message senders call it.

**Surface already renders**: the opt-in → verifying → visibility flow, flagged "pending backend".

---

## 2. MCP servers — GitHub, Google Drive, Notion   ·  **WIRED**

Already live via `bridge.connections` (loopback PKCE, tokens in the OS keyring) — the same seam
Settings uses. Connected servers mount as tools the agent proposes with.

| Service | Status | Owner must register |
|---|---|---|
| **GitHub** | WIRED | GitHub OAuth App (PKCE) → client id in CONFIG; scopes `repo`, `read:org` |
| **Google Drive** | WIRED | Google Cloud OAuth client (Desktop) → client id; scope `drive.readonly` (+ `drive.file` for writes); **verified-app review** for sensitive scopes |
| **Notion** | WIRED | Notion public integration → OAuth client id; select workspace pages |
| **Custom (stdio / https)** | PARTIAL | none — user-supplied; needs `config.write mcpServers` persistence + a spawn/allowlist path |

**Left to build**: custom-server persistence (write to the MCP config + spawn stdio / mount
https), and surfacing per-server tool lists. The three named providers work today.

---

## 3. Foundational models — local + cloud providers   ·  **PARTIAL (routes to real surfaces)**

No new wiring — Connections links out to where these already live.

| Provider | Status | Where |
|---|---|---|
| **Local model** | WIRED | Models surface (`modelsCatalog.*`) — download/verify/select |
| **OpenAI / Anthropic / gateway** | WIRED | Settings → AI provider keys, sealed in the keyring; used by chat + agent |

**Left to build (optional)**: a default-provider switch surfaced directly in Connections
(today it's set in Settings).

---

## 4. SaaS tools — Slack, Linear, Google Calendar, Gmail   ·  **NOT WIRED**

Same OAuth + keyring pattern as §2. Each becomes an agent tool whose writes stop at the ceremony.

| Service | Auth | Owner must register | Notes |
|---|---|---|---|
| **Slack** | OAuth 2.0 (bot + user scopes) | Slack app → client id/secret (**confidential → relay**); scopes `chat:write`, `channels:read` | posting is a ceremony-gated effect |
| **Linear** | OAuth 2.0 PKCE | Linear OAuth app → client id | `issues:create`, `read` |
| **Google Calendar** | OAuth 2.0 (Google client) | reuse the Google client from §2; scope `calendar.events` | verified-app review for write scopes |
| **Gmail** | OAuth 2.0 (Google client) | scope `gmail.readonly` + `gmail.compose` | **never auto-send** — draft only, send stops at the ceremony |

**Node/app to build**: extend the `connections`/`social` pattern with these service ids, a token
relay for confidential clients (Slack), and per-tool proposal → approval mapping.

---

## 5. Webhooks — outbound event posts   ·  **NOT WIRED**

Let the app POST signed notices (ceremony outcomes, node state changes, cluster health) to the
owner's own systems.

**To build**
- `webhook_add(url)` / `webhook_remove(url)` / `webhook_list()` — persisted in config; **https-only**
  (the surface already rejects http).
- **HMAC-SHA256 signature** over the body with a per-endpoint secret (generated on add, shown once,
  sealed in the keyring); send as `X-Citrate-Signature`. Include a timestamp + replay window.
- An event bus tap: ceremony `witnessed`/`declined`, node `state` transitions, cluster
  `health`/`peer` changes → serialized event → POST with retry/backoff.

**Surface already renders**: add/remove endpoints as drafts, flagged "not-yet-delivering".

---

## 6. Owner action checklist (do these to light it all up)

- [ ] **Write the privacy ADR** for social identity binding (§1) — blocks all social work.
- [ ] Register PKCE public OAuth clients: **X**, **Discord**, **GitHub**, **Google** (Desktop),
      **Notion**, **Linear**. Record client ids in `CONFIG`.
- [ ] Stand up a **thin token-exchange relay** you host for confidential-client providers
      (**LinkedIn**, **Slack**) — client secrets live there, never on the desktop.
- [ ] Submit **Google verified-app review** for any sensitive Drive/Gmail/Calendar scopes.
- [ ] Confirm keyring service-id namespace `ai.citrate.core.connections.<service>` and add
      `@rule8` sign-off (these are credential surfaces — treasury/keys review applies).
- [ ] Decide webhook secret handling + the event taps to expose (§5).

## 7. Why this is the growth engine (the gamification loop)

Once §1 is real, the loop closes:

1. A member links + verifies a social identity (opt-in, provable).
2. Their **invite link** (`citrate.ai/join?ref=…&g=…`) is shareable anywhere their socials reach.
3. Joins through it land in the group roster as **signed assertions** — no self-reported numbers.
4. The Community surface counts those assertions into the **milestone program** (100 / 500 /
   2,000 members → SALT rewards), which **settles each epoch through the ceremony**.

Every step is already honest-by-construction: verified identity, signed joins, ceremony-settled
rewards. The runbook above is what makes step 1 real — and step 1 is the whole game.
