---
title: "Sprint BC-5/BC-6 — on-chain SBT art + real billing + honest storage/SSO/comms seams"
created: 2026-07-19
branch: sprint/bc-5-6-storage-sso
author: Claude (Opus 4.8) for SaulBuilds
status: active
planset: citrate-federation/.agentile/planset/2026-07-19-core-beta-completion/ (BC-5, BC-6)
---

# Sprint BC-5/BC-6 — app-side buildable slices + honest seams

BC-5 and BC-6 are P1 "seam + light-up" phases; much is WO/infra-gated. This sprint builds
the app-side slices that are buildable NOW and makes every surface honest + ready to link.

## Grounded facts (verified 2026-07-19)
- **BC-5.3 SBT art is FULLY ON-CHAIN.** Post-reroll `CitrateMemberSBT` (0x7bE005aA…A7C4) has
  `tokenURI(uint256)→string` returning `data:application/json;base64,…` with an embedded
  `data:image/svg+xml;base64,…` emblem from `MemberEmblem.render(owner)` (wholly on-chain).
  Also `tokenIdForSub(bytes32)→uint256`, `getMember(uint256)→tuple`, `isActive`. So app-side =
  READ the authoritative on-chain tokenURI and render it (NO IPFS, NO WO-5). The local
  `src/identity/sbtArt.ts` becomes the honest offline fallback (label which is shown).
- **BC-6.3 real expiresAt EXISTS.** Entitlement claims carry `expires_at` (`oidc.rs:564`
  `pub expires_at: Option<String>`; bridge `AuthStatus.expiresAt`). Settings "Memberships &
  billing" currently renders HARDCODED dates (Settings.tsx:119 "2027-07-11" etc.) — Rule-1 gap.
- **BC-6.2 already wired:** tutorials open `tu.url` (Dashboard.tsx:268) + "open in Atlas" opens
  `federationUrl("atlas/docs/"+id)` (Commissary.tsx:347). Real navigation; silent-SSO is WO-6/WO-8.

## Work packages (buildable now)
- **BC-5.3 [core] — on-chain SBT art.** New Rust read `sbt_token_uri(token_id)` + `tokenIdForSub`
  (mirror staking.rs eth_call + pinned selectors + keccak drift tests; SBT addr 0x7bE005aA…).
  Bridge `identity.sbtArt(member)` → resolves tokenId via `tokenIdForSub(keccak256(sub))` then
  reads `tokenURI`; decode the base64 JSON → the SVG `image`. Render the ON-CHAIN emblem in the
  Wallet/Identity SBT card + Sidebar; keep `sbtArt.ts` as the honest labelled offline fallback
  when the on-chain read is unavailable. Rule 1: caption names the on-chain source.
- **BC-6.3 [core] — real billing expiresAt.** Settings billing reads the REAL `expiresAt` from the
  entitlement claim; render it (honest "—" when null/absent), kill the hardcoded dates. Real tier/
  entitlement already fold from identity. "Manage billing / renew" opens the core-membership portal
  seam — honest "coming" until the Stripe customer portal lands (core-membership CORE-S5.4).

## Honest seams (build the seam now, light up when the WO lands — do NOT fake)
- **BC-5.1 memory ingest** (chain-facts → Storage graph): the Storage constellation reads the REAL
  mem-mcp store, which is EMPTY because nothing ingests. Populating it needs a daemon WRITE/assert
  path (upstream citrate-memories) or a citrate-core ingestor once the daemon accepts asserts. Keep
  the honest empty-graph state; document the upstream dep. (Do not fabricate nodes.)
- **BC-5.2 IPFS pinning** (WO-4): keep/seam a `pin_add` client + keyring token seam returning honest
  "pinning not configured" until the IPFS Pinning Service endpoint + token land. Bonded PoSt = E-3.
- **BC-6.1 silent SSO** (WO-6): app-side already opens RP URLs (Atlas/tutorials real). Silent
  arrive-signed-in needs authority `prompt=none`/session-cookie support — honest until then (the URL
  opens; the user may see the authority session). Zero token egress (ADV-8) — app supplies only URLs.
- **BC-6.4 comms pings** (WO-9): keep the honest empty state until the notifications API lands.
- Relabel any remaining fabricated Settings toasts to honest states.

## Testability / red-green
- Rust: SBT tokenURI/tokenIdForSub selectors keccak-drift-tested; decode base64 data-URI → SVG
  (fixture); a member with no SBT reads honestly (no fabricated art). Rule 2 monotone; zero new unwrap.
- vitest: SBT card renders the on-chain SVG when available + labelled fallback when not; billing shows
  the REAL expiresAt (negative control: a distinct non-hardcoded date; "—" when null; no "2027-07-11").
