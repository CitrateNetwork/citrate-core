---
created: 2026-07-12
branch: docs/sprint-a1-bridge
author: Claude Fable 5, directed by @SaulBuilds
status: completed
sprint: CORE-A1
planset: citrate-federation/.agentile/planset/2026-07-12-core-beta-wiring/00_STATE_AND_PLAN.md (Phase A1)
depends_on: citrate-core#2 (the 1:1 front end) merged first
---

# Sprint CORE-A1 — Frontend↔backend bridge (the seam boundary)

Protocol: test count monotone (Rule 2); every WP names its acceptance data source
(Rule 11); no mocked data presented as live (Rule 1); all work on a feature branch + PR,
never main; the owner merges.

## Why this is the first wiring sprint

The front end (citrate-core#2) is a faithful 1:1 prototype driven entirely by an
in-memory sim `Store`; the Tauri Rust backend is scaffold-only; the UI makes zero
`invoke` calls. Beta means flipping every surface's data seam from sim to live. This
sprint builds the boundary that makes that possible **without ever touching a surface
again** — a typed bridge between the surfaces and the backend, runtime-selected between
a `sim` implementation (dev/web, the current prototype) and a `tauri` implementation
(the packaged app). Every later phase then changes *one bridge domain's* implementation,
and the surface above it does not change.

**A1 needs none of the three critical-path gates** (authority redeploy, contract
sign-off/counsel, core-membership). It is pure client architecture plus one genuinely
real domain, so it can start immediately after #2 merges.

## Design (the contract this sprint establishes)

- `src/bridge/` — one typed module per domain: `auth`, `wallet`, `node`, `memory`,
  `chat`, `membership`, `commissary`, `comms`, `config`. Each exports async functions
  the surfaces need (the shapes the current `store.*` methods already imply).
- **Runtime selection.** `bridge.mode` is `tauri` when running in the packaged app
  (detected via `@tauri-apps/api`), else `sim`. Selection happens once, at the boundary.
- **`sim` adapter = the current prototype behavior**, moved behind the bridge and clearly
  namespaced as a dev shim. `npm run dev` renders the whole app exactly as today.
- **`tauri` adapter** invokes Rust commands. For domains not yet wired, the command
  returns an honest `Unavailable` result (NOT fabricated data); the bridge maps that to
  the UI's existing honest "coming"/seam state. This is the Rule-1 line: in the real
  desktop app, an unwired domain says so; it never shows sim data dressed as live.
- **`store.*` delegates to the bridge.** Surfaces keep calling `store`/reading `s`; the
  store becomes a thin state cache over the bridge instead of a sim engine. Surfaces do
  not change (the 1:1 UI is preserved).
- **The packaged build has no sim path.** The sim adapter is guarded so it cannot execute
  in a Tauri build (build-time flag or runtime assertion). The seed module and the
  demo/persona panel remain for web-dev only.

## WP checklist

- [x] **A1.1 — Bridge contract + runtime selection.** `src/bridge/index.ts` + per-domain
  interface files; `bridge.mode` detector.
  - Acceptance: `bridge.mode === "sim"` under `npm run dev` and `=== "tauri"` in a Tauri
    dev run (data source: `@tauri-apps/api` presence check); `npm run typecheck` green.
  - DONE: `src/bridge/{index,mode,types,domains}.ts`; mode via `isTauri()` +
    `window.__TAURI_INTERNALS__` fallback (`src/bridge/mode.ts`). typecheck green.
- [x] **A1.2 — Sim adapter (dev shim) + store delegation.** Move the current `Store` sim
  behavior into `src/bridge/sim/`; repoint `store.*` at the bridge; guard the sim path
  out of packaged builds.
  - Acceptance: the full UI walks identically under `npm run dev` (visual parity with #2,
    verified by a dev run — the demo persona walk still works); a packaged/Tauri build
    contains no reachable sim code path (data source: a build-flag/assertion test).
  - DONE: `src/bridge/sim/index.ts` DELEGATES to the live Store (does not rewrite it) —
    Store binds itself via `bindSimHost()` in its constructor (no import cycle). Guard:
    `assertSimAllowed()` throws if reached when `BRIDGE_MODE==="tauri"` (tested). UI
    unchanged (build + full module graph transform green; surfaces untouched).
- [x] **A1.3 — Tauri adapter + Rust command contract.** Register every domain's commands
  in `src-tauri` `invoke_handler`; unwired domains return an honest `Unavailable` error.
  - Acceptance: in a Tauri dev run, an unwired domain (e.g. `wallet`) surfaces the UI's
    honest unavailable/"coming" state, not sim data (data source: the Rust command's
    `Unavailable` return; Rule 1). `cargo test --workspace --locked` count ≥ baseline.
  - DONE: `src/bridge/tauri/index.ts` + `src-tauri/src/seam.rs` (13 seam commands return
    `Err("unavailable: …")`); TS maps to `Unavailable` class. All 15 commands registered
    in `lib.rs invoke_handler`. cargo test 7 ≥ baseline 2.
- [x] **A1.4 — Prove the round-trip: wire the `config` domain for real.** Implement
  `config` end-to-end through Tauri — persisted app config (network, data dir, ports,
  autolock, update channel, telemetry) via a real Tauri store, and OS-keyring status
  read. Settings' Node-configuration + App sections read/write real config through the
  bridge in a Tauri build.
  - Acceptance: a value set in Settings persists across an app restart, read back from
    the real Tauri config store (data source: Tauri Store plugin file on disk / OS
    keyring), verified in a Tauri dev run. This is the proof the whole bridge→invoke→Rust
    round-trip works; every later domain follows this exact shape.
  - DONE: `src-tauri/src/config.rs` — `config_read/config_write/config_keyring_status`
    persist to a real `tauri-plugin-store` on-disk `config.json`; keyring status probes
    the `keyring` crate. Settings Node-config + App sections write via `bridge.config`
    (`writeConfig()`); App boot hydrates from `config.read()` on the Tauri path.
    Round-trip proven by (a) vitest read-your-write with mocked invoke simulating a
    restart, and (b) Rust `config::tests` (merge + camelCase wire contract). HONEST GAP:
    interactive `npm run tauri dev` not run — no display available headless (see Day 1).
- [x] **A1.5 — Frontend test ratchet.** Add vitest; unit-test bridge selection, the sim
  adapter contract, and the `config` domain round-trip (mocked `invoke` at the boundary).
  - Acceptance: `npm run test` green; frontend baseline recorded below (currently 0).
  - DONE: vitest + jsdom added; `npm run test` = 13 passed across
    `mode.test.ts` / `sim.test.ts` / `tauri.test.ts`.

## Test-count baseline (Rule 2 ratchet)

| Date | Suite | Count | Command |
|---|---|---|---|
| 2026-07-11 | Rust | 2 passed, 0 failed | `cargo test --workspace --locked` |
| 2026-07-11 | Frontend | 0 (no runner) | — (vitest added in A1.5) |
| 2026-07-12 | Rust | 7 passed, 0 failed | `cargo test --workspace --locked` |
| 2026-07-12 | Frontend | 13 passed, 0 failed | `npm run test` (vitest) |

## Definition of done

- The bridge exists; all nine domains have a typed contract.
- The web-dev prototype is visually unchanged (1:1 preserved) — sim behind the bridge.
- The `config` domain is genuinely live through real Tauri invoke (the proof); every
  other domain honestly returns `Unavailable` on the Tauri path (no sim-as-live).
- No reachable sim path in a packaged build.
- `typecheck` + `build` + `cargo test` + `npm run test` all green; Rule-2 counts recorded.
- No dependency on any of the three beta gates was introduced.

## Out of scope (later phases)

- Any real `auth`/`wallet`/`node`/`memory`/`chat`/`membership`/`commissary`/`comms`
  implementation — those are Phases A3–E, each flipping its bridge domain sim→live.
- The SidecarSupervisor, keyring custody service, OIDC loopback — Phase A2/A3.
- Removing the demo panel / seed module for beta — Phase E4 (they stay for web-dev now).

## Daily updates

### Day 0 — 2026-07-12 (scoped)
- Sprint defined from the CORE-BETA Phase A1 scope (federation planset 2026-07-12,
  PR #164). Awaiting citrate-core#2 (the 1:1 front end) to merge, then A1.1 starts.

### Day 1 — 2026-07-12 (built A1.1–A1.5)
- Branch `feat/core-a1-bridge` off main. Implemented the full bridge seam.
- **Contract (9 domains):** `src/bridge/domains.ts` — `config` (real) plus `auth`,
  `wallet`, `node`, `memory`, `chat`, `membership`, `commissary`, `comms` (typed seams).
  `BridgeContract` carries `.mode`.
- **Mode:** `src/bridge/mode.ts` — `BRIDGE_MODE` from `@tauri-apps/api/core` `isTauri()`,
  fallback `window.__TAURI_INTERNALS__`. Sim path guarded by `assertSimAllowed()` which
  throws in a Tauri build (packaged build has no reachable sim path).
- **Sim delegates, not rewrites:** the Store binds itself via `bindSimHost()` in its
  constructor; the sim adapter reads/patches the live prototype AppState. Surfaces
  untouched → 1:1 UI preserved. (Interactive dev walk not screenshotted; parity argued
  from: zero surface changes + `writeConfig` optimistically patches the same AppState +
  full `vite build` module-graph transform green.)
- **Tauri adapter + Rust:** `src-tauri/src/{config,seam}.rs`; 15 commands registered in
  `lib.rs`. Seam domains return `Err("unavailable: …")` → TS `Unavailable` (Rule 1).
- **A1.4 config round-trip (real):** `tauri-plugin-store` on-disk `config.json` +
  `keyring` crate status. Settings Node-config + App write through the bridge; App boot
  hydrates on the Tauri path. Proven headless via vitest read-your-write (mocked invoke,
  simulated restart) + Rust merge/serde tests.
- **Gates:** typecheck clean; `vite build` OK; vitest 13/13; `cargo test --locked` 7/7
  (≥ baseline 2); `cargo fmt --check` clean; `cargo clippy -D warnings` clean;
  `cargo build` OK (plugins + keyring compile, commands registered).
- **HONEST GAP:** a full interactive `npm run tauri dev` was NOT run — it needs a display
  and does not launch headless here. A1.4 is verified via the unit-test round-trip and the
  Rust config command tests, not an eyes-on desktop restart. Recommend a manual Tauri dev
  restart-and-read pass before relying on the on-disk persistence in the packaged app.

### Closed — 2026-07-12
- All 5 WPs done; merged in citrate-core#4. Gates verified on the branch and by
  the orchestrator: typecheck clean, `vitest run` 13 passed, `vite build` green,
  `cargo test --workspace --locked` 7 passed (baseline 2, Rule-2 monotone), fmt +
  clippy `-D warnings` clean, web prototype still serves 1:1 under `npm run dev`.
- Follow-up folded into the close: CI did not run vitest, so the new frontend
  ratchet was unenforced — added `npm run test` to the node CI job (this branch).
- Honest gap carried forward: an interactive `npm run tauri dev` restart-and-read
  of the config round-trip was not performed headless; proven by the vitest
  read-your-write + the Rust `config::tests` instead. Recommend a manual desktop
  pass before relying on packaged on-disk persistence.
- Status: **completed**. Next: A2 (keyring custody service) → A3 (OIDC loopback,
  which needs the authority redeploy).
