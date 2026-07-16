---
title: "citrate-core Master Checklist — Real Validator, Identity, Storage, SSO, Model, SBT"
created: 2026-07-16
branch: fix/core-d3-0-popup-main-thread
author: Claude (Opus 4.8) for SaulBuilds
status: draft
related:
  - docs/CITRATE_CORE_DGX_WORK_ORDER.md
---

# citrate-core — Master Checklist

The end state: a **fully working node that auto-connects as a real validator on
testnet 40204**, reads **real peers + block height**, signs users in on the auth
spine, shows **the real signed-in user everywhere** (no prototype personas),
procures the **local Gemma model in-flow**, runs a **storage + pinning daemon**,
mints **SBT identity art to IPFS**, and hands users to federation webapps **already
signed in**.

Legend: **[core]** = citrate-core app, **[cm]** = core-membership/droplet,
**[dgx]** = infra (see work order WO-#), **[rp]** = relying-party webapp.
`file:line` cites the exact code to change. Findings from the 2026-07-16
four-subsystem audit.

---

## Phase 0 — DONE (this session)

- [x] **Tier vocabulary fix** — authority `commercial.kyc` now normalizes to app
  `pilot` at the OIDC boundary so a granted membership actually unlocks the app
  (`src-tauri/src/oidc.rs` `normalize_tier` + `eff_tier`). Verified live:
  `eff_tier=Some("pilot")`, kyc verified.
- [x] **Identity = single source of truth** — `applyAuthStatus` now folds
  `sub`/`email`/`wallet`/derived name+initials and sets `signedIn`; new
  `store.identity()` resolver; Sidebar, Settings (Account & RBAC), Wallet render the
  real user; Comms shows an honest empty state instead of demo pings; logout clears
  identity. (`src/shell/state.ts`, `src/shell/store.ts`, `src/surfaces/{Settings,
  Comms,Wallet}.tsx`, `src/shell/Sidebar.tsx`.) 77 tests pass, typecheck clean.

### Phase 0 residual to verify/harden
- [ ] **[core]** Guarantee `email`/`wallet` are present at first fold: confirm the
  login result carries them, else call `auth.userinfo()` right after `authLogin` so
  identity folds immediately for an already-granted member (not only after a
  membership poll). Check `store.authLogin()` (`src/shell/store.ts`) and
  `oidc.rs` login claims vs `/userinfo` claims.
- [ ] **[core]** On boot, `start() → refreshAuth()` folds `auth.status()` (in-memory
  claims). Confirm `auth.status()` returns full claims (email/wallet) after a restart
  where the session is restored from the refresh token, not just after `/userinfo`.

---

## Phase 1 — Real node vitals (kill the fabricated peers/height)  **[core] · P0**

Root cause of "fake chain / too-high peers + height": the Node surface + sidebar read
a JavaScript random-walk, never the real bridge. The backend (`node.rs`/`rpc.rs`) is
already real and wired.

- [ ] Node surface polls `bridge.node.status()` (real height/peers/syncPct/state from
  `node.rs`) instead of `AppState` — `src/surfaces/Node.tsx:82-95`.
- [ ] `store.startNode()` calls `bridge.node.start()`; Pause/Resume/Stop call the real
  supervisor — replace the `setTimeout` theater at `src/shell/store.ts` `startNode()`.
- [ ] Sidebar height/sync read the polled real values — `src/shell/Sidebar.tsx:37`.
- [ ] Delete/guard the sim node-vitals block in `tick()` for tauri mode
  (`src/shell/store.ts` `tick()` — height/peers/syncPct/`makePeers()`).
- [ ] Persona `peers=` presets become sim-only (`src/shell/state.ts`).
- [ ] Dashboard vitals strip routes through `bridge.node.status()` (peers/finality/
  node) — today only chain height is live via wagmi (`src/surfaces/Dashboard.tsx`).
- **Acceptance:** with the node off, peers = 0 and height = live `rpc.citrate.ai`;
  with it on, both come from the local node RPC. No `Math.random()` in the path.

---

## Phase 2 — Full validator on 40204 (auto-connect)  **[core]+[dgx] · P0**

- [ ] **[dgx]** WO-1 delivered (bootnodes+keys, RPC methods, validator spec, faucet).
- [ ] **[dgx]** WO-2 delivered (platform node binaries present so `resolve_node_bin`
  succeeds; the "node overlay" build is shipped).
- [ ] **[core]** Honest sync %: add `eth_syncing` (or `citrate_getDagStats`) to
  `rpc.rs` and compute real progress — replace the binary 0/100 at `node.rs:299`.
- [ ] **[core]** Real peer rows from a node RPC (`citrate_getDagStats`/peer info) to
  replace `makePeers()`.
- [ ] **[core]** Validator registration + staking flow **through the
  SignatureCeremony** (Rule 3): register proposer pubkey, stake ≥ threshold to
  `LiquidStakingPool`, read validator-set membership + real `blocksProposed`.
- [ ] **[core]** Add `chainId`/`bootnodes` to `AppConfig` (`src-tauri/src/config.rs`)
  if operator override is wanted; pass to the node spawn args (`node.rs` build_spec).
  Fix stale `boot-eu1` string (`src/surfaces/Settings.tsx:508`).
- [ ] **[core]** Auto-connect on first run: after install + sign-in, spawn the node,
  dial bootnodes, sync, and (once funded) register as validator — in the smooth flow.
- **Acceptance:** a fresh install auto-syncs against boot1/2/3, shows real peers, and
  can enter the validator set.

---

## Phase 3 — In-flow local model (Gemma) procurement  **[core]+[dgx] · P1**

- [ ] **[dgx]** WO-3 delivered (weights URL + checksum + runtime contract).
- [ ] **[core]** New model module + `#[tauri::command]`s: `model_status`,
  `model_download` (streamed, resumable), `model_verify` (SHA-256), wired like the
  memory-daemon manager in `lib.rs`.
- [ ] **[core]** Slot procurement into the onboarding stage machine (the smooth open
  flow) — download + verify + launch the runtime, with honest progress; harness
  (`agent.rs`) defers to the local endpoint, remote gateway as fallback.
- [ ] **[core]** Settings → a model section (path, version, re-download, switch to
  remote).
- **Acceptance:** first run downloads + verifies Gemma, launches the runtime, and the
  in-app agent answers from the local model (or honest fallback).

---

## Phase 4 — Storage + pinning daemon, Settings, graph ingest  **[core]+[dgx] · P1**

The Storage graph is wired to the real memory daemon (`mcp_serve`) and is empty only
because **nothing ingests nodes**. The **pinning daemon does not exist** and must be
built.

- [ ] **[dgx]** WO-4 delivered (pinning service + IPFS gateway).
- [ ] **[core]** New `src-tauri/src/pin.rs` + commands `pin_add`/`pin_status`/
  `pin_list`/`pin_remove`/`storage_stats` (IPFS Pinning Service client or embedded
  node). Value-bearing bond actions route through the SignatureCeremony (Rule 3).
- [ ] **[core]** Replace the pin **sims** (fabricated CIDs) in `Journal.tsx:201-224`
  and `Node.tsx:150-175` with real `pin_add` calls.
- [ ] **[core]** Memory **ingest** path so `constellation()` returns real nodes: wire
  `memory.assert` (currently a seam stub) + a chain-state ingest; add a
  **memory-daemon start control** so the user can bring it online (root cause of the
  empty graph — `src/surfaces/Storage.tsx:178-203`).
- [ ] **[core]** Journal persistence to the encrypted Rust data dir (today it's plain
  `localStorage` despite the "encrypted at rest" claim at `Journal.tsx:348` — Rule 1
  gap).
- [ ] **[core]** New **Settings → Storage & Pinning** section (`Settings.tsx` SECS):
  pinning endpoint + token (sealed in keyring), IPFS gateway URL, replication factor,
  bond policy, challenge cadence, memory-daemon start/stop/status.
- **Acceptance:** the Storage graph populates from the running daemon; a pin produces
  a real retrievable CID; storage/pinning is configurable in Settings.

---

## Phase 5 — SBT identity art → IPFS → tokenURI  **[cm]+[dgx] · P1**

- [ ] **[dgx]/[cm]** WO-5 delivered (art gen point, IPFS pin, `CitrateMemberSBT`
  tokenURI ABI).
- [ ] **[cm]** Deterministic generative art (abstract geometric + pixel, 2–5 color
  palettes, seeded by tokenId/sub) generated at grant in core-membership/droplet.
- [ ] **[cm]** Pin art + metadata JSON to IPFS (Phase 4 service); set the token URI at
  mint (droplet signer).
- [ ] **[core]** Render the SBT image as the user's identity icon in-app (replace the
  bare `hasSbt` flag; fetch tokenURI → gateway image).
- **Acceptance:** each granted member gets a unique on-chain SBT whose IPFS image
  renders as their in-app identity icon.

---

## Phase 6 — Cross-app SSO + Commissary + tutorials  **[core]+[dgx]+[rp] · P1**

- [ ] **[dgx]** WO-6 delivered (authority SSO handoff mechanism chosen + built).
- [ ] **[rp]** Atlas/CitrateScan/Dataroom/Comms/Memrizz/Marketplace accept the handoff
  and verify the same entitlement (WO-6/WO-8).
- [ ] **[core]** `open_app(target)` command (mirror `membership.rs` popup / opener) +
  real `onClick` on the docs/services/SDK spans (`Commissary.tsx:301-362`).
- [ ] **[core]** `open_app_authenticated(target)` — build the RP URL with the SSO hint
  (prefer no-token-egress shared-authority path per ADV-8); gate on `auth_status`.
- [ ] **[core]** Wire `commissary_catalog` to the signed manifest (WO-7); real gated
  downloads (stream + sha256) replacing the `setTimeout` sim
  (`Commissary.tsx:54-68`).
- [ ] **[core]** Tutorials: **sort** (unlocked-first / tier / minutes) and make the
  CTA route to Atlas **through the SSO handoff** (`Dashboard.tsx:247-261`,
  `seed.ts:78-83`).
- **Acceptance:** Commissary lists live apps and installs them for real; clicking a
  service/tutorial lands the user in Atlas et al. **already signed in** on their
  entitlement.

---

## Phase 7 — Remaining real-wiring  **[core]+[dgx] · P2**

- [ ] **[core]** Wallet: wire `wallet.balances`/`wallet.activity` to real commands
  (replace `seam::wallet_*`); Wallet surface reads them instead of `AppState`; stop
  fabricating tx hashes — settle via `signing.broadcast` (`Wallet.tsx`).
- [ ] **[core]+[dgx]** Comms: `comms.pings` domain + Rust command against the relay
  (WO-9); replace the honest empty state with live pings.
- [ ] **[core]+[cm]** Settings: wire `membership.entitlement` (WO-10) for real billing;
  make Connections OAuth + gateway-key issuance real (replace `setTimeout`/`AppState`).
- [ ] **[core]** `chat.backend` wired so Dashboard/Journal agent panels reflect the
  real transport.
- [ ] **[core]** Global: disable the sim `tick()` loop in tauri mode so no fabricated
  state can leak into the packaged app (`src/shell/store.ts` `tick()`).

---

## Phase 8 — Land the work (Agentile)  **[core]+[cm]**

- [ ] Fold this session's live-debug fixes into reviewable PRs with @rule8 review:
  - **[cm]** `fix/oidc-callback-path` (callback path + live KYC re-check + F-1/F-2) →
    PR/review/merge.
  - **[core]** popup main-thread/async, nested-entitlement parser, tier normalize,
    identity source-of-truth, prototype-tag removal → PR/review/merge.
- [ ] Remove diagnostic `eprintln!`s from `oidc.rs`/`membership.rs` before final.
- [ ] **Rotate `TREASURY_SIGNER_TOKEN`** (was pasted in chat).
- [ ] Rebuild the signed/notarized DMG once Phases 1–2 land (WO-2).

---

## Sequencing recommendation

1. **Phase 1** (real node vitals) — pure app-side, immediately kills the "fake chain"
   feel. Do first.
2. **Phase 0 residual** (identity folds on login) — small, closes the bug you saw.
3. **Phase 2** (validator) — needs WO-1/WO-2 from DGX in parallel.
4. **Phases 3–6** — each gated on its work-order item; can proceed in parallel once
   endpoints land.
5. **Phase 8** — continuous; land each phase behind review before the next DMG.
