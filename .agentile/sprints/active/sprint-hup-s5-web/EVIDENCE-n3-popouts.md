---
created: 2026-10-01T09:10:00Z
branch: hup/n3-popouts-monitor
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S5 (S5.4) + HUP-S7 (S7.6)
---

# Evidence: HUP-S5.4 pop-out framework + HUP-S7.6 Activity monitor

Design and data sources: `docs/POPOUT_WINDOWS.md`. Base `origin/release/0.5.0-hermes-upskill` @ 525b9ca.

| Gate | Before | After | Command |
|---|---|---|---|
| vitest | 796 passed, 3 skipped (88 files) | 853 passed, 3 skipped (97 files) | `npx vitest run` |
| tsc | clean | clean | `npx tsc --noEmit` |
| cargo workspace | 745 passed (533 app + 212 kit), 6 ignored | 769 passed (557 app + 212 kit), 6 ignored | `cargo test --workspace` (sources touched; `Compiling citrate-core (…/n3-popouts/src-tauri)` confirmed) |
| clippy | n/a | clean for `citrate-core`, all targets | `rustup run 1.98.1 cargo clippy --no-deps -p citrate-core --all-targets -- -D warnings` |
| main-thread tripwire | pass | pass (2 new async commands via `off_main`) | in the workspace run |

## Red first

- TS: 7 new test files failed to resolve their modules before any implementation (recorded run).
- Rust: with the capability files present but `main-window.toml` holding one command and
  `default.json` not granting it, `the_main_capability…` and `every_registered_command…` failed;
  green after the full allowlist. The pure-function Rust tests were written in the same step as
  `popout.rs`, so their proof is the mutation run below rather than a red run.

## Mutation checks (each mutant applied alone, then restored)

Rust (`cargo test --lib popout`): main capability widened to `popout-*` (2 fail); popout capability
given the app commands (3 fail); caller guard removed (1); every kind `available` (2); title-bar
visibility threshold loosened (1); any http host allowed for navigation (2); size not clamped (1);
half-valid position kept (1). All killed.

TS: late tool-call guard in `sendChat` removed (1 fail); sidecar stop route not called (1); turns
not serialised (1); bridge accepts any type as stop (8); host ignores stop (1); gateway spend
reported as 0 (3); Stop never disabled (2); turn activity accepts late events (1); loop's
pre-request stop check removed (1); answer reveal ignores stop (1); pre-tool stop check removed (1).
All killed. One redundant check (after the model request, already covered by `untilStopped`) was
found unkillable and removed.

## Not proven

- No run in the packaged app and no click-through on a real machine.
- The ACL proof uses Tauri's resolver over the source files in a test; the running app's ACL comes
  from the same files through `tauri-build`, which validated them at build time.
