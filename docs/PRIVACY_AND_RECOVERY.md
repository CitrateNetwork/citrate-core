---
created: 2026-10-01T00:00:00Z
branch: hup/n4-recovery-privacy
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S10.5
---

# Privacy and recovery (HUP-S10.5)

Settings, Privacy & recovery holds four things: the device key recovery kit, "delete my
local data", the offline matrix ([OFFLINE_MATRIX.md](OFFLINE_MATRIX.md)) and the default
budget values. The telemetry consent screen sits in Settings, App, next to the
diagnostic report it gates.

## 1. Device key recovery kit

### What it covers

The app mints two random keys of its own in the OS keychain, service `ai.citrate.core`:

| Keychain account | Protects | Code |
|---|---|---|
| `node-storage-key` | the node's chain data at rest | `src-tauri/src/node.rs` |
| `memory-store-key` | the memory store the journal's daily entry draws from | `src-tauri/src/memory.rs` |

Neither can be derived from anything else. If the keychain is lost (a new computer, a
keychain reset) while the app data folder survives (a backup, a migration), that data is
unreadable without them. Note that the memory daemon does not consume its key yet
(`memory.rs` passes it for forward compatibility), so today the kit mainly protects the
node data; it already covers the memory key so no second kit is needed when that lands.

Not covered, and said so on screen:

- **The wallet.** Its vault keys live under `ai.citrate.core.custody` and its own
  recovery is a separate decision (the beta provisioning flow drops the one-time
  mnemonic; see `provisioning.rs`). Owner decision, recorded below.
- **The messaging key** (`comms-member-key-v2`) is re-derived from the wallet.
- **Journal export files** open with the passphrase chosen at export time
  ([JOURNAL_EXPORT_FORMAT.md](JOURNAL_EXPORT_FORMAT.md)). Nothing stores that passphrase.

### The two forms (member's choice)

- **Recovery phrase sheet** (`*.citrate-recovery.txt`): one 24-word BIP39 phrase per key
  (the key bytes are the BIP39 entropy), each under a `[account] fingerprint <16 hex>`
  label. Rust writes the sheet straight to the path the member picked, mode `0600`; the
  words never cross into the webview (I-2). The member prints it, keeps it offline and
  deletes the file.
- **Recovery file** (`*.citrate-recovery`): the keys sealed under a passphrase of at
  least 12 characters. Same construction as the journal export (custody vault Argon2id,
  m=64 MiB t=3 p=1; AES-256-GCM; the header is the associated data; a reader accepts only
  the parameters this build writes) with its own magic `CITRKEY\0`, so a journal export
  can never be read as a kit or the other way round.

Making a kit records each key's fingerprint (first 8 bytes of
SHA-256(`citrate-device-key-fingerprint-v1` ‖ account ‖ 0x00 ‖ key)) in
`<app data>/recovery/fingerprints.json`. The fingerprint is not secret and does not help
recover the key.

### Restore rules (all-or-nothing)

1. A phrase with a wrong or missing word fails its BIP39 checksum or its printed
   fingerprint: refused.
2. A recovery file opened with the wrong passphrase, or altered: refused.
3. A kit whose key does not match the fingerprint this install recorded is from another
   install: always refused, even with "replace".
4. A kit key that differs from a key already in the keychain: refused unless the member
   ticks "replace" (the case where the app minted a fresh key after the loss).
5. Same key already present: reported as unchanged.

Every check runs before any write. After a restore the member restarts the app so the
node and memory store load the restored keys.

Tests: `src-tauri/src/recovery_kit_tests.rs` (round trip for both forms on an empty
keychain, wrong word, unknown word, wrong passphrase, tampering, foreign file, another
install's kit, replace, no-op, keychain down, owner-only file mode, no key bytes in
status) and `src/privacy/recovery.test.ts`.

## 2. Delete my local data

A dry run first, then a typed confirmation. Rust builds both from its own path API and
never deletes a path the webview sends.

- **Folders:** the app data folder, plus the config, local data, cache and log folders
  when they differ (macOS also `~/Library/WebKit/ai.citrate.core`). A folder counts only
  if its last path component is the bundle id `ai.citrate.core`; anything else is skipped
  with a note. Each top-level item is listed with its size. Symlinks are removed as links,
  never followed.
- **Keychain:** the app's own entries on `ai.citrate.core` (node storage, memory store,
  both messaging keys, AI provider keys including a custom default provider id) and, only
  when the member includes the wallet, the four `ai.citrate.core.custody` vault entries.
  The `custody-*` accounts on the legacy `ai.citrate.core` service belong to another
  Citrate app and are never listed or touched.
- **Defaults:** the wallet vault (`custody.enc` and its keychain entries) is kept. Ticking
  "also delete my wallet" requires a second phrase, `delete my wallet`. "Keep downloaded
  models" keeps the `models` folder.
- **Order:** the webview clears its own local and session storage; Rust checks the
  phrases again, stops every sidecar, rebuilds the plan, deletes it, reports anything it
  could not delete, and exits the process 2.5 s later without running the normal exit
  hooks (so the settings store cannot write a fresh file back).
- **Limits, honestly:** the system webview may recreate an empty storage folder when it
  next starts; removing the app itself is the member's step (Trash on macOS, the system
  uninstaller elsewhere).

Tests: `src-tauri/src/local_data_tests.rs` and `src/privacy/localData.test.ts`.

## 3. Telemetry consent

Off by default (`telemetry: false` in `freshState` and the bridge defaults). Turning it
on goes through a consent screen that lists exactly the fields a report carries, what is
never sent, and the endpoint. The list is
[`src/privacy/telemetry-fields.json`](../src/privacy/telemetry-fields.json); a Rust test
pins it to the fields `DiagnosticBundle` actually serializes and to the pinned ingest
URL. Even when on, nothing is sent in the background: each report is prepared, reviewed
and sent by hand (the existing ConsentGate flow in `DiagnosticReport`). The ingest
service is not live yet, so a send fails with an honest error.

## 4. Default budget values (pending owner sign-off)

[`src/privacy/budget-defaults.json`](../src/privacy/budget-defaults.json), status
`placeholder-pending-owner-sign-off`. These are conservative starting values for budget
grant cards. No budget exists until the member grants one through an HIC-1 ceremony
(ADR-2026-09-30-rule3-budgetable-signatures), so the file changes nothing on its own, and
x402 stays inert until a payment token exists on chain.

| Budget | Placeholder | ADR ceiling |
|---|---|---|
| SIWE sign-ins per budget | 20 | 50 |
| SIWE budget lifetime | 7 days | 30 days |
| x402 single payment | 0.1 SALT | 1 SALT |
| x402 per recipient, rolling 24 h | 1 SALT | 10 SALT |
| x402 all recipients, rolling 24 h | 2 SALT | 20 SALT |
| x402 payments per budget | 20 | 200 |
| x402 authorization validity | 10 min | 10 min |
| x402 budget lifetime | 1 day | 7 days |
| Scheduled task runs per day | 4 | none set |
| Scheduled task minutes per run | 10 | none set |
| Scheduled task model tokens per run | 20,000 | none set |

Medusa call budgets are not repeated here; they live in
[`templates/medusa-budgets.json`](../templates/medusa-budgets.json) (Rule 9) and the
defaults file links to it. A Rust test fails if any value exceeds its ADR ceiling, if the
x402 caps do not nest, or if the pending-sign-off status is dropped.
