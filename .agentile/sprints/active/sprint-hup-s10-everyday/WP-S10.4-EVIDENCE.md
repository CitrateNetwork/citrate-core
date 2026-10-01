---
created: 2026-10-01T00:00:00Z
branch: hup/n3-journal-export
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S10
wp: HUP-S10.4
---

# HUP-S10.4 evidence: journal daily entry + encrypted export

Spec: planset `04_FEATURES_BDD.md` US-10.4. Design and file format:
[`docs/JOURNAL_EXPORT_FORMAT.md`](../../../../docs/JOURNAL_EXPORT_FORMAT.md).

## Red to green

| Suite | Red observed | Green |
|---|---|---|
| `src/journal/dailyEntry.test.ts` (11) | module missing | 11 pass |
| `src/journal/bundle.test.ts` (8) | module missing | 8 pass |
| `src/journal/encryptedExport.test.ts` (12) | module missing | 12 pass |
| `src/journal/journalVaultPanel.test.tsx` (5) | written with the panel, before wiring; see note | 5 pass |
| `src/surfaces/journalDailyExport.test.tsx` (5) | 5 failed against the unchanged surface | 5 pass |
| `kit/src/custody_tests.rs` `passphrase_kdf_*` (3) | E0425: `derive_passphrase_key` / params missing | 3 pass |
| `src-tauri/src/journal_export_tests.rs` (12) | E0425 on every function | 12 pass |

Note: the panel suite was written in the same step as the panel, so its red run was
not separately recorded.

Mutation checks on the Rust guards (break, see a test fail, restore):

| Mutant | Killed by |
|---|---|
| header KDF parameter check disabled | `foreign_truncated_and_unsupported_files_are_named` |
| symlink refusal disabled | `export_refuses_to_write_through_a_symlink` |

## Gates

| Gate | Result |
|---|---|
| `npx tsc --noEmit` | clean |
| `npx vitest run` | 837 passed, 3 skipped (baseline 796 passed) |
| `cargo test --workspace` (from `src-tauri`) | 760 passed, 0 failed, 6 ignored (baseline 745 passed) |
| `cargo clippy --no-deps -p citrate-core -p citrate-core-kit --all-targets -D warnings` (1.98.1) | clean |
| `main_thread_tripwire` | pass: both new commands are `async` over `blocking::off_main` |
| `invoke_secret_scan_tests` | pass: the commands return a byte count and the bundle text, no key material |
| rustfmt (new files) | clean |

## Owner decisions taken conservatively

1. AEAD is AES-256-GCM (the vault's construction), not XChaCha20-Poly1305, which is
   not a dependency. A fresh random salt per file makes every key single-use, so the
   96-bit random nonce is safe.
2. Export passphrase minimum is 12 characters. A passphrase is the only option for now;
   a wallet-derived key (US-10.4 AC2 alternative) is not built, because it would need a
   ceremony-backed derivation.
3. Import never overwrites: a differing page with the same id comes in as
   "<title> (imported)". Imported pages come in unpinned.
4. The file extension is `.citrate-journal`, and the export command only writes paths
   with that extension that are not symlinks.
5. The day is the UTC date, as `journal_append` already uses.

## Not done

- The summary reads the approved `@agent` bullets and today's wallet activity only.
  The sidecar event log, metering records and the memory `personal` tenant (US-10.4 AC1)
  are not read yet.
- No automatic daily writing. The member presses **Hermes summary**.
- Key recovery for the journal passphrase is S10.5. A lost passphrase means the file
  cannot be opened, and the UI says so.
- Hands-on QA in the packaged app (save/open dialogs on macOS, Linux, Windows) is not done.

## Journal

The useful surprise was the main-thread tripwire. A private helper named `derive`
made `social_verify_forget` fail, because the scanner resolves calls by name and read
`#[derive(Serialize)]` below that command as a call to a blocking `derive`. Renaming
the helper fixed it. The lesson for future lanes: give private helpers in modules that
reach Argon2 or file I/O distinct names, never a word that shows up in attributes.
The second surprise was that the one-day-per-entry rule was already implied by
`journal_append`'s `d-<date>` id. Reusing that id meant the agent's approved writes and
the member's entry are the same page, so the summary can read them without a second store.
