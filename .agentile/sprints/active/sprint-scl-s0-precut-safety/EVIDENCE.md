---
created: 2026-10-06T01:35:00Z
branch: scl/s0.6-s8.5a (PR into release/0.5.0-hermes-upskill)
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: SCL-S0
companions: SCOPE.md
---

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

PR: "SCL-S0.6 + S8.5a: genesis-change startup barrier and DB-lock check", branch
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
