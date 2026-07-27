---
created: 2026-07-26
branch: feat/w4-mcp-oauth
author: Claude (Opus 4.8), directed by @SaulBuilds
status: accepted (beta) — public-release migration noted
---

# ADR-3 — OAuth redirect strategy + token custody (W4 MCP connections)

## Context
W4 connects Google Drive, Notion, GitHub as agent-callable MCP tools. Each needs an
OAuth 2.0 authorization-code flow. Citrate Core is a **desktop app** (Tauri), so the
classic web redirect + confidential-client model does not cleanly apply, and the
providers differ (Google supports dynamic loopback; GitHub + Notion want a fixed,
exact callback; Notion may require https).

## Decision

### Redirect: fixed loopback + PKCE (RFC 8252)
- One callback registered for all three: `http://127.0.0.1:8975/oauth/callback`.
  Fixed port (not the OIDC path's random `:0`) because GitHub + Notion match the
  redirect_uri exactly. A brief single-use listener binds only during the flow;
  if the port is busy the flow fails closed (no silent fallback to a wrong port).
- **PKCE (S256)** on every flow; `state` CSRF check; the listener reads one bounded
  callback line then closes — mirrors `oidc.rs`.
- Fallback (Notion only, if it rejects `http://` loopback): a hosted redirect on
  `auth.citrate.ai/oauth/notion/callback` that 302s back to the loopback. Not built
  unless Notion forces it.

### Token custody: OS keyring, per service, sealed in Rust
- Per service, the app seals `{client_id, client_secret, access_token,
  refresh_token, expiry, scopes}` in the OS keyring (the A2 custody pattern). No
  `#[tauri::command]` returns a token or the client secret (I-2 barrier), same as
  the AI provider key. The webview picks WHICH service; it never supplies the token.
- Refresh on expiry; explicit revoke on disconnect (best-effort provider revoke +
  local wipe).

### Capability scope + writes
- Request the **narrowest** scope per service (Drive readonly; Notion read; GitHub
  repo read + PR draft). Every agent-proposed WRITE routes through the
  SignatureCeremony / an approval gate (Rule 3) — a connected token never lets the
  agent write unattended.

## Beta vs public (the honest tradeoff)
For the 3-person beta the client secret is sealed in each member's keyring. A single
OAuth app's secret shared across users is fine for trusted beta but is **not** a
public-release posture. **Before public release**, move token exchange behind a
backend proxy on `auth.citrate.ai` (the proxy holds the secret; the desktop app does
PKCE and exchanges the code via the proxy). The registered redirect URI above does
not change when we switch. Tracked as the W4 hardening item.

## Consequences
- Reuses the proven `oidc.rs` loopback machinery (low new attack surface).
- One redirect URI to register everywhere (the runbook).
- Desktop secret-at-rest is keyring-protected but shared-per-app in beta — the proxy
  migration is required before public and is called out, not silently deferred.
