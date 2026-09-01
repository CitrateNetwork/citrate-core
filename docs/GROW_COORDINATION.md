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

**Short-code resolver (DGX-owned, ⬜ contract TBD):** when live, replaces the self-contained link with
opaque codes. DGX to fill in:
```
POST /api/join         → { code }                 (mint; body: { clusterId, inviter, goal? })
GET  /api/join/<code>  → { cluster, inviter, goal } (signed resolve; rate-limited; private attribution)
```
- Mac follow-up (mine, after contract lands): mint via `POST /api/join`; teach `parseJoinLink` to accept `/join/<code>`.

**Download redirect (⬜ needs bucket URL):** `citrate.ai/download/mac` → the placed `.dmg` in object
storage (R2/DO Spaces/S3). DGX wires the redirect once the Mac team supplies the bucket URL (or the
GitHub Release URL if we ship the light client).

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
- feat/grow-s1b-join-resolver — short-code resolver — **un-paused, DGX-owned** (Neon/Drizzle/jose; needs join_codes migration + JOIN_SIGNING_JWK)

## Open asks / blockers (owner in brackets)
- ⬜ **Object-storage bucket + public .dmg URL** [DGX/infra] → unblocks `citrate.ai/download/mac`.
- ⬜ **Resolver API contract** [DGX] → Mac wires the mint/short-code adaptation.
- ⬜ **`TAURI_SIGNING_PRIVATE_KEY`** [owner/infra] + latest.json artifacts → bucket (WO-2).
- ⬜ **Light-client go/no-go** [owner] → the pivotal unblock for download+updates+funnel (Mac builds it).
- ⬜ **Relay at scale** (F5 sharding/DDoS posture) [DGX/infra] — not blocking alpha, don't single-home silently.
- ⬜ **X/Discord prod OAuth creds + Discord bot** [owner/DGX] — for the social-readiness workstream (its own planset).

## Decisions log
- 2026-08-31 — 32k-SALT membership bond = the A1 reward/sybil floor; gift memberships; orgs = groups.
- 2026-09-01 — relay **default-on** for alpha (F5 tradeoff accepted, off-switch env kept).
- 2026-09-01 — lead with `citrate://` scheme; **defer** AASA associated-domains entitlement (notarization safety).
- 2026-09-01 — self-contained join links now; opaque short-codes when the DGX resolver lands.
