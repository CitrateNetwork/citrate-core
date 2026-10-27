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
- `src-tauri/src/upgrade_updater_feed_tests.rs` (updater feed),
- `src-tauri/src/upgrade_xgenesis_tests.rs` (the same install on r1005, started on a new genesis).

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

## Cross-genesis fixture (`node-r1005/`, `node-r1005.keyring.json`, `golden/`)

`upgrade_xgenesis_tests` replaces `app-data/node` with `node-r1005/`: a 0.5.1 node data dir on the
r1005 testnet, with every kind of entry the K6 migration sorts (node_genesis.rs module table). It is
written by hand, not by v0.5.1, and every key in it is synthetic:

| File | What it stands for |
|---|---|
| `chain-genesis` | the 0.5.1 marker (`"<genesis>\n"`, node_genesis.rs in v0.5.1) naming r1005 `0x1dcfc490…75f1` |
| `node.toml` | the v0.5.1 `member-node.toml` as `ensure_node_config` rendered it (data dir of a macOS install), with one member edit (`max_peers = 24`) so the set-aside copy is shown to keep hand edits |
| `CURRENT`, `IDENTITY`, `LOCK`, `LOG`, `LOG.old.*`, `MANIFEST-*`, `OPTIONS-*`, `*.log`, `*.sst` | RocksDB file names with placeholder bytes |
| `proposer.key`, `noise.key`, `peer.id`, `encryption.meta`, `crash-records.jsonl` | node identity and core's crash log, ASCII placeholder bytes of the right length, not keys |
| `../node-r1005.keyring.json` | the 0.5.1 node storage key slot (service `ai.citrate.core`, account `node-storage-key`), 32 placeholder bytes |

`golden/xgenesis-post-upgrade.manifest` lists every file in the app data dir and every keychain slot
after the switch, with its sha256 (`present` where the bytes depend on the run: the marker carries
a random migration nonce, the new `node.toml` the absolute data dir). When the switch changes on
purpose (for example K6b archiving the chain database instead of deleting it), regenerate it with
`CITRATE_WRITE_XGENESIS_GOLDEN=1 cargo test -p citrate-core --lib upgrade_xgenesis` and review the
diff.

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
