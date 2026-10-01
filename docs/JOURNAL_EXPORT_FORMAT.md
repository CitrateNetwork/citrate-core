---
created: 2026-10-01T00:00:00Z
branch: hup/n3-journal-export
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S10.4
---

# Journal daily entry and encrypted export (HUP-S10.4)

US-10.4: each day the member has one journal entry, Hermes can add a summary of
what it did, and the member can export the journal privately. Nothing leaves the
machine unless the member exports it.

## Daily entry

- One entry per day. Its page id is `d-YYYY-MM-DD` (UTC date), the same id the
  `journal_append` tool writes to. The **Today** button opens it, creating it once.
  Code: `src/journal/dailyEntry.ts` (`ensureDailyEntry`).
- The entry is an ordinary journal page, edited with the existing editor.

## "What Hermes did today"

The **Hermes summary** button (shown on today's entry) writes an editable block:

```
What Hermes did today (from local records on this device):
  Journal note you approved: <text of each @agent bullet in today's entry>
  Wallet activity: <kind> · <amount> (confirmed | failed | pending)
```

Sources, all already on this device: the `@agent` bullets the member approved
into today's entry, and the wallet activity rows timestamped today. Demo persona
seed rows (`id` starting `seed`) are skipped. With no records the block says
"No Hermes activity is recorded on this device today." Re-running replaces the
block instead of stacking a second one.

Not included yet: the sidecar session event log, metering records and the memory
`personal` tenant named in US-10.4 AC1. They are not readable from the webview
offline today; they belong in a follow-up once the sidecar exposes a dated,
local read.

## Encrypted export file (`.citrate-journal`, format v1)

The webview builds a JSON bundle (`src/journal/bundle.ts`, format
`citrate-journal-bundle`, version 1) and hands it in memory to the Rust command
`journal_export_encrypted` (`src-tauri/src/journal_export.rs`), which seals it and
writes only ciphertext to the path the member chose in the save dialog.

Crypto is reused from the custody vault, not new:

| Part | Choice | Source |
|---|---|---|
| KDF | Argon2id v0x13, m = 65536 KiB, t = 3, p = 1, 32-byte key | `citrate_core_kit::custody::derive_passphrase_key` (the vault's own D-A2-1 derivation) |
| AEAD | AES-256-GCM, 12-byte random nonce, 16-byte tag | the `aes-gcm` construction the vault seals slots with |
| Salt | 16 random bytes per export | `PASSPHRASE_SALT_LEN` |

Layout (integers big-endian). The 53-byte header is the AEAD associated data:

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | magic `CITJRNL\0` |
| 8 | 1 | format version `1` |
| 9 | 1 | KDF id `1` (Argon2id v0x13) |
| 10 | 4 / 4 / 4 | m_cost KiB, t_cost, p_cost |
| 22 | 1 | salt length `16` |
| 23 | 16 | salt |
| 39 | 1 | AEAD id `1` (AES-256-GCM) |
| 40 | 1 | nonce length `12` |
| 41 | 12 | nonce |
| 53 | rest | ciphertext with tag |

Reading rules: the reader checks every fixed header field against what this
build writes before any key derivation, so a crafted file cannot make the app do
more KDF work than an ordinary unlock. A wrong passphrase and a changed or damaged
file give the same message. A newer format version is named as such.

Write rules: the passphrase must be at least 12 characters (checked in the
webview and again in Rust). The path must be absolute, end in `.citrate-journal`,
and not be a symlink. The file is created owner-only (`0600`) on unix. No
plaintext temp file is ever written.

## Import

The member picks a `.citrate-journal` file and enters its passphrase;
`journal_import_encrypted` opens it (files over 32 MiB are refused before
reading) and returns the bundle text. The webview validates it strictly and
merges without overwriting: new pages are added, identical pages are skipped, and a
page whose id exists with different content is added as a separate page titled
`<title> (imported)`, so one daily entry per day still holds. Imported pages arrive
unpinned.

## Web preview

The web preview has no sealer. Export and import there say they need the desktop
app.
