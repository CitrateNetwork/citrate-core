---
created: 2026-10-19T00:00:00Z
branch: beta/k5-store-v1
author: Larry Klosowski + Claude Opus 5.5
status: draft (catalog key, catalog host and placeholder values pending the key ceremony and owner sign-off)
wp: K5a (0.5.2 betanet)
---

# Store v1: the signed catalog

The Store lists only what a signed catalog says, verified in the app against the catalog key
from the genesis key ceremony. Code: `components/src/catalog.rs` (verifier, schema v1) and
`src-tauri/src/store_catalog.rs` (commands, staging, paused installs, removal). Component
updates are a separate feed with a separate key: see [`COMPONENT_UPDATER.md`](COMPONENT_UPDATER.md).

## Trust

- **Key slot.** `PRODUCTION_CATALOG_PUBKEY` is empty until the ceremony. With it empty, every
  catalog command answers "Store catalog not available yet: signing key not issued" before any
  network or disk access. Only the public half is committed.
- **Key separation.** The catalog key must not be the app-updater key or the component key. Keys
  are compared as decoded Ed25519 public keys, so the same key under another minisign key id is
  still refused.
- **Signature.** Minisign, prehashed (`ED`) only, over the exact catalog bytes, with a trusted
  comment starting `citrate-store-catalog`. Size cap (1 MiB) first; no JSON is read before the
  signature verifies.
- **First-party only.** A listing with `first_party: false` fails the whole catalog in this
  version. Third-party submissions with a review step come after mainnet.

## Rollback window after app data is cleared

Anti-rollback keeps the highest catalog sequence seen (and its digest) in
`<app data>/store/state.json`. A lower sequence, or the same sequence with different bytes, is
refused. **Clearing app data (or a fresh install) clears that record.** After a reset, the only
bound on replaying an older catalog is its own expiry: an older catalog is accepted again until
its `expires_at`, and the lifetime cap (`MAX_CATALOG_LIFETIME_SECS`, 31 days, placeholder) is
therefore the rollback window after a reset. Pinned by the test
`clearing_the_recorded_sequence_leaves_only_the_lifetime_cap_as_the_rollback_bound`.

For the ceremony runbook: keep catalog lifetimes short (re-sign often rather than issue
long-lived catalogs), and withdraw a listing by publishing a new sequence well before the old
catalog expires. A sequence high-water mark that survives an app-data clear (for example in the
OS keyring) is a possible later hardening; it is not in v1.

## Tier gating

Core enforces the tier, fail-closed, from the claim-derived `AuthStatus`:

| Member state | Rank | Reaches |
|---|---|---|
| Signed out, no tier claim, `free` / `public`, or a lapsed entitlement | 0 | `public` listings |
| `pilot`, `member`, `commercial`, `commercial.kyc`, `academic` | 1 | `public`, `member` |
| `enterprise`, `confidential` | 2 | everything |
| Any other tier name | -1 | nothing, not even `public` |

An expiry that does not parse counts as lapsed. The tier names are a hard-coded allowlist **on
purpose**: a tier added in identity later locks its members out of every listing, public
included, until core ships the new name. Adding a tier to identity therefore needs a core release
alongside it.

## Install path

Verify → stage → review → approve (HIC) → install paused.

- **Stage.** The cached verified catalog is re-verified, the tier and kind are checked, and for a
  daemon the template is downloaded and its size and SHA-256 checked. For a model the hash is
  checked as it downloads, after approval.
- **Approve.** A single-use stage id, valid for 15 minutes (placeholder), bound to the listing
  exactly as reviewed.
- **Install.** The install record is saved **before** the target is created, so the Store never
  leaves an install it does not track: if the record cannot be saved, nothing is created; if
  the target fails, the record is taken back out; if only the final confirming write fails, the
  record stays and still tracks the install. Daemons are created paused in one write of the
  daemon book; models are downloaded and verified and not selected.
- **Remove.** One call, no key. A target removed elsewhere counts as removed. A listing whose
  install is still running cannot be removed (K5-F): core refuses it with `install_running`
  and the screen shows "Installing…" instead of Remove until the install ends. A pending record
  left by an install that is no longer running is removable.

Apps, widgets, skills and MCP servers are listed but answer "Not installable yet".

## Counts

The Store shows no "N skills" or "N models" figure. Any such figure must read
`SkillRegistry.totalSkills()` / `ModelRegistry.totalModels()` on chain (UI truth contract §2),
never a count of catalog listings. The catalog is not cross-checked against the registries in v1.

## Owner and DGX items

- The catalog key (key ceremony).
- Hosting `downloads/store/v1/catalog.json` and `catalog.json.minisig` on the CDN.
- Artifact URLs with pinned SHA-256 and size; daemon templates are JSON
  `{name, prompt, schedule}` up to 64 KiB; model URLs end in a plain `.gguf` file name.
- An HIC outbox decision kind for the install approval (the agent-runtime sidecar's
  `/records/core` must accept it first).
- Sign-off on the placeholders: catalog lifetime cap, stage expiry, template size cap.
