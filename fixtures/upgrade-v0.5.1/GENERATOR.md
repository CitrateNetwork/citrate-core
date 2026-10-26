---
created: 2026-10-23
branch: test/upgrade-051-052
author: Larry Klosowski (MAC lane; Claude Opus 5.5)
status: active
---

# 0.5.1 upgrade fixtures

These files are what a Citrate Core 0.5.1 install holds on disk. They were written by the v0.5.1
code itself (tag `v0.5.1`, commit b521d98e), not by hand. The upgrade tests load them with the
current build:

- `src/shell/upgradeV051.test.ts` (webview saved state),
- `kit/src/upgrade_v051_tests.rs` (wallet vault, app config),
- `src-tauri/src/upgrade_v051_tests.rs` (app data dir),
- `src-tauri/src/upgrade_updater_feed_tests.rs` (updater feed).

The inventory these tests prove is in the federation planset,
`.agentile/planset/2026-10-19-betanet/UPGRADE_051_052.md`.

## What each file is

| File | Written by (v0.5.1) |
|---|---|
| `webview/localStorage.citrate-core-proto-v2.member.json` | `Store.save()` (store.ts:2003, the `PERSIST_KEYS` filter) on `freshState("p1")` + `reconciledGrantPatch` for a settled 32,000 SALT grant, stage `done` |
| `webview/localStorage.citrate-core-proto-v2.onboarding-s6.json` | the same, stage `s6` |
| `app-data/custody.enc` + `custody.keyring.json` | `CustodyVault::new` + `ensure_auto_unlocked` + `wallet::import` (the public Hardhat test mnemonic) + `put("connection-github")`, through an in-memory `Keyring`; the keyring entries the vault created are in `custody.keyring.json` (base64), since the envelope opens only with them |
| `app-data/config.json` | `AppConfig` serialized the way tauri-plugin-store 2.4.3 saves it (`{"app": ...}`, `to_vec_pretty`) |
| `app-data/invites/pending.json`, `referral-log.json` | `PendingInvite` / `ReferralEvent` through `write_pending` / `write_secret_file` (invites.rs:151, :306) |
| `app-data/ipfs/pins.json` | `StorageManager::pin(cid, "local")` |
| `app-data/daemons.json` | `DaemonBook::save` + `store_book` |
| `app-data/models/*.status.json` | `ModelManager::verify` on a file matching its pin (the file is a few placeholder bytes, not a model) |
| `app-data/hermes/terminal-settings.json` | `hermes_terminal::save` (member turned terminal commands off) |
| `app-data/node/chain-genesis` + DB file names | `reconcile_genesis` on a fresh dir, then chain DB file names (placeholder bytes) |
| `updater/latest.v0.5.1.json` | the `latest.json` asset of the v0.5.1 GitHub release (byte-identical to the CDN copy on 2026-10-23) |

Every key, token and address here is test material: the mnemonic is the public Hardhat seed, the
invite key and token were minted for this fixture, and the connection token is a placeholder
string. None of it is used on the network.

## Regenerating

In a scratch checkout of `v0.5.1` (never commit there):

1. Copy `generator/webview_gen.test.ts.txt` to `src/zz_upgrade_fixture_gen.test.ts` and run
   `FIXTURE_OUT=<out> npx vitest run src/zz_upgrade_fixture_gen.test.ts`.
2. Copy `generator/kit_gen.rs.txt` to `kit/examples/zz_upgrade_fixtures.rs` and run
   `FIXTURE_OUT=<out> cargo run -p citrate-core-kit --example zz_upgrade_fixtures`.
3. Copy `generator/app_data_gen_tests.rs.txt` to `src-tauri/src/zz_upgrade_fixture_gen_tests.rs`,
   add `#[cfg(test)] mod zz_upgrade_fixture_gen_tests;` to `lib.rs` and a `#[cfg(test)]`
   `write_pending_for_fixture(path, v)` wrapper around `write_pending` to `invites.rs`, then run
   `FIXTURE_OUT=<out> cargo test -p citrate-core --lib zz_upgrade_fixture_gen -- --ignored`.

The invite key, token and vault keys are random on each run, so a regenerated set differs byte
for byte; the tests read values from the fixtures rather than hard-coding them.
