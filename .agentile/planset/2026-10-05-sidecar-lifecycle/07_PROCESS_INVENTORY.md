---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-1 draft)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core + citrate-agent-runtime
companions: 02_ARCHITECTURE.md, 05_SPRINTS_AND_WPS.md
---

# Process Inventory and Exit Paths

Sources: citrate-core `release/0.5.0-hermes-upskill` at `d16f194`; citrate-agent-runtime
`main` at `397a6b1`; citrate-chain `main` at `0aab474b` for node descendants only (not
checked against the exact chain rev the bundled node is built from); Tauri 2.11.5,
tauri-plugin-updater 2.10.1, tauri-plugin-process 2.3.1, tauri-plugin-single-instance 2.4.3.
Paths are relative to each repo root. SCL-S1.4 re-grounds this table before dispatch.

This inventory describes ownership structure. Where a gap has security impact, the specific
behavior is tracked privately (#298) and the row says only which SCL WP owns it.

## Counts

| Metric | Count |
|---|---|
| Distinct process kinds the app can start, directly or through descendants | **34** |
| Supervised (`kit/src/supervisor.rs`) | **9** (8 bundled binaries; `llama-server` runs twice) |
| Bespoke, started by core | **12** |
| Bespoke, started by the Hermes sidecar | **10** |
| Bespoke, started by the node | **3** |
| Kinds that start descendants of their own | **10** |
| Kinds not reliably stopped on every exit path today | **16** (details per kind in the private research record) |

## A. Supervised (lifecycle cell)

| # | Process | Spawn site | Manager | Probe | Descendants | SCL owner WP |
|---|---|---|---|---|---|---|
| S1 | `citrate` node | `src-tauri/src/node.rs` spec `:558`, start `:599-618` | `NodeState` | none | yes (A.3) | SCL-S6.1 |
| S2 | `node-agent` | `agent.rs:506`, `:531-551` | `AgentState` | none | none known | SCL-S6.2 |
| S3 | `mem-mcp` daemon | `memory.rs:631`, `:685-704` | `MemoryState` | none | none | SCL-S6.2 |
| S4 | `llama-server` chat | `serve.rs:365`, `:400-431` | `ServeState` | `GET /health` | none | SCL-S7.1 |
| S5 | `llama-server` embed (BGE) | `embed_serve.rs:232-247` | `EmbedServer` in the Hermes manager | `GET /health` | none | SCL-S7.1, S7.3 |
| S6 | `ipfs daemon` (kubo) | `ipfs.rs:97`, `:110-125` | `IpfsState` | none | none | SCL-S6.3 |
| S7 | `comms-member-daemon` | `comms.rs:321`, `:366-390` | static `MANAGER` | connect | none | SCL-S7.2 |
| S8 | `cluster-daemon` | `cluster.rs:216`, `:263-291` | static `MANAGER` | connect | none | SCL-S7.2 |
| S9 | `hermes` (agent-sidecar) | `hermes.rs:571`, `:665-701` | static `HERMES` | `GET /health` | yes (section C) | SCL-S7.3, S11 |

## B. Bespoke, started by core (ownership handle)

| # | Process | Spawn site | Deadline today | Descendants | SCL WP |
|---|---|---|---|---|---|
| B1 | `ipfs init` | `ipfs.rs:74-78` | none | no | SCL-S6.3 |
| B2 | `ipfs config` | `ipfs.rs:87-90` | none | no | SCL-S6.3 |
| B3 | `mem-mcp import-corpus` | `knowledge_import.rs:304-314` | first line only | no | SCL-S8.2 |
| B4 | `citrate-fork run` (fork dry-run) | `fork_dry_run.rs:205-252` | 120 s | possibly | SCL-S8.2 |
| B5 | `tailscale status` | `fleet_tailscale.rs:202-237` | 4 s | no | SCL-S8.2 |
| B6 | `sysctl` hardware probe (macOS) | `tier.rs:385-392` | none | no | SCL-S8.2 |
| B7 | `nvidia-smi` probe | `tier.rs:395-399` | none | no | SCL-S8.2 |
| B8 | `powershell` RAM probe (Windows) | `tier.rs:451` | none | no | SCL-S8.2 |
| B9 | `forge` standard-json for verify | `postdeploy.rs:532-538` | none | yes | SCL-S8.2 |
| B10 | `pgrep` (startup cleanup) | `lib.rs:203,221` | none | no | removed by SCL-S8.3 |
| B11 | `ps` (startup cleanup) | `lib.rs:232` | none | no | removed by SCL-S8.3 |
| B12 | `kill` (startup cleanup) | `lib.rs:250` | none | no | removed by SCL-S8.3 |

Not counted: the OS URL opener (fire and forget); the app binary re-run as MCP stdio helpers
(counted as C7, since Hermes starts them); test-only spawns.

## A.3 Bespoke, started by the node (contained by the node's owner)

| # | Process | Where (citrate-chain) | SCL WP |
|---|---|---|---|
| N1 | `solc --standard-json` (compile and verify RPC) | `core/api/src/server.rs` | SCL-S8.4 |
| N2 | `llama-cli --version` probe | `core/api/src/server.rs` | SCL-S8.4 |
| N3 | GGUF engine binary | `core/mcp/src/gguf_engine.rs` | SCL-S8.4 |

## C. Bespoke, started by the Hermes sidecar (runtime owner)

| # | Process | Spawn site (runtime) | Descendants | SCL WP |
|---|---|---|---|---|
| C1 | Toolchain worker (`hermes --worker toolchain`) | `agent-workers/src/lib.rs` | yes | SCL-S11.1 |
| C2 | forge / slither / aderyn / medusa runs | `agent-shell/src/lib.rs`, `sandbox.rs` | yes | SCL-S11.1 |
| C3 | `shell_run` commands (member-approved) | `agent-sidecar/src/shell_run.rs` via `agent-shell` | yes | SCL-S11.3 |
| C4 | Sandbox probe | `agent-shell/src/sandbox.rs` | no | SCL-S11.3 |
| C5 | Managed headless browser | `agent-browser/src/chromium.rs` | yes (helpers) | SCL-S0.3, S11.2 |
| C6 | Search service (SearXNG) | `agent-search/src/searxng.rs` | possibly (unverified) | SCL-S11.2 |
| C7 | MCP stdio servers (built-in and user-added, including launcher wrappers) | `agent-mcp-host/src/transport.rs` | wrappers: yes | SCL-S11.3 |
| C8 | MCP probe server | `agent-mcp-host/src/probe.rs` | no | SCL-S11.3 |
| C9 | GPU sampling (`ioreg`, `nvidia-smi`) | `agent-sidecar/src/resources.rs` | no | SCL-S11.3 |
| C10 | `git` (checkpoint store) | `agent-checkpoints/src/git.rs` | possibly (repo filters) | SCL-S11.3 |

Spawn no process: WASM capsules (in-process), model calls (HTTP clients only), component
installs (in-process).

## D. Exit paths

Core's only teardown hook today is the `.run` callback (`src-tauri/src/lib.rs:1029-1039`),
which calls `shutdown_all_sidecars` on `ExitRequested` and on `Exit`.

| Exit path | Mechanism today | App `RunEvent` teardown runs? | SCL route | Proof (S12.5 row) |
|---|---|---|---|---|
| Last window close | `ExitRequested` | yes | coordinator | test |
| Cmd+Q / menu Quit (macOS) | `Exit` only | yes | coordinator | test + native |
| Updater "Restart now" | plugin-process `restart` → `request_restart` | yes | native coordinated restart | test + native |
| Update install, macOS and Linux | bundle swapped in place; app keeps running until restart | no (no exit) | native update command; drain at restart; owners record exact paths | native |
| Update install, Windows | installer launched, then direct process exit | **no** | drain before installer (S0.2 now, S12.2 final) | Windows native |
| Critical update | automatic install from notes | as above per OS | same native command | test |
| Factory reset (`local_data_delete`) | stop sidecars, delete data, timed direct exit | no (explicit stop instead) | coordinator reset | test |
| `app.exit()` from JS | allowed by `process:default`, not called | yes | capability removed | capability test |
| `AppHandle::restart()` on the main thread | would skip events | no | not callable; coordinator only | code review + test |
| Hermes setting change | `restart_if_running` | n/a (owner restart) | Hermes owner restart with child drain | test |
| Second launch (single instance) | rejected before setup | n/a | unchanged, before admission | test |
| Panic, SIGTERM, SIGKILL, force quit, logout | none | no | not `Complete`; recorded cleanup at next launch (Unix), kill-on-job-close (Windows) | native |
| Webview reload | webview only | n/a | services keep running; observer re-snapshots | vitest |
