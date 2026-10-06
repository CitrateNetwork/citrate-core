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

- First run: pending.
