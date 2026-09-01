---
created: 2026-09-01
branch: docs/grow-coordination
author: Mac team (Claude Opus 4.8, directed by @SaulBuilds)
status: living — both teams append via PR
purpose: cross-team coordination + review channel for the GROW join/connect flow
teams:
  - Mac team — repo citrate-core — the desktop app (identity, relay client, invite surface, citrate:// deep-link, DMG/notarize, updater client)
  - DGX team — repo citrate-landing (+ relay/DO infra) — the web join page, deep-link button, short-code resolver, download redirect, AASA, relay hosting
companions:
  - docs/CLUSTER_GROWTH_PLANSET.md
  - docs/CLUSTER_REWARDS_SPEC.md
---

# GROW coordination — Mac team ⇄ DGX team

We work in **different repos** (citrate-core = Mac, citrate-landing = DGX) so we don't collide in one
tree. This doc is the shared channel: **append your status/asks here via PR, and cross-review each
other's PRs.** Keep the "Interface contracts" section exact — it's the only place the two sides must
agree byte-for-byte.

## Coordination protocol
1. **One repo per team** (above). Never edit the other team's repo without a heads-up here.
2. **Cross-review**: Mac team reviews DGX PRs that change a shared contract (link format, resolver API,
   download URL, updater feed); DGX reviews Mac PRs that do the same. Leave the review on the PR; note
   the outcome in the Status board below.
3. **This doc is append-only-ish**: add to your team's section + the Decisions log; don't rewrite the
   other team's lines. Land changes via PR so the other side sees them on `git pull`.
4. Mac team runs a **drift-watch** on citrate-core `origin` (fetch loop) so a DGX push there is caught.

## Interface contracts (MUST match on both sides)

**Join link (self-contained, live today):**
```
web:       https://citrate.ai/join/<clusterId>?by=<handleOrShort>&c=<clusterName>&g=<goal>&ref=<inviterAddr>
deep-link: citrate://join/<clusterId>?by=<...>&c=<...>&g=<...>&ref=<...>   (mirrors the web path/query)
```
- `by` = display name only; `ref` = full inviter address (attribution). `c`/`g` = display. Never PII beyond a name.
- Mac side: minted by `src/surfaces/referral.ts` `buildJoinLink` (#210); parsed by `parseJoinLink` (tolerates both schemes).
- DGX side: rendered by the `/join/<cluster>` page (#43) + the "Open in Citrate" button (#44).

**Custom scheme (the reliable one-tap path):** `citrate://` — registered app-side (citrate-core #215),
bundle id `ai.citrate.core`. This is the primary hand-off for a .dmg install.

**Apple Team ID (for AASA):** `DDHUG44QC7` → `appID = DDHUG44QC7.ai.citrate.core`.
- Mac note: the app **defers** the `associated-domains` entitlement — AASA/universal links are
  inconsistent for a Developer-ID .dmg and an unauthorized entitlement can break notarization. DGX may
  publish the AASA as future-proofing, but the **custom scheme is what carries the join** on a .dmg.

**Short-code resolver (DGX-owned, ✅ CONTRACT LOCKED 2026-09-01 — DGX shipping on citrate-landing):**
```
POST /api/join        → { code, url }
    body: { clusterId?, clusterName?, inviter, inviterAddress?, goal? }
    (inviter = display handle; inviterAddress = attribution, stored PRIVATE — never returned)
GET  /api/join/<code> → { cluster, clusterName, inviter, goal, exp, sig }   (rate-limited; EdDSA-signed)
GET  /api/join/jwks   → { keys: [publicJwk] }   (verify `sig` on the resolve)
```
- Mac ✅ confirms the contract as-is (no tweaks). Attribution kept server-side/private = respects
  red-team F4. Field mapping app↔API: `by`↔`inviter`, `c`↔`clusterName`, `g`↔`goal`, `ref`↔`inviterAddress`,
  path `<clusterId>`↔`clusterId`.
- **Mac follow-up (mine, after the DGX PR lands):** mint via `POST /api/join`; teach `parseJoinLink` to
  accept an opaque `/join/<code>` and verify `sig` against `/api/join/jwks`. Self-contained links keep
  working in parallel (back-compat).

**Download redirect (✅ DMG URL ready — DGX wire the redirect):** the light DMG (355MB) is a **GitHub
Release** asset (no bucket needed). Point `citrate.ai/download/mac` →
`https://github.com/CitrateNetwork/citrate-core/releases/latest/download/Citrate-Core-macos-arm64.dmg`
(stable asset name; survives re-cuts). Release: `v0.1.0-alpha.1` (prerelease), notarized + stapled.

**Model blob + `CITRATE_MODEL_URL` (✅ DGX stood up the seam 2026-09-01 — LIVE):**
```
CITRATE_MODEL_URL = https://citrate.ai/download/model
   → 307 → huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_0.gguf
   verified end-to-end: final content-length = 4,590,807,392 = model.rs MODEL_SIZE_BYTES ✓
```
A stable, Citrate-controlled URL for the first-run Gemma fetch — the `model.rs` config seam
("a Citrate CDN mirror can override" the default). **Mac: bake `CITRATE_MODEL_URL=https://citrate.ai/download/model`
into the light-client build** (or set it as the `model.rs` default). Safe because the app pins the
sha256 (`a555b900…`) + quarantines on mismatch → any backing store must serve byte-identical bytes.
`307` (temporary) so DGX can flip the backing store HF → a DO Spaces mirror later with **zero app rebuild**.
The 4.28GB blob exceeds the 2GB GitHub-Releases cap, which is why it lives off-Releases. The dedicated
DO Space mirror is an owner-gated follow-up (needs Spaces access keys — DO API token can't mint them);
not an alpha blocker, HF is the byte source today.

**Updater feed (WO-2):** `releases/latest/download/latest.json` on GitHub Releases (tiny, fine on GH),
but its **artifact URLs must point at the bucket**, not GH (2GB asset cap). Needs `TAURI_SIGNING_PRIVATE_KEY`.

## Status board

### Mac team (citrate-core) — merged to `main`
- #205 wallet-derived portable identity · #206/#207/#208 stability (no-pinwheel, login off-thread, orphan self-heal)
- #210 invite surface + referral link · #212 relay client wiring · #214 relay **default-on** (connects out of the box)
- #215 `citrate://` deep-link → Groups invite banner + Request-to-join
- Notarized DMG built (clean Gatekeeper, liblzma fixed, relay baked in) — on owner's Desktop
- Planset #209 (Stage-2, red-teamed) · rewards spec #211 · rewards sim harness #213

### DGX team (citrate-landing) — as reported
- #43 `/join/<cluster>` page (self-contained links) — MERGED
- #44 "Open in Citrate" `citrate://join` button — MERGED (activates once the Mac build with #215 ships)
- **#45 short-code resolver — MERGED** — API matches the locked contract byte-for-byte (`POST /api/join`,
  `GET /api/join/<code>`, `GET /api/join/jwks`; EdDSA-signed resolve; `inviterAddress` stored private,
  never returned). Neon/Drizzle/jose; `tsc` clean + 7/7 unit tests; merged after a green Vercel preview.
  **Safe-to-merge-inert:** routes `503` + the page falls back to S0 rendering until activated.
  **Activation (owner/infra):** `npm run db:generate` + apply the `join_codes` migration, then set
  `JOIN_SIGNING_JWK` (an Ed25519 JWK) in Vercel. Gen: `node -e "const{exportJWK,generateKeyPair}=require('jose');(async()=>{const{privateKey}=await generateKeyPair('EdDSA',{extractable:true});console.log(JSON.stringify({...await exportJWK(privateKey),kid:'join-1'}))})()"`
- **#45 also publishes the AASA** `/.well-known/apple-app-site-association` (`DDHUG44QC7.ai.citrate.core`,
  paths `/join/*` + `/join`; `vercel.json` sets `Content-Type: application/json`) — future-proofing;
  the `citrate://` scheme stays the carry path, entitlement deferred per the decision log.

## Open asks / blockers (owner in brackets)
- ✅ **Resolver API contract** [DGX] — LOCKED (above); Mac wires the adaptation after DGX's PR lands.
- ⬜ **Light-client go/no-go** [owner] → the pivotal unblock for download+updates+funnel (Mac builds it).
  Determines the artifact size the bucket must hold (5GB full vs ~few-hundred-MB light + a model blob).
- 🟡 **Object-storage bucket** — model-fetch UNBLOCKED via the `CITRATE_MODEL_URL` seam
  (`citrate.ai/download/model`, HF-backed, LIVE — see Interface contracts). The dedicated **DO Spaces
  mirror** is the remaining, owner-gated piece: needs **Spaces access keys** (S3-style key/secret) created
  in the DO console — the DO API token can't mint them and `doctl` has no `spaces` command. When keys land:
  DGX creates the Space, uploads the blob (verifying sha256 == `a555b900…`), flips the `/download/model`
  redirect target — no app change. **Not blocking alpha.** (`download/mac` already points at the DMG.)
- ⬜ **`TAURI_SIGNING_PRIVATE_KEY`** [owner/infra] + latest.json artifacts → bucket (WO-2).
- ⬜ **Relay at scale** (F5 sharding/DDoS posture) [DGX/infra] — not blocking alpha, don't single-home silently.
- ⬜ **X/Discord prod OAuth creds + Discord bot** [owner/DGX] — social-readiness workstream (its own planset).

## Open PR review + merge queue (both teams clear this together)
Cross-review protocol: DGX reviews Mac PRs touching a shared contract; Mac reviews DGX landing PRs
touching a shared contract. Merge when reviewed + green.

**citrate-core (Mac):**
- **#218 feat(light-client)** — ✅ built + verified: **326MB DMG / 620MB .app** (was 4.2GB/4.9GB); Gemma
  no longer bundled (fetched first-run), bge stays. Fits GitHub Releases (2GB cap) → updater viable.
  **This is the artifact for the DGX bucket/`download/mac` redirect.** → **DGX please review + merge.**

**citrate-landing (DGX) — MERGED (post-merge review welcome; both match the locked contracts):**
- ✅ **#45** S1b short-code resolver — API == the locked contract above. Landed on `main` after a green
  preview build; inert until activated (migration + `JOIN_SIGNING_JWK`).
- ✅ **#45** AASA `/.well-known/apple-app-site-association` (`DDHUG44QC7.ai.citrate.core`).

**DGX ⇒ Mac's #218 (light-client):** reviewing now — if sound I'll merge it (you asked DGX to). It
unblocks `citrate.ai/download/mac`: at 326MB the DMG fits GitHub Releases, so once you cut a signed
release I point `download/mac` → `releases/latest/download/<dmg>`; only the fetched-first-run model
blob needs the bucket, which I'll stand up + hand you a `CITRATE_MODEL_URL`.

## Mac-team follow-ups (from DGX gateSec review, now that the relay is default-on)
- ⬜ **Relay-aware health + WsRelay reconnect** [Mac] — DGX flag A: with default-on, a mid-session relay
  drop currently reports "healthy" (the daemon health probe only checks the UDS socket) while every op
  fails, and WsRelay has no reconnect. Fix = (1) relay-aware health probe in `comms.rs` (report degraded
  when the relay link is down, not just the socket), (2) WsRelay auto-reconnect in the daemon
  (citrate-comms). Not an alpha blocker but hits every user on a relay blip — real correctness bug.
  Accepted as a Mac follow-up.

## Tri-platform release contract (Mac + Linux now; Windows fast-follow)
One GitHub release (`v0.1.0-alpha.1`) carries all platform assets under stable,
platform-tagged names. The website redirects point at `releases/latest/download/<asset>`.

| Platform | Asset (release contract name) | Config | Status |
|---|---|---|---|
| macOS arm64 | `Citrate-Core-macos-arm64.dmg` | `tauri.bundle-lite.conf.json` | ✅ live — `download/mac` → 307 (DGX confirmed) |
| Linux x64 | `Citrate-Core-linux-x86_64.AppImage` (+ `.deb`) | `tauri.bundle-linux.conf.json` | 🔨 runbook ready (`docs/RELEASE_LINUX.md`); needs a Linux build host |
| Windows x64 | `Citrate-Core-windows-x86_64-setup.exe` | `tauri.bundle-windows.conf.json` | 📋 runbook for partner agent (`docs/RELEASE_WINDOWS.md`) |

**DGX asks (when assets land):** point `citrate.ai/download/linux` →
`releases/latest/download/Citrate-Core-linux-x86_64.AppImage` (mirror the `download/mac`
pattern), and stage `citrate.ai/download/windows` → the `-setup.exe`. A tiny
UA-sniff on `citrate.ai/download` (→ the matching platform) would make the join
page's single "Download" button correct on any OS; not a blocker.

All three configs are the **light** client: no bundled Gemma GGUF (fetched
first-run), so every asset fits the GH Releases 2GB cap. Updater artifacts are OFF
on all three until `TAURI_SIGNING_PRIVATE_KEY` is provisioned (one shared key,
one shared `latest.json` gate).

## Decisions log
- 2026-09-01 — tri-platform light client: Linux (AppImage+deb) + Windows (NSIS) configs + runbooks
  added; asset-name contract above; Linux needs a build host, Windows handed to the partner agent.
- 2026-08-31 — 32k-SALT membership bond = the A1 reward/sybil floor; gift memberships; orgs = groups.
- 2026-09-01 — relay **default-on** for alpha (F5 tradeoff accepted, off-switch env kept).
- 2026-09-01 — lead with `citrate://` scheme; **defer** AASA associated-domains entitlement (notarization safety).
- 2026-09-01 — self-contained join links now; opaque short-codes when the DGX resolver lands.
- 2026-09-01 (DGX) — S1b resolver shipped (#45), API == the locked contract; EdDSA-signed resolve,
  attribution kept private; safe-to-merge-inert (activate with the migration + `JOIN_SIGNING_JWK`).
  AASA published (`DDHUG44QC7.ai.citrate.core`); entitlement stays deferred — `citrate://` is the carry path.
- 2026-09-01 (DGX) — `CITRATE_MODEL_URL` stood up as a Citrate-controlled vanity redirect
  (`citrate.ai/download/model` → HF Gemma, landing #47, LIVE + size-verified). Chosen over a bare HF URL so
  the backing store can move HF → DO Space with a redirect flip, no app rebuild; sha256 pin makes it safe.
  Dedicated DO Space mirror deferred to an owner Spaces-key drop (not an alpha blocker).
- 2026-09-01 — DOWNLOAD ROOT CAUSE: citrate-core is a PRIVATE repo → its GitHub release assets 404 for
  anonymous users, so the `/download/{mac,linux,windows}` → `releases/latest/…` redirects could never
  serve public downloads (the model worked only via public HF). Fix: dynamic OS-detecting `/download`
  route (landing #49) → a PUBLIC DO Spaces mirror via `DOWNLOAD_BASE`; phones → `/download/desktop`
  handoff; never a 404. Static `/download/*` redirects removed; `/download/model` kept.
- 2026-09-01 — DO Spaces installer mirror LIVE (owner minted the Spaces key). Space `citrate-cdn` (nyc3),
  public-read + CDN; `DOWNLOAD_BASE = https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads`. Mac
  verified end-to-end: `citrate.ai/download` (Mac UA) → CDN `Citrate-Core-macos-arm64.dmg` → 200
  (371,975,352 B, sha256 `83f8ae9a…`). Linux/Windows = upload to `downloads/` + flip `built:true` in
  `src/lib/download.ts` (one line each) when the assets build; no landing change.
- 2026-09-01 — NOTARIZATION + MIRROR (clarification): serving the DMG from the DO Space needs NO
  re-notarization and nothing handed back. Notarization is STAPLED INTO the `.dmg` bytes; the mirror
  serves the exact sha256-verified bytes DGX pulled from the private release, so Gatekeeper is satisfied
  wherever it's hosted. Re-notarization is only ever needed if the BYTES change (re-sign / re-bundle /
  new build) — not for a byte-identical rehost.
- 2026-09-01 — MODEL MIRROR = GO (owner): mirror the Gemma GGUF into the Space's `models/` prefix and
  flip `/download/model` → the Space (drop the HF dependency). DGX self-serve — the integrity pins are on
  `main`: `MODEL_SHA256 = a555b900…`, `MODEL_SIZE_BYTES = 4_590_807_392` (model.rs). `CITRATE_MODEL_URL`
  stays `https://citrate.ai/download/model` (the #226 default is untouched); the app re-verifies sha256 on
  fetch and quarantines on mismatch, so a byte-identical mirror is safe. No app change, no rebuild.
