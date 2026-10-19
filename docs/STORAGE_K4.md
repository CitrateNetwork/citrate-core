---
created: 2026-10-19
branch: beta/k4-storage
author: Larry Klosowski (with Claude Opus 5.5)
status: active
---

# Storage in 0.5.2 (K4): files, memory, local pins, verified backup

Board item: **K4 · Storage finished**. Plan: betanet plan, section K4. Truth rules: UI truth
contract v1, section 1 (Pins), section 2 (pin bond parameters), section 4 (IPFSIncentivesV3 dormant).

## 1. Components

Node › Storage is composed (by the five-place nav work) from four self-contained components,
exported together from `src/surfaces/storageParts.ts`:

| Component | Route today | Data source |
|---|---|---|
| `StorageFiles` (Files) | `files` | `storage_*` commands → local kubo API (127.0.0.1:5001) + pin index |
| `Storage` (Memory) | `storage` | `memory_*` commands → supervised `mem-mcp` daemon socket |
| `StoragePins` (Pins) | `pins` | `storage_list` (pin index ∪ kubo `pin/ls`), `storage_pin_params` (IPFSIncentivesV3 `BOND()`, `REWARD()`, `ROUNDS()` by `eth_call` on 40204) |
| `StorageBackup` (Backup) | `backup` | `storage_backup_export` / `_verify` / `_restore`, progress on `storage://backup-progress` |

Until Node › Storage lands, all four are tabs of the Files hub.

## 2. Pins: "Local pin · no bond"

IPFSIncentivesV3 is deployed but not funded and holds no pins, and the on-chain bond (S2.2) is
blocked on the CommD question (`docs/FINDING_PIN_COMMD_BOND_2026-08-26.md`). So in this build:

- Every pin is a **local pin**: `storage_pin_local` pins the CID in this node's kubo store and
  records `local` in the pin index. No bond, no transaction. The Files and Pins surfaces only call
  this. The older bonded `storage_pin` command (local pin plus a pending `registerModel`
  signing request carrying the 55 SALT model bond) is removed from the invoke handler and the
  main-window allowlist until S2.2 ships, so no path can open a bond signing request
  (`the_bonded_storage_pin_command_is_not_invocable`). The commitment and calldata builders stay,
  tested against the chain's frozen vectors.
- A kept pin is labelled **"Local pin · no bond"**; an index row kubo no longer holds is
  **"Not pinned on this node"**. Pinned / Challenged / Released / Slashed are never shown.
- **"Pin bonds not live yet"** is shown with the V3 `BOND`, `REWARD` and `ROUNDS` values, labelled
  as parameters. A failed read shows "—" and its reason, never 0.

"Local pin · no bond" is the design's wording; the chain team is adding it to UI truth contract
v1.2.

## 3. Export, backup and restore

Rust does all of it (`src-tauri/src/storage_backup.rs`). The member picks a folder; export writes a
new `citrate-backup-YYYYMMDD-HHMMSS` folder (UTC, never reused, 0700 on Unix):

```text
citrate-backup.json   manifest: format, createdAt, appVersion, parts, entries[], entriesSha256
files/<cid>           each object pinned on this node, bytes read from kubo
memory/<path>         the memory dir, copied with the mem-mcp daemon stopped
```

Each manifest entry has `path`, `size`, `sha256`, the file's Unix `mode` and, for files, `cid`.
`dirs` records each memory store directory with its mode. `parts` lists only the parts the backup
holds; a requested part with nothing to back up (no memory store, an empty one, no pinned files)
is listed in `skippedParts` with the reason instead. `entriesSha256` is a plain SHA-256 over all
of it.

**What the manifest does and does not prove.** The manifest is not signed and the digest is not
keyed. Verification catches a damaged backup, a changed or missing file, and a manifest edit that
did not recompute the digest. It does **not** catch someone who edits both the files and the
manifest and recomputes the digest: anyone who can write to the backup folder can produce a
backup that verifies. Keep backups where only you can write. Because of this, modes read from the
manifest are masked on restore (permission bits only, never group- or world-writable).

**The Memory part of an export is not encrypted.** It is a plain copy of the memory store as it is
on disk (core does not encrypt the store at rest today, see `memory.rs`). The Files part is a
plain copy of each file. A backup is as private as the folder it is written to; the surface says
so.

**Verification** (`verify_folder`, run at the end of every export and at the start of every
restore): the format matches; the entry digest matches; every path is a plain relative path under
`files/` or `memory/` (no absolute paths, `..`, backslashes, repeats; `files/` entries are exactly
`files/<cid>`); every listed file exists as a regular file (never a symlink) with the listed size
and SHA-256; and nothing in the folder is unlisted.

**Restore refuses any mismatch before it touches anything.** Then:

- Only the requested parts the backup actually holds are restored. The report lists those in
  `parts` and names every other requested part in `skipped` with the reason; a skipped Memory
  part never stops the daemon or touches the live store. Nothing to restore is an error.
- Memory: the importer slot and the memory store lock are held (while the lock is held,
  `MemoryManager::start` refuses, so nothing can start the daemon mid-restore), a running daemon
  is stopped, the files are copied into a staging dir beside the store and hashed again there,
  each file and directory gets its recorded mode back, masked so it is never group- or
  world-writable (0600 / 0700 when none was recorded), and the restore checks it took, the current memory dir is moved to `memory.before-restore-<time>`
  (kept, not deleted), the staging dir takes its place, the lock is released, and the daemon is
  started again if it was running.
- Files: each file's bytes are hashed again as read, re-added to kubo and pinned locally. If kubo
  returns a different CID (a CID-version change), the report lists it.

Every dir and file the backup code creates is owner-only from creation: the backup folder and
its contents, and the restore staging dir (a sibling of the memory dir) with everything staged in
it. Dirs are made 0700 via `DirBuilder` mode and files 0600 via `OpenOptions` mode, before any
bytes land, so nothing is readable by other local users mid-copy even when the app-data parent is.
A restored file then gets its recorded mode.

App quit during a long export or restore: shutdown closes the import registry before it stops
memory. The backup restarts a previously running daemon only through the registry gate
(`unless_closed`), so after quit begins it does not start it again (the report notes this).

Not backed up: the daemon socket (`memdag.sock`) and the crash log.

**Known limitations**

- The memory swap is two renames: the live `memory/` dir moves to `memory.before-restore-<time>`,
  then the staging dir becomes `memory/`. If the app is killed (or the machine loses power)
  between them, there is no live `memory/` dir; the previous store is intact in
  `memory.before-restore-<time>` and the restored copy in `memory.restore-staging-<time>`. There
  is no automatic recovery yet: rename `memory.before-restore-<time>` back to `memory/` by hand
  (with the app quit) to get the previous store back. This existed before K4's follow-ups.
- Backups are not signed (see above).

## 4. Storage processes and the supervisor

Checked on macOS in this branch (code read and the tripwire tests in `storage_backup_tests.rs`):

| Process | How it starts | Supervised | Stopped on quit | Orphan sweep at launch |
|---|---|---|---|---|
| `ipfs daemon` (kubo) | `ipfs_start` → `IpfsManager::start` → `Supervisor::start` (started alongside the node; Files can now start it on its own) | yes | yes (`shutdown_all_sidecars`) | yes (`OWNED_SIDECARS` "ipfs") |
| `mem-mcp` daemon | `memory_start` → `MemoryManager::start` → `Supervisor::start` | yes | yes | yes ("mem-mcp") |
| `ipfs init`, `ipfs config` | one-shot `Command::output()` in `IpfsManager::ensure_init`, first start only | no (short-lived, waited for) | n/a | n/a |
| `mem-mcp import-corpus` | one-shot child owned by `ImportRegistry` | no (registry-owned) | yes (stopped first, SIGTERM then SIGKILL) | yes (same binary path) |

Gaps for K1 (not fixed here):

1. `ipfs init` / `ipfs config` have no deadline. A hung `ipfs init` hangs `ipfs_start`.
2. Storage talks to `127.0.0.1:5001` (or `CITRATE_KUBO_API`). If another kubo (for example IPFS
   Desktop) already holds :5001, the supervised daemon fails to bind and Files talks to the other
   one. The app does not check which daemon answers.

### Linux

Same code paths as macOS: the supervisor's SIGTERM → grace → SIGKILL stop and the `/proc`-based
orphan sweep both apply. Needed for containment (K1): put each sidecar in its own process group or
cgroup so a crashed app cannot leave children behind (`PR_SET_PDEATHSIG` or a systemd-run scope),
and confine the kubo repo and memory store to the app data dir with owner-only permissions.

### Windows

The supervisor has no SIGTERM there: stop is `Child::kill` (hard), so kubo and RocksDB get no
graceful shutdown, and the orphan sweep is a no-op (`owned_cleanup` returns nothing on Windows).
The memory socket is a named pipe without read timeouts. Needed for K1: a Job Object per sidecar
with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (no orphans), a graceful stop (`CTRL_BREAK_EVENT` to a
`CREATE_NEW_PROCESS_GROUP` child, or kubo's `/api/v0/shutdown`), and an ACL on the backup folder
(it inherits the chosen parent's ACL today).
