---
created: 2026-07-11
branch: feat/core-s0-scaffold
author: Claude Fable 5 (CORE-S0 scaffold agent), directed by @SaulBuilds
status: active
sprint: CORE-S0
planset: citrate-federation/.agentile/planset/2026-07-11-citrate-core/05_SPRINTS_AND_WPS.md
---

# Sprint CORE-S0 — Repo, scaffold, and skeleton shell

Protocol: test count monotone (Rule 2); every WP names its acceptance data
source (Rule 11); no mocked data anywhere (Rule 1).

## WP checklist

- [ ] **WP 0.1 — Repo scaffold** (`.agentile/`, CLAUDE.md, Tauri 2.x workspace, CI fmt/clippy/test/audit `--locked`)
  - Acceptance: CI green on the shell; `cargo test --workspace --locked` baseline recorded.
  - Status: scaffold landed in this PR; CI status pending first run. Baseline recorded below.
- [ ] **WP 0.2 — Federation registration** (manifest entry + `[[drift]]` for citrate-chain wallet-core pin)
  - Status: NOT STARTED. Deliberately deferred — the manifest `rev` needs the merged scaffold SHA, so this is a follow-up PR in citrate-federation after this PR merges.
- [ ] **WP 0.3 — Shell app** (sidebar nav, routing, empty pages, `citrate-core://` deep-link scheme; wagmi + viem scaffold with ceremony connector stub, D-13)
  - Status: PARTIAL. Landed: wagmi + viem scaffold (`src/chain.ts`, `src/wagmi.ts`) and the live block-height read against 40204 (`src/App.tsx`, acceptance: viem read renders live height). Remaining: sidebar nav, routing, empty pages, deep-link scheme, WebDriver e2e run log. No ceremony connector was stubbed as a fake signer — connectors array is deliberately empty until CORE-S2 (Rule 1 / I-2).
- [ ] **WP 0.4 — SidecarSupervisor** (spawn/monitor/restart/backoff + crash records)
  - Status: NOT STARTED.

## Test-count baseline (Rule 2 ratchet)

| Date | Suite | Count | Command |
|---|---|---|---|
| 2026-07-11 | Rust | **2 passed, 0 failed** | `cargo test --workspace --locked` |
| 2026-07-11 | Frontend | 0 (no test runner yet) | — |

## Daily updates

### Day 1 — 2026-07-11 (scaffold)

- Created `CitrateNetwork/citrate-core` (private). Initial commit on `main` = README only; all other work on `feat/core-s0-scaffold`.
- Scaffolded Tauri 2.x via `create-tauri-app` (react-ts template), rebranded to `citrate-core` / `ai.citrate.core` / "Citrate Core", wrapped `src-tauri` in a root Cargo workspace.
- Frontend: wagmi + viem + @tanstack/react-query; `src/chain.ts` defines chain 40204 (`https://rpc.citrate.ai`); default page renders live block height via `useBlockNumber({ watch: true })`. Loading and error states are honest; no mocks.
- Rust: template `greet` demo command removed; 2 real config-integrity tests added (tauri.conf identity + version drift).
- `.agentile/` scaffold, CLAUDE.md, CI workflow, `services/membership/` reservation README written.
- Verification (local, macOS arm64):
  - `cargo test --workspace --locked` — **2 passed, 0 failed** (baseline).
  - `cargo fmt --all -- --check` — clean. `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
  - `npm run typecheck` — clean. `npm run build` — succeeds (vite 7.3.6, 727 modules, 288.82 kB js).
  - Live RPC proof: `eth_chainId` → `0x9d0c` (40204), `eth_blockNumber` → `0x46122` (287010) from https://rpc.citrate.ai.
  - Native Tauri bundle (`npm run tauri build`) NOT run in S0 — the shell compiles (clippy builds all targets); installer bundling is CORE-S7 scope.
- Known issue (honest): `npm audit` reports **21 vulnerabilities (20 moderate, 1 high)**, all transitive via wagmi → `@wagmi/connectors` → walletconnect/@reown packages, which this app does not use (`connectors: []`). `npm audit fix` fails on an upstream peer-dependency conflict; not forcing a broken resolution. Revisit when the CORE-S2 ceremony connector work touches wagmi versions.
