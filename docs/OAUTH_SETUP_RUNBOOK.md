---
created: 2026-07-26
branch: feat/w4-mcp-oauth
author: Claude (Opus 4.8), directed by @SaulBuilds
status: active — owner action (register OAuth apps)
---

# OAuth app registration runbook (W4 — MCP connections)

Register one OAuth app per service so Citrate Core can connect Google Drive,
Notion, and GitHub as agent-callable MCP tools. Do them **in this order** (fastest
+ most permissive first). For each, you end up with a **Client ID** + **Client
secret** — keep them; you'll paste them into **Settings → Connections** in the app
(I'm building that screen), which seals them in the OS keyring. **Do not paste
secrets into chat.**

## The one value every app needs
**Authorization callback / redirect URI — register this EXACT string:**

```
http://127.0.0.1:8975/oauth/callback
```

The app runs an RFC-8252 loopback listener on `127.0.0.1:8975` for the duration of
the sign-in, with PKCE + a `state` CSRF check. (Google's "Desktop app" client type
allows any loopback port automatically, but registering the exact URL is harmless
and keeps all three identical.)

---

## 1. GitHub  (easiest — do first)
1. Go to **https://github.com/settings/developers** → **OAuth Apps** → **New OAuth App**.
   (Or, to own it under the org: **https://github.com/organizations/CitrateNetwork/settings/applications** → New OAuth App.)
2. Fill in:
   - **Application name:** `Citrate Core`
   - **Homepage URL:** `https://citrate.ai`
   - **Authorization callback URL:** `http://127.0.0.1:8975/oauth/callback`
   - Leave "Enable Device Flow" unchecked.
3. **Register application** → copy the **Client ID**.
4. Click **Generate a new client secret** → copy the **Client secret** (shown once).
5. Scopes are requested by the app at sign-in time (read repos + open PR drafts) —
   nothing to configure here.

**Hand off:** GitHub Client ID + Client secret.

---

## 2. Google Drive  (a few more steps)
1. **https://console.cloud.google.com/** → create a project (e.g. `citrate-core`) or pick one.
2. Enable the Drive API: **https://console.cloud.google.com/apis/library/drive.googleapis.com** → **Enable**.
3. OAuth consent screen: **https://console.cloud.google.com/apis/credentials/consent**
   - User type: **External** → Create.
   - App name `Citrate Core`, your support email, developer email.
   - **Scopes** → Add: `.../auth/drive.readonly` (read files/metadata).
   - **Test users** → add your own Google address (required while the app is
     unverified — up to 100 testers, no Google review needed for the beta).
4. Credentials: **https://console.cloud.google.com/apis/credentials** → **Create
   Credentials** → **OAuth client ID** → **Application type: Desktop app** →
   name it → **Create**.
   - (Desktop-app clients allow loopback redirects automatically. If it offers a
     redirect field, add `http://127.0.0.1:8975/oauth/callback`.)
5. Copy the **Client ID** + **Client secret** from the dialog.

**Hand off:** Google Client ID + Client secret.

---

## 3. Notion  (fussiest — do last)
1. **https://www.notion.so/my-integrations** → **New integration**.
2. Create it, then open **Distribution / OAuth Domain & URIs** and switch it to a
   **Public integration** (required for the OAuth code flow; internal integrations
   only issue a static token).
3. **Capabilities:** Read content (and Read comments if you want it later).
4. **Redirect URIs:** add `http://127.0.0.1:8975/oauth/callback`.
   - ⚠️ If Notion rejects the `http://` loopback and demands `https://`, stop and
     tell me — that's the one provider that may force the fallback (a small hosted
     redirect on `auth.citrate.ai`), and I'll wire it. Try the loopback first.
5. Copy the **OAuth client ID** + **OAuth client secret** from the integration's
   secrets section.

**Hand off:** Notion Client ID + Client secret.

---

## Later (not now): Gmail, Google Calendar, Slack
Room is left for these. Gmail/Calendar reuse the Google project above (just add the
scopes + enable those APIs). Slack is its own app at **https://api.slack.com/apps**.

## How you'll give the app the keys
Enter each Client ID + secret in **Settings → Connections** (the screen I'm
building) — sealed in the OS keyring, bound per service, never in the repo or the
build. For dev testing before that UI lands, drop them in a gitignored
`src-tauri/oauth.dev.json` and I'll read from it.

## Honest security note (beta vs public)
For the 3-person beta the client secret is sealed in each member's keyring. A
single OAuth app's secret shared across users is acceptable for trusted beta but is
**not** how we ship publicly — before public release we move token exchange behind
a backend proxy on `auth.citrate.ai` so the secret never leaves our server. Tracked
in ADR-3. The redirect URI above does **not** change when we make that switch.
