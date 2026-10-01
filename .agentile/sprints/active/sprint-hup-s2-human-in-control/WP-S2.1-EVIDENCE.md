---
created: 2026-10-01T00:00:00Z
branch: hup/n4-grants-e2e
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S2
wp: HUP-S2.1 (end to end, core half)
---

# HUP-S2.1 evidence: Hermes folder grants, end to end (core half)

Spec: planset `04_FEATURES_BDD.md` US-2.1 (AC1-AC4), `05_SPRINTS_AND_WPS.md` S2.1.
Grant model and check semantics (one source): citrate-agent-runtime `agent-grants/README.md`.
Runtime half (sessions, file tools, toolchain roots): citrate-agent-runtime branch
`hup/n4-grants-e2e`, `.agentile/sprints/active/2026-10-01-HUP-S2.1-wire-grants-sessions.md`.
Federation sprint issue #279.

## What shipped

- `src-tauri/src/agent_grants.rs` (new): the grant store. The document is the
  citrate-agent-grants `GrantState` (version 1), stored owner-only at
  `<app data>/agent/agent-grants.json` (temp file + rename). The agent's default-deny list
  covers Citrate app data, so the agent cannot edit its own grants.
  - Missing file: no grants. A file that does not parse or breaks a grant rule (same rules as
    `FolderGrants::from_state`, minus the deny list the sidecar runs): **corrupted**, which grants
    nothing. The panel says so, nothing is written over it, and "Set the unreadable file aside"
    moves it to `agent-grants.json.corrupt-<time>` and starts empty. The agent receives an empty
    document meanwhile.
  - Folder grants: the picked folder is canonicalized (symlinks resolved) and must exist; read and
    write are separate grants (one per ticked box). Credential folders under home (.ssh, .aws,
    .gnupg, .kube, .docker, .config/gcloud, Library/Keychains) and core's own app data are refused
    early with a clear message; the sidecar's deny list stays the authority on every operation.
  - Full access: read-only over home, exactly 24 h. HIC-1: `agent_grants_full_access_prepare`
    returns the exact statement and a one-shot id valid for 120 s;
    `agent_grants_full_access_confirm(id)` creates the grant. A wrong, reused or late id does
    nothing; it cannot be stacked while on; "Turn off now" revokes it.
  - Commands (async via `blocking::off_main`, added to `generate_handler!` and the main-window
    ACL; pop-outs get none): `agent_grants_view`, `agent_grants_add_folder`,
    `agent_grants_revoke`, `agent_grants_full_access_prepare`,
    `agent_grants_full_access_confirm`, `agent_grants_reset`.
- `src-tauri/src/hermes.rs`: `hermes_session_open` attaches the document (`grants`) to every
  session body; the manager remembers sessions opened with grants and, after each change, posts
  the whole new document to each (`POST /sessions/:id/grants`). A 404 forgets the session; a
  refusal or transport error is reported back to the panel. The set is cleared on stop.
- `src/agent/grants/` (new): `grants.ts` (types, countdown, desktop IO) and `GrantsPanel.tsx`,
  mounted in Settings > App as "Hermes · folder access": the list with status and countdown,
  revoke, folder picker with Read / Write toggles (read on, write off by default), the full-access
  card with its confirmation countdown, the corrupted state, and a line saying how many open
  Hermes conversations took the change. The web preview says folder grants are desktop-only.
- `src/bridge/tauri/invoke.ts`: deadlines for the four grant-changing commands (each change may
  make up to 8 bounded loopback calls).

## BDD (US-2.1)

| AC | Where it is met | Tests |
|---|---|---|
| AC1 descendants only, symlinks resolved first, read and write separate | sidecar check at use (runtime half); core stores canonical roots, separate read/write grants | `granting_a_folder_creates_separate_read_and_write_grants_and_persists_them`, `a_symlinked_folder_is_stored_as_its_target`; panel: "grants read only unless write is also ticked", "can grant write alone, and refuses neither" |
| AC2 credential stores denied even under full access | sidecar deny list (runtime half); core refuses them as grant roots | `credential_folders_app_data_missing_folders_and_root_writes_are_refused`; panel: "shows a refusal from core as it is" |
| AC3 full access is an HIC-1 toggle, read-only, 24 h, countdown shown | `full_access_prepare` / `confirm`, Grants panel | `full_access_needs_a_one_shot_confirmation_and_is_read_only_for_24_hours`, `a_confirmation_expires`; panel: the four full-access scenarios |
| AC4 TLA+ `FolderGrant` green, traversal fuzz | runtime `agent-grants/formal` (re-run 2026-10-01, no error) and `traversal_fuzz.rs` | see runtime evidence |
| Store is versioned; corrupted grants nothing and the UI says so | `GrantStore::load` / `view` / `reset_corrupted` | `a_corrupted_file_grants_nothing_and_says_so`, `tampered_documents_are_refused_whole` (13 mutants + unknown field); panel: "a corrupted store grants nothing, says so, and offers a reset" |
| Sent on session start and on change | `attach_grants`, `HermesManager::push_grants` | `a_session_opens_with_the_document_and_receives_every_change`, `a_closed_session_is_forgotten_and_a_refusal_is_reported`, `with_no_sidecar_running_a_change_is_stored_and_reported_as_not_sent` |
| Contract with the runtime | `tests/fixtures/agent-grants/core-grant-state-v1.json` (copy of the runtime fixture) | `the_document_matches_the_runtime_contract_fixture` |

## Red to green

- `src/agent/grants/grantsPanel.test.tsx` (14): run first with no module (suite failed to load),
  then green.
- `src-tauri/src/agent_grants_tests.rs` (14): written before the module, but the module was
  written before the first compile, so no separate red run was recorded. Mutation checks
  instead (one edit, run, restore), each killed: confirmation window ignored; any id confirms;
  unbounded full access loads; credential folders grantable; corrupted store sends a non-empty
  document; a 404 session not forgotten; sessions opened with grants not tracked.
- `src/surfaces/settingsFolderAccess.test.tsx` (2): the card is in Settings > App only.

## Gates

- `npx tsc --noEmit` clean. `npx vitest run`: 1003 to 1019 passed (9 skipped, unchanged).
- `cargo test --workspace`: 909 to 923 passed, 0 failed (includes the main-thread tripwire and
  the main-window ACL test).
- `clippy -D warnings` (1.98.1) clean for citrate-core.

## Pending owner sign-off (safe defaults in place)

- Panel placement (Settings > App) and copy.
- Full access covers the home folder and always lasts exactly 24 h; the confirmation is valid
  for 120 s.
- `granted_by` is recorded as "member" (not the wallet address).
- The early credential-folder list in core (the sidecar's deny list is the authority).

## Not done

- No in-chat surfacing of "Hermes asked for a folder" (a grant request from the agent); grants
  are created only from Settings, as designed for this WP.
- A grant changed while the sidecar is stopped is stored and reaches the next session; nothing
  is queued for sessions that no longer exist.
- The sidecar loop itself is still the opt-in preview (Settings > App), so members see no change
  until it is turned on.
