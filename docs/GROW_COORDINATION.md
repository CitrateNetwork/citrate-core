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
Only the fetched-first-run **model blob** still wants a bucket + a `CITRATE_MODEL_URL` (DGX to stand up).

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
- ⬜ **Object-storage bucket** — DGX OFFERED to stand it up (R2/DO Spaces/S3). PLAN: Mac ships the
  **light** DMG (GitHub Releases-hostable) + hands DGX the **model blob** URL to place in the bucket +
  a `CITRATE_MODEL_URL`; DGX points `citrate.ai/download/mac` at the DMG. Confirm once light-client is a go.
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

## Decisions log
- 2026-08-31 — 32k-SALT membership bond = the A1 reward/sybil floor; gift memberships; orgs = groups.
- 2026-09-01 — relay **default-on** for alpha (F5 tradeoff accepted, off-switch env kept).
- 2026-09-01 — lead with `citrate://` scheme; **defer** AASA associated-domains entitlement (notarization safety).
- 2026-09-01 — self-contained join links now; opaque short-codes when the DGX resolver lands.
- 2026-09-01 (DGX) — S1b resolver shipped (#45), API == the locked contract; EdDSA-signed resolve,
  attribution kept private; safe-to-merge-inert (activate with the migration + `JOIN_SIGNING_JWK`).
  AASA published (`DDHUG44QC7.ai.citrate.core`); entitlement stays deferred — `citrate://` is the carry path.
