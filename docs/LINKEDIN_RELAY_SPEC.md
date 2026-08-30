---
created: 2026-08-30
branch: feat/social-d4-invites
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (Stage-1) — owner deploys the relay; client wiring follows
repo: citrate-core
companions:
  - docs/adr/ADR-2026-08-30-social-identity-privacy-model.md
  - docs/CONNECTIONS_RUNBOOK.md (§1 / §4)
  - src-tauri/src/social.rs (where the client `linkedin` config lands)
  - src-tauri/src/connections.rs (HOSTED_REDIRECT_URI + the loopback the desktop owns)
---

# LinkedIn social-identity relay — spec

LinkedIn is a **confidential OAuth client**: its token endpoint requires the `client_secret`, and it
has not adopted public-client PKCE the way Discord and X have. We must **never** ship that secret in
the desktop app. So — unlike Discord/X, which link entirely on-device — LinkedIn needs a small
**hosted relay you deploy** (on `auth.citrate.ai`) that holds the secret and does the code→token
exchange server-side.

Key simplification: for *identity* we only need the person's **handle** + **proof of ownership** —
not ongoing LinkedIn API access. So **the LinkedIn access token never has to reach the desktop.** The
relay verifies ownership, reads the handle, and hands the desktop only `{ handle }`. The wallet-signed
binding (the D3 "Verify" step) is unchanged and happens entirely on-device afterward. The relay is
therefore tiny and stateless-ish.

## 1. What you register (LinkedIn Developer Portal)

1. https://www.linkedin.com/developers/apps → **Create app** (associate a Company Page).
2. **Products** → add **"Sign In with LinkedIn using OpenID Connect"**.
3. **Auth** tab → **Authorized redirect URLs**: `https://auth.citrate.ai/oauth/linkedin/callback`
   (the RELAY's callback — not the desktop loopback).
4. Copy the **Client ID** and **Client Secret**. The secret goes **only** into the relay's env
   (`LINKEDIN_CLIENT_SECRET`), never into this repo or the app.
5. Scopes: `openid profile` (OIDC — returns `sub` + `name` + `preferred_username` where available).

## 2. What you deploy (the relay — 2 endpoints)

Host on `auth.citrate.ai`. The relay holds `LINKEDIN_CLIENT_ID` + `LINKEDIN_CLIENT_SECRET`.

### `GET /oauth/linkedin/start`
Kicks off the flow (the desktop opens this in the system browser).
- Query in: `state` (opaque, from the desktop), `challenge` (PKCE S256 — optional; LinkedIn ignores
  it, but pass it through so the desktop can still bind the round-trip), `rd` = the desktop loopback
  return (`http://127.0.0.1:8975/oauth/callback`).
- The relay stores `{state → rd}` briefly (≤5 min TTL) and 302s the browser to LinkedIn's authorize:
  `https://www.linkedin.com/oauth/v2/authorization?response_type=code&client_id=…&redirect_uri=https://auth.citrate.ai/oauth/linkedin/callback&scope=openid%20profile&state=<state>`

### `GET /oauth/linkedin/callback`
LinkedIn redirects here with `code` + `state`.
- The relay: exchanges the code **server-side** (with the secret) at
  `https://www.linkedin.com/oauth/v2/accessToken` (`grant_type=authorization_code`, `code`,
  `redirect_uri`, `client_id`, **`client_secret`**), then calls
  `GET https://api.linkedin.com/v2/userinfo` with the bearer to read `sub` + a handle
  (`preferred_username` or `name`).
- The relay then **302s the browser back to the desktop loopback** it stored for `state`:
  `http://127.0.0.1:8975/oauth/callback?state=<state>&handle=<urlenc handle>&sub=<sub>`
  — i.e. it returns only the verified `handle` (+ `sub`), **never the LinkedIn token**.
- The relay discards the LinkedIn token; it keeps nothing sensitive.

> Security: the relay validates `state`, enforces the TTL, allows only the fixed loopback `rd`, and
> never logs the token. HTTPS only. Rate-limit `/start`.

## 3. What I wire on the desktop (once the relay is live)

Small, additive — mirrors the Discord/X path but with the relay as the "provider":
- Add a `linkedin` config in `social.rs` whose `authorize` is `https://auth.citrate.ai/oauth/linkedin/start`
  and whose "token/userinfo" steps are **skipped** — the relay already returned `handle` on the
  loopback callback. So `social_start("linkedin")` becomes: open `/start?state&rd`, capture the
  loopback callback, read `handle` from its query (not a `code`), and record the unverified link.
  No token, no keyring entry for LinkedIn.
- Everything downstream is unchanged: **Verify** (wallet-signed binding, D3) → **share** (D1) →
  **faces**. LinkedIn "just works" like the others after the relay is up.

The one client nuance: `capture_public_pkce` today expects a `code` on the callback; for LinkedIn the
callback carries `handle`. I'll add a `capture_loopback_query` variant that returns the raw callback
params, so the LinkedIn path reads `handle` directly. ~30 lines, no new deps.

## 4. Owner checklist

- [ ] Register the LinkedIn app (§1); add **Sign In with LinkedIn using OpenID Connect**.
- [ ] Set the redirect URL to `https://auth.citrate.ai/oauth/linkedin/callback`.
- [ ] Deploy the 2 relay endpoints (§2) with `LINKEDIN_CLIENT_ID` + `LINKEDIN_CLIENT_SECRET` in env.
- [ ] Confirm the relay 302s back to `http://127.0.0.1:8975/oauth/callback?state=…&handle=…&sub=…`.
- [ ] Tell me the relay is live → I wire the ~1-file desktop change and LinkedIn links + verifies +
      shares like Discord/X.

## Why this shape (vs. the alternatives)

- **Embed the secret in the app** — rejected: anyone can extract it from a desktop binary.
- **Relay returns the LinkedIn token to the desktop** — unnecessary: identity needs only the handle +
  ownership proof, so the relay returning `{handle}` keeps the token off every device and out of the
  keyring. Smaller blast radius.
- **Full server-side session** — overkill: we don't need LinkedIn API access, just the one-time
  ownership read at link time.
