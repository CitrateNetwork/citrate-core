---
created: 2026-10-05T19:30:00Z
branch: scl/s0.5-s0.7-s1.6a
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: SCL-S0
---

# SCL-S0 evidence

Evidence for the SCL-S0 work packages, one section per WP or PR. Other lanes append their own
sections; nothing here is edited after it is recorded except to add results that were pending.

## Rule-2 record

| PR / branch | Base | `cargo test --workspace --locked` (passed / failed / ignored) | vitest |
|---|---|---|---|
| baseline | `release/0.5.0-hermes-upskill` `6ab289e` | 2314 / 0 / 18 | not touched |
| `scl/s0.5-s0.7-s1.6a` | `6ab289e` | 2323 / 0 / 18 (+9) | not touched (no TS change) |

Gates on `scl/s0.5-s0.7-s1.6a` (macOS arm64, 2026-10-05): `cargo fmt --all` clean;
`cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings` clean.

## S0.5: shutdown coverage

Tests: `src-tauri/src/shutdown_coverage_tests.rs` (real child processes, every pid reaped by a
drop guard on failure).

| Test | What it proves | Result |
|---|---|---|
| `quit_stops_the_embedding_server_together_with_hermes` | Starting Hermes starts the embedding server; the quit path's `HermesManager::stop()` leaves neither child pid alive and removes the embed key file | pass |
| `a_hermes_that_fails_to_start_leaves_no_embedding_server_running` | A Hermes start that fails after the embedding server came up stops the embedding server | **red first** (failed on the base code at line 150), pass after the fix |
| `the_quit_hook_stops_hermes_and_hermes_stop_stops_the_embedding_server` | `RunEvent::ExitRequested` / `Exit` -> `shutdown_all_sidecars` -> `hermes::shutdown()` -> `stop()` -> `embed.stop()`; only Hermes start starts the embedding server | pass |
| `live_a_hermes_worker_exits_when_its_stdin_closes` | A real `citrate-agent-sidecar --worker toolchain` answers `ping`, then exits 0 when its stdin closes | pass with `CITRATE_E2E_HERMES_BIN` set (skips without it) |
| `live_a_hermes_worker_exits_when_the_process_holding_its_stdin_is_killed` (Unix) | The process holding the worker's stdin is SIGKILLed (sidecar died without a clean stop); the worker reads EOF and exits 0 | pass with `CITRATE_E2E_HERMES_BIN` set |

Live run: sidecar built from `citrate-agent-runtime` `origin/main` `397a6b1` (debug,
`cargo build -p agent-sidecar --bin citrate-agent-sidecar --locked`), macOS arm64.

Gap found and fixed: `HermesManager::start` started the embedding server, then returned early if
the bearer could not be written or the supervisor could not start, leaving the embedding server
running with no Hermes until the app quit. It now stops the embedding server on that path.

Pending (native, per the WP's acceptance): process list after quit on macOS, Linux and Windows
packaged builds, recorded here.

## S0.7: Windows installer stops this installation's own sidecars

- `src-tauri/windows/installer-hooks.nsh`: `NSIS_HOOK_PREINSTALL` runs Tauri's own main-app check
  first, then `stop-own-sidecars.ps1` on `$INSTDIR` with the eight sidecar names.
- `src-tauri/windows/stop-own-sidecars.ps1`: stops only processes whose executable path equals
  `<InstallDir>\<name>.exe` (full path, case-insensitive); graceful step, then terminate; by pid.
- `src-tauri/tauri.bundle-windows.conf.json`: `bundle > windows > nsis > installerHooks`.
- Script-level test: `src-tauri/windows/tests/test-stop-own-sidecars.ps1` (direct call and through
  the hook compiled into `hook-harness.nsi`).
- Cross-platform tests: `src-tauri/src/windows_installer_hook_tests.rs` (hook list equals the
  bundle's `externalBin`, exact-path rule, no bare-name kill, main-app check before the stop).
- Local: `makensis 3.13` (macOS) compiles the harness with the hook (exit 0).

Pending: Windows team native run of a manual 0.4.2 to 0.5.0 install with the old app and Hermes
running (steps in the PR body, "Windows team"): task list before and after, no old sidecar left,
no other process signalled.

## S1.6a: minimal hosted Windows CI job

`.github/workflows/windows-installer-hook.yml` (`windows-latest`, path-filtered to the hook, its
script, the Windows bundle config and the workflow). First run: recorded below once it has run.

- First run, `37399094707` (head `e49be10`): failed. The NSIS hook path passed every check, but one
  test call passed a quoted argument ending in a single `\`, which Windows argv parsing reads as an
  escaped quote. That was a test-only defect, fixed in `a889d0c`.
- First green run: https://github.com/CitrateNetwork/citrate-core/actions/runs/37399211383
  (head `a889d0c`, 1m0s). 26 of 26 checks: direct script (exact paths stopped; same name
  elsewhere, sub-folder, prefix sibling and non-sidecar left running; case and trailing separator;
  no-op; refused names) and through the hook compiled by makensis into the harness.

<!-- merged from #256 (S0.6 + S8.5a) -->

# SCL-S0 evidence

Per-WP proof for [the sprint](SCOPE.md) (`g1-precut` and the other `release: v0.5.0` criteria in
the SCL [gates.yaml](../../../planset/2026-10-05-sidecar-lifecycle/gates.yaml)). Each WP records
the red run, the green run, the test counts, and the native runs (done or still owed). Private
WPs (S0.1, S0.3, S0.4) record their evidence through the private route, not here.

## Rule-2 baseline

Taken at `release/0.5.0-hermes-upskill` `6ab289e` (the successor of `d16f194` named in SCOPE),
macOS arm64, 2026-10-06.

| Suite | Command | Passed | Failed | Ignored / skipped |
|---|---|---|---|---|
| Rust workspace | `cargo test --workspace --locked` | 2314 | 0 | 18 |
| vitest | `npx vitest run` | 2428 (264 files) | 0 | 24 (2 files) |

## S0.6 + S8.5a (one lane: node startup and the #243 reset)

PR #256: "SCL-S0.6 + S8.5a: genesis-change startup barrier and DB-lock check", branch
`scl/s0.6-s8.5a`. Stories: US-0.4 AC1; US-7.3 AC1, AC2. Red-team: RT-11.

### What changed

- `src-tauri/src/node_holder.rs` (new): takes the chain database lock the way RocksDB does
  (`fcntl(F_SETLK)` write lock on `LOCK` on macOS and Linux, no-share open on Windows), names
  the holder (`F_GETLK` pid; path from `proc_pidpath` / `/proc/<pid>/exe`), and probes the
  node's TCP ports from the `node.toml` it is launched with (holder pid from `lsof` /
  `netstat -ano` when present). Looks only; never deletes or signals.
- `NodeManager::start`: waits on the startup barrier, then refuses with the named holder while
  the chain database lock or a node port is held (`NodeError::Blocked`, `NodeStatus.blocked`),
  then runs the genesis gate.
- `node_genesis::reconcile_genesis` (#243 reset): takes the chain database lock before deleting
  anything and holds it until the marker is written; a holder refuses the reset
  (`GenesisGateError::ChainDbHeld`) even when nothing answers on the local RPC. The RPC check
  stays. `LOCK` is deleted last; the set of deleted and kept files is unchanged (test
  `a_free_lock_proceeds_and_lock_is_deleted_last`).
- `src-tauri/src/startup_barrier.rs` (new): `StartupBarrier::run_cleanup` wraps the existing
  startup cleanup call in `lib.rs` `setup` (current signature, no dependency on its
  internals) and opens the barrier when it returns; a panicking cleanup leaves it closed.
- UI: `node_status.blocked` folds into `nodeBlocked`; the Node surface shows the holder and the
  action ("The node cannot start yet" + the message); the node shows as an error, not "off".
- Residual crash window: stated in the `node_genesis` module doc and the `node_holder` module
  doc (the last `LOCK` unlink until the marker write; advisory lock; Windows names no file
  holder; TCP only).

### Red (tests first, before the wiring)

`cargo test -p citrate-core --lib --locked -- node_holder node_genesis node::tests startup_barrier`
with the new modules present but `start`, the reset and `setup` not yet wired:

```
test node::tests::a_normal_start_is_refused_while_an_orphan_holds_the_db ... FAILED
test node::tests::node_spawn_is_admitted_only_after_the_startup_cleanup ... FAILED
test node::tests::a_start_is_refused_while_a_node_port_is_in_use_and_names_it ... FAILED
test node::tests::genesis_change_start_is_refused_while_an_orphan_holds_the_db_and_names_it ... FAILED
test node_genesis::tests::a_free_lock_proceeds_and_lock_is_deleted_last ... FAILED
test startup_barrier::tests::setup_runs_the_startup_cleanup_through_the_barrier ... FAILED
test node_genesis::tests::an_orphan_holding_the_db_lock_blocks_the_reset_even_when_rpc_is_silent ... FAILED
test result: FAILED. 53 passed; 7 failed; 2 ignored
```

vitest `src/shell/nodeBlocked.store.test.tsx`: 4 of 4 failed before the store and surface change.

### Green

Same filter: 60 passed, 0 failed, 2 ignored. vitest file: 4 passed.

Lock-holder fixture: a separate process (the test binary re-run on the ignored
`node_holder::tests::fixture_child_entry`) holds `LOCK` the RocksDB way, or listens on a
loopback port, and never answers RPC. Every fixture is killed and reaped on drop (also on test
failure) and exits on its own if the test process dies (stdin closes). After every run: zero
fixture processes left (`ps`).

### Counts after (Rule 2)

| Suite | Passed | Failed | Ignored / skipped | Change |
|---|---|---|---|---|
| Rust workspace (`cargo test --workspace --locked`) | 2333 | 0 | 19 | +19 passed, +1 ignored (the fixture entry) |
| vitest | 2432 (265 files) | 0 | 24 | +4 |

Gates: `cargo fmt --all -- --check` clean; `cargo +1.98.1 clippy --workspace --all-targets
--locked -- -D warnings` clean; `npm run typecheck` clean. `node_holder.rs` and its tests also
pass `cargo +1.98.1 clippy --all-targets -- -D warnings` for `x86_64-unknown-linux-gnu` and
`x86_64-pc-windows-msvc` (checked in a standalone crate holding the same two files; the full
app does not cross-compile here).

### Cross-check against a real RocksDB holder (macOS, 2026-10-06)

The installed 0.4.2 app's node binary (`citrate 0.4.0`) was started by hand on a scratch data
dir with ports 18545/18546/30399 and no bootnodes, and the module's `find_blocker` was run
against it from another process:

- database: `ChainDatabase`, pid = the node's pid, path
  `/Applications/Citrate Core.app/Contents/MacOS/citrate`, message "An older Citrate node is
  still running (process <pid>, /Applications/Citrate Core.app/Contents/MacOS/citrate) and holds
  the chain database in ...".
- ports (scratch dir without the lock): `Port` 18545, same pid and path.
- after the node exited: free.

### Native runs still owed (`g1-precut`, `g4-native-v050`)

- macOS: packaged 0.4.2 to 0.5.0 in-app update on the rerolled 40204 chain, with and without
  the old node left running; confirm the Node surface names it and that the reset runs once it
  is quit.
- Linux (DGX): manual 0.4.2 to 0.5.0 install; same two cases. First native run of the Linux
  `F_GETLK` and `/proc/<pid>/exe` paths (CI on ubuntu runs the unit tests).
- Windows (with the S0.7 run): manual install with the old app running. First run of the
  no-share `LOCK` open against the real node; Windows names a database holder as unidentified
  and a port holder through `netstat -ano`.

<!-- merged from #257 (S0.1) -->

## S0.1 (private WP, generic record)

Close-with-proof records per WP. Private WPs (S0.1, S0.3, S0.4) are recorded here in generic
terms only; specifics are on federation #298.

### S0.1 Rule-2 baseline

`cargo +1.98.1 test --workspace --locked` on `release/0.5.0-hermes-upskill` `6ab289e` (macOS
arm64): 2,314 passed, 0 failed, 18 ignored.

### S0.1: Startup cleanup matches exact owned executable paths only

- New module `src-tauri/src/owned_cleanup.rs`, tests in `owned_cleanup_tests.rs`, native
  fixture `src-tauri/tests/fixtures/cleanup_fixture.rs`. Owned paths come from the same
  resolvers the managers launch the sidecars with (`resolve_owned_sidecar` in `lib.rs`).
- Red: 12 new tests run against the previous matching rule behind the same seam; the three
  decoy fixtures (command-line mention, `argv[0]` naming our binary, owned binary under a live
  instance) failed; the positive control (a leftover at the exact owned path is stopped) passed.
- Green (macOS arm64): 12 of 12. Workspace after: 2,326 passed, 0 failed, 18 ignored (+12).
  The same module tests also pass 12 of 12 on Linux (aarch64 container, standalone harness).
- Gates: `cargo +1.98.1 fmt --all -- --check` clean; `cargo +1.98.1 clippy --workspace
  --all-targets --locked -- -D warnings` clean; `npm run typecheck` clean.
- Every fixture process is recorded and stopped by a guard; no fixture process remained after
  any run (process list checked).
- Open: native run on each OS's installed package (US-0.1 AC2), recorded on #298.

## v0.5.0-rc.1: Hermes and sidecar rebuild, pins, gates (A7, A8, B5, B7)

Recorded 2026-10-06 on the release Mac (macOS arm64), branch `release/v0.5.0-rc-pins` from
`release/0.5.0-hermes-upskill` `98c846a` (after #267, S7.5a). The only tracked changes are the
`runtime-deps` pins and this record; the gates below ran on that tree.

### A7: Hermes sidecar

`scripts/build-hermes.sh --target aarch64-apple-darwin` from citrate-agent-runtime `main`
`c2f394c973a617c8144fd0bca7821c13e882d365` (includes #71, the SCL-S0 child-process cleanup, and
#70). `strip -x` 30,299,088 to 26,156,536 bytes; links system libraries only.

### B5: macOS sidecars (aarch64-apple-darwin)

| Sidecar | Source | sha256 |
|---|---|---|
| citrate (node) | citrate-chain `2979a157` (staged earlier with `scripts/build-sidecar.sh`, `LZMA_API_STATIC=1`; md5 and consensus fingerprint `0x0daa463069843ea3884371baf903ddf7` per its provenance; not rebuilt) | `9ad56eda5af2dbdd8f1bef44a890f3adb23cdcfba4c75774bb72ea47763736e8` |
| hermes | citrate-agent-runtime `c2f394c` (`scripts/build-hermes.sh`) | `6baa8440196a68c9811b4fb7b151c247f6ab269d317e338411b3717d9ff4813f` |
| comms-member-daemon | citrate-comms `9cd12c4` (`scripts/build-comms-daemon.sh`) | `adeacdd2e335ec0bba8034a33af8f64fbcc78df9cfba335028799de457853367` |
| cluster-daemon | citrate-cluster `755cf21` (`scripts/build-cluster-daemon.sh`) | `7de3c5576c1d44828ed7152d704b7b5242ca59fe0aabfc06153ed7f61163e411` |
| mem-mcp | citrate-memories `0e9d488`, `--features rocksdb,transformer` (src-tauri/binaries/README.md); carries `import-corpus` and the `CITRATE_BGE_MODEL_DIR` / `CITRATE_MEM_EMBED` intake | `315984355eb9652118c457248d907661f877d8516f7fe0193f01dc85e29bf8e6` |
| node-agent | citrate-node-agent `1d8cb8e` (src-tauri/binaries/README.md) | `2435a484c54deb2d7c93180d3a8c1534cc68ec825b24719829139e1d3af177c7` |
| ipfs | Kubo 0.42.0 (upstream, `ipfs version` checked; unchanged from 0.4.x) | `f254fedf3766c25867d6e33dacea635505ddefe73606872efd9de9bf28884cbc` |
| llama-server | llama.cpp build staged for 0.4.x (upstream, links the bundled `llama/` runtime; unchanged) | `ff931bc315f7cea369ad7d0b25024311df8c3e3352c1273f17abf7953c8ea2d4` |

Every built sidecar used `--locked` (no lockfile in any source checkout changed) and links only
`/System` and `/usr/lib` (otool). citrate, mem-mcp, node-agent and llama-server (the four
`release.yml` stages) are uploaded to `runtime-deps` and pinned in `src-tauri/runtime-deps.sha256`;
the downloaded assets pass `scripts/ci/verify-runtime-deps.sh`.

Not pinned (A9, DGX): `knowledge-corpus.tar.gz` (not on `runtime-deps`), and the llama runtime
tarball, the Gemma GGUF and `bge-base-en-v1.5.tar.gz` (the last is uploaded but unpinned). The
release workflow fails closed until they are pinned. When the corpus lands, stage it against the
mem-mcp above with `scripts/stage-knowledge-corpus.mjs` before pinning.

### A8 / B7: gates on the release head plus the pins

- `cargo +1.98.1 fmt --all -- --check`: clean.
- `cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `cargo +1.98.1 test --workspace --locked`: 2,365 passed, 0 failed, 19 ignored (21 suites).
  Up from 2,362 recorded on #265 before S7.5a (Rule 2).
- `npm run typecheck`: clean.
- vitest: 2,446 passed, 24 skipped (267 files passed, 2 skipped).
- `scripts/ci/check-release-pins.sh`: OK.
