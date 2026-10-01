---
created: 2026-10-01T18:30:00Z
branch: hup/n4-components
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S5 (S5.5) + HUP-S6 (S6.1)
---

# Evidence: HUP-S5.5 signed component updater + HUP-S6.1 toolchain bundle definitions

Fan-out 4 lane `hup/n4-components`, based on `release/0.5.0-hermes-upskill` @ 8b88df5. Policy
and runbook: [`docs/COMPONENT_UPDATER.md`](../../../../docs/COMPONENT_UPDATER.md). Directory map:
[`components/README.md`](../../../../components/README.md).

## What landed

| Piece | Where |
|---|---|
| `citrate-components` crate: pinned key (empty slot), signed manifest, staging, verify-then-swap, rollback, recovery, safe unpacking, CVE SLA client policy, bundle checks, release CLI | `components/` (new workspace member) |
| Toolchain bundle definitions: solc 0.8.36, foundry 1.5.1, python 3.12.14 (for slither), slither 0.11.6, aderyn 0.6.8, medusa 1.5.1, node 24.21.0, per OS; OpenZeppelin, Solady, forge-std archives at the `templates/deps.lock.json` commits, with licence texts | `components/toolchain-bundle.json`, `components/locks/`, `components/licenses/` |
| App wiring: `components_status`, `components_update`, `components_rollback` (async, main-window ACL), bridge domain, Settings card | `src-tauri/src/components.rs`, `src/bridge/*/components.ts`, `src/components/ComponentUpdates.tsx` |
| TLA+ model of the install path | `src-tauri/formal/ComponentSwap.tla` (+ two configs, mutant runner) |

## Acceptance

| Criterion | Status | Proof |
|---|---|---|
| Component manifest with name, version, per-OS URL, sha256, size, signature | met | `components/src/manifest.rs`; `tests/manifest.rs` (12 tests, 24 field rules) |
| Signature verified against a pinned public key (minisign, Ed25519) | met | prehashed only, trusted-comment domains, key separation from the app-updater key; `tests/manifest.rs`, `tests/key.rs` |
| Production key slot empty; updater refuses everything until it is set | met | `PRODUCTION_COMPONENT_PUBKEY = None`; `the_production_slot_is_empty_and_refuses`; core `update_refuses_without_the_key_and_creates_nothing` (no network, no disk) |
| Tests use a key generated in the test | met | `tests/common/mod.rs` `TestKey::generate` |
| Staging dir, verify-then-swap atomically, rollback on failure | met | `tests/install.rs` (17 tests): hash, size, signature, comment, swapped bytes, health, fetch failures all leave the current version intact; explicit rollback; recovery after a crash |
| CVE SLA policy documented | met (values pending owner sign-off) | `docs/COMPONENT_UPDATER.md`; client side in `components/src/policy.rs`, `tests/policy.rs` |
| Bundle entries with real upstream URLs; macOS arm64 sha256 verified by download | met | every macOS arm64 artifact downloaded and hashed 2026-10-01, each matching the upstream published digest; every tool run once from the archive as unpacked by this crate (below) |
| Other OSes: URL recorded, hash "to be measured" | met | `status: to_be_measured`, no hash; `check_bundle` refuses a hash on an unmeasured entry |
| Vendored OZ + Solady pinned by commit with licences | met | archives at the deps-lock commits (not copied: `check-bundle` fails if a URL does not name the commit); licence texts from the pinned archives |
| No binaries committed | met | text only |
| Sign-off + update test on real hardware (WP proof) | **not met** | needs the @rule8 key ceremony and a published manifest |

## Real artifacts (macOS arm64, 2026-10-01)

Downloaded, hashed, unpacked with `citrate-components unpack` (the app's own unpacker), then run:

| Tool | sha256 (first 16) | Unpacked | Ran |
|---|---|---|---|
| solc 0.8.36 (universal) | d4abcf0b3e24b794 | 1 file | `Version: 0.8.36+commit.8a079791.Darwin.appleclang` |
| foundry 1.5.1 | b3bf1752be066e08 | 4 files | `forge Version: 1.5.1-v1.5.1` |
| aderyn 0.6.8 (tar.xz) | 624c6652bb9478b3 | 3 files, 1 dir | `aderyn 0.6.8` |
| medusa 1.5.1 | a8b38bbd07a60f51 | 1 file | `medusa version 1.5.1` |
| node 24.21.0 | bed7eea5325e1108 | 4797 files, 1088 dirs, 3 symlinks | `v24.21.0`, npm `11.19.0` (through an archive symlink) |
| python 3.12.14+20260929 | 1bb3e53d231ee2c8 | 1653 files, 10 symlinks | `Python 3.12.14` |
| slither 0.11.6 + 46 wheels | wheel 01f809198d3f171e | venv with `--no-index --require-hashes` | `0.11.6` |
| openzeppelin-contracts v5.7.0 | 8158f1a22b468a0b | 897 files | two fetches, same bytes |
| solady v0.1.26 | 36991c81bbebc515 | 404 files | two fetches, same bytes |
| forge-std v1.17.0 | 79b96423c021a859 | 72 files | two fetches, same bytes |

## Tests

| Suite | Before | After |
|---|---|---|
| `cargo test --workspace` (core repo) | 909 (computed: after minus the 70 added here; matches the fan-out 3 stack-top count) | 979 passed, 0 failed, 7 ignored |
| `citrate-components` | n/a | 64 |
| core `components::` | n/a | 6 |
| vitest | 1003 | 1010 passed, 9 skipped |
| `tsc --noEmit` | clean | clean |
| clippy `-D warnings` (`citrate-components`, `citrate-core`) | | clean |
| main-thread tripwire, app-command ACL, invoke secret scan | | pass |

Red step: the crate's integration tests were written first and failed to compile against an empty
crate (unresolved imports). The core wiring tests were written with the wrappers; the key-first
ordering was then mutation-checked (moving the key check after the store open fails
`update_refuses_without_the_key_and_creates_nothing`).

Mutation check (crate, by hand): 26 guard mutants, 26 killed after two tests were added for the
two symlink guards that first survived (each was masked by the other).

Formal: `scripts/run-tlc.sh ComponentSwap all`: 130 distinct states (depth 12) and 517 distinct
states (depth 14), no error. `ComponentSwap_mutants.py`: 9 of 9 killed.

## Not done

- The key ceremony, the first signed manifest and the CDN upload (owner, @rule8). Gate
  `g3-updater` stays false.
- Hashes for macos-x64 (except solc), linux-x64, linux-arm64 and windows-x64 (to be measured on
  those machines).
- The slither wheelhouse archive (`to_be_built`) and its locks for other platforms.
- zip unpacking (Windows foundry and node), part of the Windows toolchain spike.
- Licence review and source offer (`g3-licence`, separate WP).
- Replacing the hand-written aderyn and medusa fixtures in core and runtime with captured runs
  (retro A29): the tools now run from these archives, but the fixtures were not regenerated in
  this lane.
- Wiring the toolchain verifiers to the installed component paths (they still look for tools on
  PATH).
