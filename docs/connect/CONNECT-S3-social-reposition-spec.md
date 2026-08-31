---
created: 2026-08-31
branch: feat/connect-s3-social-reposition
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (implemented)
planset: connect-realign
code: CONNECT-S3
repo: citrate-core
companions:
  - docs/CONNECT_REALIGN_PLANSET.md
  - docs/adr/ADR-2026-08-30-social-identity-privacy-model.md
  - src/surfaces/Connections.tsx
---

# CONNECT-S3 — X/Discord repositioning

## The user & the outcome

A member links their X or Discord account and understands, without guessing, **what it does for
them**: it becomes their *face* (a verified `@handle`) to people they share a group with, and it is
how someone can invite them by `@handle` instead of a raw `0x` address. Linking is optional and
useful, never a dead-end "connect for nothing," and the app never implies it imports followers,
friends, or contacts.

## Why this step exists

The Connections surface shipped the social section behind a **`pending backend`** flag with a
"NOT WIRED" header comment. Both are now **false**: the social flow is fully wired on the desktop
build (`src-tauri/src/social.rs`, registered in `lib.rs`):

- `social_start` — public-client loopback-PKCE OAuth ownership proof, token sealed in the OS keyring
  (never crosses the bridge).
- `social_verify_request` / `social_verify_approve` — the wallet signs an `IdentityBinding` through
  the SignatureCeremony (ADR D3). Rule 3 holds: the wallet signs, no sidecar signs.
- `social_set_visibility` — `private` (default) | `groups`.
- `social_export_binding` / `social_ingest_binding` — the verified, group-visible binding rides a
  server-blind group message (`cbind1:` sentinel) and peers verify `sender==address` + signature on
  ingest before storing.
- `social_resolve` — member addresses → the verified faces this device knows.

So the honest problem is **framing**, not plumbing: a wired, useful feature was labelled a dead-end,
and the copy never stated the one thing users kept asking ("what do I get?") or the one thing it must
never claim ("it imports my friends").

## Data-source trace (Rule 7)

| Shown / claimed | Source |
|---|---|
| Linked handle, verified badge, visibility | `bridge.social.status()` → `social_status` (device-local record; `verified` derived from a stored binding) |
| "Verify" opens a wallet signature | `store.verifySocial` → `social_verify_request` → SignatureCeremony → `social_verify_approve` (wallet signs) |
| A handle appears as a **face** in the People directory + add-member picker | `store.refreshPeople()` → `bridge.social.resolve(addrs)` → `buildPeopleDirectory(..., faces, ...)` (CONNECT-S0/S2) |
| "does not import followers/friends" | Truth about X/Discord OAuth scopes (`users.read` / `identify` only) — asserted, not fetched |

## Acceptance criteria

- [x] **A verified handle renders as a face in the directory and the add-member picker.** Already
  wired via `refreshPeople → social.resolve → buildPeopleDirectory`; S3 keeps it and states it in the
  UI. Verified by the existing People/S2 path; `social.resolve` returns only verified group-visible
  faces (`social.rs`).
- [x] **The social section states each network's real job** — *face + invite-by-@handle reachability*
  — in the section intro and per-row subcopy. No `pending backend` flag on a wired feature.
- [x] **No false friend-import claim (Rule 1).** An explicit line states linking proves ownership and
  does **not** import followers, friends, or contacts, because X/Discord don't expose them.
- [x] **Optional, not a dead-end.** The section reads as opt-in and useful (it powers invite-by-handle
  and faces), with private-by-default privacy stated.
- [x] Honesty test asserts the reposition copy is present and the false `pending backend` flag is gone
  from the social section (`connectionsSocialHonesty.test.tsx`).

## What this step does NOT change

- No new backend, no new bridge method, no OAuth scope change. `social.*` is untouched.
- MCP / SaaS / Webhooks sections keep their honest flags — those *are* still pending or partial.
- LinkedIn stays a listed network (same flow); if its OAuth client isn't configured, `social_start`
  returns an honest error — no fake link is shown.
- Invite-by-@handle *resolution* (typing `@handle` → address) is CONNECT-S1/S4 territory; S3 only
  frames the handle as the delivery channel, it does not add a handle→address lookup here.

## Out of scope (v1)

- On-chain public opt-in for a handle (ADR D2 follow-on).
- SaaS/Webhooks wiring (their own runbook items).
