---
title: "citrate-core Finish Plan — state of the app + what it takes to ship"
created: 2026-07-18
branch: docs/core-finish-plan-2026-07-18
author: Claude (Opus 4.8, 1M) for SaulBuilds
status: handoff — for the app-surface engineers picking up citrate-core
related:
  - docs/CITRATE_CORE_MASTER_CHECKLIST.md
  - docs/CITRATE_CORE_DGX_WORK_ORDER.md
---

# citrate-core — Finish Plan (2026-07-18)

Ball-pass to the app-surface engineers. This is the code-verified state of the app and
the concrete work left to ship the master-checklist end state (a real validator node
that signs users in, procures the local model, pins storage, mints SBT art, and hands
users to federation apps already signed in). Verified against the actual code + git, not
just docs (C-1). `file:line` cites the exact code to change.

## TL;DR

- **~55–60% of the named end-state is done.** The **money/identity/node/wallet spine is
  genuinely real** in Tauri mode — auth, custody/ceremony, node vitals, wallet
  balances/send/stake/withdraw/tx-history, on-chain claim, and the memory graph all hit
  live 40204 or the real daemon.
- **What remains is almost entirely infra-gated headline features** — local model,
  pinning/IPFS, SBT mint pipeline, cross-app SSO, live Commissary, comms relay — plus
  **validator onboarding** and **honest sync %**. **8 of 10 DGX work-order items are
  undelivered and are the dominant blocker.**
- The frontend "sims" are **dev-only**: `src/bridge/mode.ts` picks `tauri` vs `sim` at
  the boundary, the `Math.random()` walk in `store.ts tick()` is fenced behind
  `if (BRIDGE_MODE === "sim")` (store.ts:696), and `assertSimAllowed()` throws if the sim
  adapter is ever reached in a packaged build. The genuine gaps are enumerated below.

## Git state — start here

- **Repo:** `github.com/CitrateNetwork/citrate-core`. Single worktree, clean tree.
- **Merged to `main` (the surfaces — mostly built):** PRs **#38–#57** (2026-07-14→17):
  node vitals #46, wallet balances #47, real Send+ceremony #48, Stake #53, Withdraw #54,
  AI inference (Rust-custodied key) #55, SBT art #56, tx-history #57; auth-gate+identity
  #45, honest AI/Settings #49/#50, open-links #51, federation hosts #52, onboarding
  checkout #43, popup-webview auth #42, memory daemon #38.
- **No open PRs.** The ~15 remote `feat/core-*` branches are **squash-merge leftovers**
  (their PRs are merged) — **stale, safe to prune**, not pending work.
- **One branch to merge:** `reroll/address-sweep-2026-07-18` (1 ahead of main) re-pins
  `src/data/seed.ts` to the **live** post-reroll addresses (factory `0x5a45B6…`,
  paymaster `0xF14F56…`, SBT `0x7bE005…`, vault `0x0aceb7…`, EntryPoint `0xC698fe…`).
  These match the reconciled canonical book — **merge it to main** so the app targets
  live contracts. (Chain-side detail: `citrate-chain/handoffs/REROLL_SESSION_2026-07-18/
  CANONICAL_TRUTH_HANDOFF.md`.)

## Phase-by-phase (checklist vs reality)

| Phase | Status | Evidence / what remains |
|---|---|---|
| **0 — tier fix + identity SoT** | **DONE** | PR #45. Signed-in never renders a sim persona (store.ts:602). |
| **1 — real node vitals** | **DONE** | PR #46. `refreshNode()` polls `bridge.node.status()` (store.ts:360-382); `node.rs status()` reads live `eth_blockNumber`/`net_peerCount` (node.rs:283-306). *Residual:* per-peer rows blank in tauri (count only) — needs `citrate_getDagStats` (folds into Phase 2). |
| **2 — full validator on 40204** | **PARTIAL** | Self-stake is real (#53). **Missing:** proposer-pubkey registration, validator-set join/read, auto-connect→register flow; honest sync % (binary `0.0/100.0` at **node.rs:299**); stale bootnode `boot-eu1.citrate.ai` **Settings.tsx:586**. Blocked on **WO-1**. |
| **3 — in-flow local model (Gemma)** | **NOT STARTED** | No `src-tauri/src/model.rs`. AI chat uses a remote OpenAI-compatible provider (#55). Needs the whole model module + onboarding procurement. Blocked on **WO-3**. |
| **4 — storage + pinning daemon** | **PARTIAL** | Memory graph real (Storage.tsx). **Missing:** `src-tauri/src/pin.rs` + IPFS client; memory assert/ingest write path (`seam::memory_assert` stub). **Rule-1 gap:** Journal claims "encrypted at rest" (Journal.tsx:331) but `store.save()` writes **localStorage** (store.ts:637). Blocked on **WO-4**. |
| **5 — SBT art → IPFS → tokenURI** | **PARTIAL** | #56 renders a deterministic **local** emblem (`src/identity/SbtEmblem.tsx`) — honest stand-in. **Missing:** IPFS-pinned art + metadata JSON + `tokenURI` set-at-mint + in-app tokenURI image. Blocked on **WO-5** (core-membership/droplet). |
| **6 — cross-app SSO + Commissary + tutorials** | **PARTIAL** | Links open (Commissary.tsx:304+) but **unauthenticated** (no SSO handoff). Commissary download is a `setTimeout` sim (**Commissary.tsx:60-69**); `commissary_catalog` is a seam stub. Tutorials inert. Blocked on **WO-6/7/8**. |
| **7 — remaining real-wiring** | **PARTIAL** | Wallet fully real (#47/#48/#53/#54/#57); Settings honest (#50). **Missing:** `comms.pings`/`comms_connections`, `membership.entitlement`, `chat.backend` (all honest seam stubs today); disable sim `tick()` in tauri. Blocked on **WO-9/WO-10**. |
| **8 — land the work** | **ONGOING** | #38-#57 merged. Owner TODOs: strip diagnostic `eprintln!`s, **rotate `TREASURY_SIGNER_TOKEN`**, signed/notarized DMG + updater (WO-2, @rule8). |

## Surface-by-surface

| Surface | Real? | Remaining stub (file:line) |
|---|---|---|
| Dashboard | mostly real | tutorial rows inert (Dashboard.tsx:247-261) |
| Node | real vitals | peer rows blank in tauri; pinning tab honest-unwired (Node.tsx:145-152); PoSt sealer pending (Node.tsx:381) |
| **Wallet** | **fully real** | — |
| Comms | honest empty | `bridge.comms` unwired (Comms.tsx:13-21) — WO-9 |
| Storage | real graph | empty until daemon running (honest); assert/ingest write path unwired |
| Settings | real/honest | stale bootnode (Settings.tsx:586); billing read persona-derived until WO-10 |
| Commissary | links real, downloads sim | `startDownload` setTimeout (Commissary.tsx:60-69); catalog hardcoded; opens unauthenticated |
| Journal | real editor | **Rule-1:** "encrypted at rest" claim vs localStorage (Journal.tsx:331 ↔ store.ts:637) |

## DGX work-order status (the dominant blocker)

| WO | Delivered? | Blocks |
|---|---|---|
| WO-1 validator onboarding + RPC + bootnodes | **PARTIAL** (RPC live; validator spec not consumed; bootnode string stale) | Phase 2 |
| WO-2 node binaries + signed/notarized DMG + updater | **PENDING** (@rule8) | Phase 8 shipping |
| WO-3 Gemma weights + runtime contract | **NOT delivered app-side** | Phase 3 |
| WO-4 pinning service + IPFS gateway | **NOT delivered** | Phases 4, 5 |
| WO-5 SBT art gen + tokenURI + ABI | **NOT delivered** | Phase 5 |
| WO-6 auth-spine SSO handoff | **NOT delivered** | Phases 6, 8 |
| WO-7 signed catalog + gated downloads | **NOT delivered** | Phase 6 |
| WO-8 Atlas tutorial routes + SSO | **NOT delivered** | Phase 6 |
| WO-9 comms notifications API | **NOT delivered** | Phase 7 Comms |
| WO-10 live entitlement/billing read | **NOT delivered** | Phase 7 Settings |

**Note:** "WO-3 model runtime LIVE on the DGX" is the **inference-gateway** path (the app's
AI chat already uses it via #55). What's unbuilt is the **in-app local Gemma procurement**
(download/verify/launch) — a separate app module regardless of the DGX endpoint.

## Remaining work, prioritized

**P0 — real-validator + shippable (mostly app-side; WO-1/WO-2 gated)**
1. Honest sync %: add `eth_syncing`/`citrate_getDagStats` to `rpc.rs`, replace binary `node.rs:299`.
2. Validator onboarding **through the SignatureCeremony** (Rule 3): proposer-pubkey registration + validator-set membership read + real `blocksProposed` (needs WO-1 spec).
3. Real peer rows from `citrate_getDagStats` (kill `makePeers` in the tauri path).
4. Fix stale bootnode string (Settings.tsx:586).
5. Merge `reroll/address-sweep-2026-07-18` → main (live contract addresses).
6. Signed/notarized DMG + updater key (WO-2, @rule8); rotate `TREASURY_SIGNER_TOKEN`; strip diagnostic `eprintln!`s.

**P1 — headline features (each gated on its WO)**
7. Model module `src-tauri/src/model.rs` + onboarding procurement (WO-3).
8. `src-tauri/src/pin.rs` + IPFS client + memory assert/ingest; **fix Journal localStorage→encrypted-data-dir** to match its own claim (WO-4).
9. SBT mint pipeline: IPFS art+metadata+tokenURI at grant, render tokenURI image in-app (WO-5).
10. SSO handoff command (`open_app_authenticated`) + RP acceptance; route Commissary/tutorial links through it (WO-6/8).
11. Real Commissary: `commissary_catalog` → signed manifest; real streamed downloads + sha256 (WO-7, @rule8).

**P2**
12. `comms.pings` domain + command (WO-9); `membership.entitlement` live read (WO-10); disable sim `tick()` in tauri.

## Key blockers
- **Infra endpoints WO-3..WO-10 (8 of 10 undelivered)** — the app cannot honestly build model/pinning/SBT-mint/SSO/Commissary/comms surfaces without them. Sequence the DGX work order alongside app work.
- **Validator onboarding spec (WO-1)** — the one P0 chain dependency; self-staking exists, validator-set entry does not.
- **@rule8 security gates** — signed/notarized DMG + updater keys (WO-2), gated-download signing (WO-7), treasury token rotation.
- **Money-path addresses** — resolved as of 2026-07-18 (live addresses on the `reroll/address-sweep` branch, pending merge). Not a blocker.

## Test / build state
- **Rust:** `cargo test --workspace --locked` → **294 passed, 3 failed, 6 ignored** (33.5s). The 3 failures are **subprocess-spawn integration tests** the sandbox blocks (stub helper never spawned / loopback), not logic regressions: `agent::tests::start_spawns_stub_and_bearer_round_trips_over_loopback`, `memory::tests::store_on_disk_is_ciphertext_not_plaintext`, `node::tests::data_dir_is_ciphertext_at_rest`. **Re-run on a clean box** with stub binaries built to confirm green.
- **Frontend:** `vitest run` → **131 passed (10 files)**; `tsc --noEmit` → **clean**.
- **Caveat for the ENCRYPT program:** the two `*_is_ciphertext_at_rest` test names assert encryption-at-rest, but `lib.rs:155-170` / `memory.rs` state the mem-store is **plaintext at rest today**. Confirm whether those tests exercise a stub-only path or an aspirational gate — same root as the Journal Rule-1 gap above.

## Recommended sequencing for the engineers
1. **P0 #5** (merge address-sweep) + **#1/#3/#4** (sync %, peer rows, bootnode) — pure app-side, immediate honesty wins.
2. **P0 #2** (validator onboarding) as soon as WO-1's spec lands — this is the headline "real validator" claim.
3. **P1** features in parallel, each unblocked by its WO as the DGX side delivers.
4. **P0 #6** (signed DMG + token rotation) before any public build.
5. Land each behind @rule8 review; owner merges (repo Rule 4).
