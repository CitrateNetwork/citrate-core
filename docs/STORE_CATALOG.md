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

An expiry is parsed strictly: all digits (unix seconds, or milliseconds), an RFC 3339 full-date
(`YYYY-MM-DD`, midnight UTC), or an RFC 3339 date-time with a real calendar date, in-range time
fields and an explicit offset (`Z` or `±HH:MM`, applied). Anything else, including trailing text
or a date-time without an offset, counts as lapsed. The tier names are a hard-coded allowlist **on
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
- **Remove.** One call, no key. A target removed elsewhere counts as removed. For a started
  daemon the app first pauses it, stops its run and any queued claim of it, and waits until
  each has ended and been recorded; core refuses to delete a daemon with a run in flight
  (`daemon_running`) unless that run is past the stale-run release. A run that ends after its
  daemon is gone is refused and is not written to the journal. A listing whose
  install is still running cannot be removed (K5-F): core refuses it with `install_running`
  and the screen shows "Installing…" instead of Remove until the install ends. A pending record
  left by an install that is no longer running is removable.

Apps, widgets, skills and MCP servers are listed but answer "Not installable yet".

## Daemon permissions bind its tools (deny by default)

A daemon installed from the Store is stored with the tools its approved permissions grant, and
its runs are offered and may call only those (`store_catalog.rs::PERMISSION_TOOLS`, enforced in
`src/daemons/turn.ts`). A permission not in the table, or one that names no core tool, grants
nothing; no permissions means no tools. Every effectful tool it may call still needs the member's
explicit decision in each run. A daemon the member writes in the Daemons panel keeps the
member-daemon tool set.

| Permission | Tools |
|---|---|
| `local_model` | none (it is where the daemon runs) |
| `memory_read` | `memory_recall`, `memory_search` |
| `memory_write` | `memory_assert` |
| `journal_read` | `journal_read` |
| `journal_write` | `journal_append` |
| `chain_read` | `node_status`, `staking_status`, `contract_view`, `get_verified_source` |
| `groups_read` | `groups_list`, `group_roster` |
| `models_read` / `skills_read` | `models_list` / `skills_list` |
| `calendar_read` / `sheets_read` / `schedule_read` | `calendar_list` / `gsheets_read` / `schedule_list` |

### Tools the Hermes sidecar hosts itself

The Hermes sidecar adds its own tool sets to **every** session while they are on, whatever core
passes for that session. There is no per-session switch for them yet (follow-up in
citrate-agent-runtime: a session flag to turn host tool sets off). Until there is, core does not
start a Store daemon while any set its approved permissions do not cover is on: Start (resume)
is refused with the reason and how to run it sandboxed, and a run that comes due is recorded as
`not_started` instead of running (`daemons.rs::HOST_TOOL_SETS`, `resume_checked`,
`claim_due_with`; the check reads the environment the running sidecar was started with).

| Tool set | Tools | On when | Covering permission | How it is excluded today |
|---|---|---|---|---|
| Web search and page reading | `web_search`, `read_url` | `CITRATE_HERMES_SEARCH=1` | `web_search` | Daemon not started while on; the member turns it off in Settings > Web search & decisions |
| Hermes's browser | `browser_*` | `CITRATE_HERMES_BROWSER=1` | `web_browse` | Daemon not started while on; Settings > Web search & decisions |
| MCP servers | `mcp__<server>__<tool>` | an MCP allowlist is written (`CITRATE_HERMES_MCP`) | `mcp` | Daemon not started while any server is on; Settings > MCP servers |
| Contract toolchain | `forge_test`, `slither_scan`, `aderyn_scan`, `medusa_fuzz` | `CITRATE_HERMES_TOOLCHAIN=1` | `toolchain` | Daemon not started while on; Agent > Contracts |
| Installed skills | `skill_load` | a skills library is configured (`CITRATE_HERMES_SKILLS`) | `skills_read` | Daemon not started while on; no app switch yet |
| Skill learning | `learn_propose` | `CITRATE_HERMES_LEARN_DIR` | `learn_propose` | Daemon not started while on; no app switch yet |
| Installed capsules | the installed capsules | `CITRATE_HERMES_CAPSULES` | `capsules_run` | Daemon not started while on; no app switch yet |
| File, sheet, project and undo tools | `file_*`, `fs_*`, `sheet_*`, `project_create` | a grant document in the session | none | Daemon sessions are opened with no grant document |
| `shell_run` | `shell_run` | `CITRATE_HERMES_SHELL_RUN=1` and folder grants | none | No grant document; and in a daemon run every command is declined (no one approves it) |

The app ships with the starter capsules and the bundled skills, so in practice **a Store daemon
does not start until the runtime switch exists**, whatever its permissions. That is deliberate:
"no tools" must be true, not approximately true.

The prompt of a Store daemon run is the plain agent prompt with no live app snapshot (balances,
wallet, node state); only what its approved tools return reaches it. A member-written daemon keeps
the chat prompt with its context.

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
- citrate-agent-runtime: a session flag to turn the sidecar-hosted tool sets off (all of them,
  or all but the ones a Store daemon's permissions cover), so a sandboxed daemon run no longer
  depends on the member turning sets off for every session.
- Sign-off on the placeholders: catalog lifetime cap, stage expiry, template size cap.
