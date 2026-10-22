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
  record stays and still tracks the install. A model install the app left between the download's rename
  and its verify (the app quit in that window) is checked again by its pinned hash on the next
  read of the installed list: confirmed when it matches, otherwise the file and its record are
  removed together. Removing a pending model record also removes its unverified file. Daemons are created paused in one write of the
  daemon book; models are downloaded and verified and not selected.
- **Remove.** One call, no key. A target removed elsewhere counts as removed (a daemon already
  deleted from the Daemons screen included: there is no run left to stop). A model counts as
  the Store's only while its file is verified against the listing's pinned SHA-256 (recorded by
  the verify in the model's status file) and has the listing's size; a different model saved
  under the same file name (for example in Models, after the Store's copy was deleted there)
  shows as missing, and Remove clears only the Store's record and leaves that file alone. For a started
  daemon the app first pauses it, stops its run and any queued claim of it, and waits until
  each has ended and been recorded. A run in Hermes has ended only when the sidecar confirms
  it: the stop is acknowledged, the session reports idle (within 30 s) and is closed. If any of
  that fails, the run is not recorded as ended and Remove is refused ("Could not stop its run"); core refuses to delete a daemon with a run in flight
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

The Hermes sidecar adds its own tool sets to every session while they are on. Whether a Store
daemon may run with them depends on the sidecar:

- **A sidecar with per-session tool sets (citrate-agent-runtime#87 or later).** `GET /status`
  reports `capabilities.sessionToolSets` and `capabilities.sessionPromptSnapshot`. When those
  cover every set below, core opens a Store daemon's session with `toolSets` naming exactly the
  sets its approved permissions cover (an empty list for none), `promptSnapshot: false`, and only
  its approved core tools. The other sets stay off for that session whatever is on elsewhere, so
  the start check is relaxed. Because such a sidecar would fail open if `toolSets` did not
  arrive, the session-create response must echo `toolSets` (the same names) and
  `promptSnapshot: false`; otherwise core closes the session and the run does not start.
  `toolSets` is always an explicit array built in core from the daemon's stored approved
  permissions (never from the webview), and never names `files` or `shell_run`. Relaxing needs
  both capabilities: `sessionToolSets` covering every set below and `sessionPromptSnapshot`.
- **An older sidecar (no `capabilities`, or not every set listed).** Core does not start a Store
  daemon while any set its approved permissions do not cover is on: Start (resume) is refused
  with the reason and how to run it sandboxed, and a run that comes due is recorded as
  `not_started` (`daemons.rs::HOST_TOOL_SETS`, `resume_with`, `claim_due_facts`; the check reads
  the effective environment the running sidecar was started with).

| Tool set | Tools | On when | Covering permission | With a #87 sidecar | With an older sidecar |
|---|---|---|---|---|---|
| Web search and page reading | `web_search`, `read_url` | `CITRATE_HERMES_SEARCH=1` | `web_search` | Off unless in `toolSets` | Daemon not started while on; turn it off in Settings > Web search & decisions |
| Hermes's browser | `browser_*` | `CITRATE_HERMES_BROWSER=1` | `web_browse` | Off unless in `toolSets` | Not started while on; Settings > Web search & decisions |
| MCP servers | `mcp__<server>__<tool>` | an MCP allowlist is written (`CITRATE_HERMES_MCP`) | `mcp` | Off unless in `toolSets` | Not started while any server is on; Settings > MCP servers |
| Contract toolchain | `forge_test`, `slither_scan`, `aderyn_scan`, `medusa_fuzz` | `CITRATE_HERMES_TOOLCHAIN=1` | `toolchain` | Off unless in `toolSets` | Not started while on; Agent > Contracts |
| Installed skills | `skill_load` | a skills library is configured (`CITRATE_HERMES_SKILLS`) | `skills_read` | Off unless in `toolSets` | Not started while on; no app switch yet |
| Skill learning | `learn_propose` | `CITRATE_HERMES_LEARN_DIR` | `learn_propose` | Off unless in `toolSets` | Not started while on; no app switch yet |
| Installed capsules | the installed capsules | `CITRATE_HERMES_CAPSULES` | `capsules_run` | Off unless in `toolSets` | Not started while on; no app switch yet |
| File tools from the launch environment | `fs_*` | `CITRATE_HERMES_FILES=1` and a grants file (no grant document needed); core never sets it | `files` (never granted, never sent) | Off: not in `toolSets` | Daemon not started while on; start Citrate Core without `CITRATE_HERMES_FILES` |
| File, sheet, project and undo tools from a grant document | `file_*`, `fs_*`, `sheet_*`, `project_create` | a grant document in the session | `files` (never granted, never sent) | Off: not in `toolSets`, and no grant document | Daemon sessions get no grant document |
| `shell_run` | `shell_run` | `CITRATE_HERMES_SHELL_RUN=1` and folder grants | `shell_run` (runtime name; never sent by core) | Off: not in `toolSets` | No grant document; every command is declined in a daemon run |

With an older sidecar: the app ships with the starter capsules and the bundled skills (and turns
skill learning on), so in practice **a Store daemon starts only if its approved permissions cover
those sets too** (`capsules_run`, `skills_read`, `learn_propose`), which no first-party listing is
expected to declare. That is deliberate: "no tools" must be true, not
approximately true.

Three more rules close the gaps around restarts and the launch environment:

- **Unknown environment blocks.** A Store daemon is claimed only against a known sidecar
  environment. While Hermes is not running, in crash backoff or restarting after a settings
  change, a due run is recorded as `not_started`.
- **Re-checked right before the session opens.** The claim records which host sets were on.
  `hermes_session_open_unattended` (given the daemon id) refuses the open when the environment is
  now unknown, when the sets changed since the claim, or when an uncovered set is on.
- **Fail closed at open.** The daemon session open must name a daemon the book holds and the run
  core claimed for it, still in flight; a missing or unknown id, a missing run id, another
  daemon's run, or a finished or released run is refused. A refusal at open is recorded as
  `not_started`, not `failed`.
- **Environment names follow the platform.** On Windows, where variable names are
  case-insensitive, `citrate_hermes_search` counts as `CITRATE_HERMES_SEARCH`.
- **The effective environment is checked.** The sidecar inherits the app's own environment, so a
  `CITRATE_HERMES_*` flag in the launch environment would turn a set on without core setting it.
  Core records what the child actually sees (the inherited `CITRATE_*` variables overlaid with
  core's own values) and checks that.

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
- citrate-agent-runtime#87 (per-session `toolSets` and `promptSnapshot: false`) must ship in the
  bundled sidecar for Store daemons to run with host tool sets on elsewhere.
- Sign-off on the placeholders: catalog lifetime cap, stage expiry, template size cap.
