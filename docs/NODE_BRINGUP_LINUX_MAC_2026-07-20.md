---
title: "citrate-core bundled-node bring-up — Linux (verified) + Mac (recipe)"
created: 2026-07-20
branch: main
author: Claude (Opus 4.8, 1M) for SaulBuilds
status: Linux build VERIFIED on aarch64; Mac recipe for the core team (macOS SDK not available on the DGX)
---

# TL;DR

citrate-core builds and runs on Linux (verified on the aarch64 DGX: frontend 759
modules clean, `src-tauri` backend 0 errors, encrypted-at-rest node spawn works).
The bundled `citrate` node MUST be built from **citrate-chain with the resilient
full-replay sync fix (PR #91)** or a fresh member's node cannot catch up to a deep
chain. **The bundled node's genesis MUST match the live chain** — see the genesis
gotcha below; this is the #1 thing to get right around the reroll.

# What was verified on Linux (aarch64 DGX, 2026-07-20)

- `npm run build` (tsc + vite) → clean, `dist/` emitted (759 modules).
- `cargo build --release -p citrate-core` → 0 errors; `libcitrate_core_lib.{a,so,rlib}` + `citrate-core` produced.
- NodeManager-style spawn (`citrate --network testnet --data-dir <dir>` with
  `CITRATE_STORAGE_KEY`) → node boots, serves RPC, writes `encryption.meta`, and the
  RocksDB `.sst`/`.log` data is ciphertext (no plaintext `40204` chain-id marker in the
  data files; only in the stdout log, which is expected).
- Deep-sync itself is proven separately: a fresh follower with the PR #91 binary syncs a
  deep multi-producer chain 0→head through the deploy region on a HEALTHY fleet (the old
  ~223 wedge was live-fleet health noise, not sync logic — a fresh reroll's healthy fleet
  removes that confound).

> Note: the DGX is **aarch64-linux**; most Linux users/the fleet are **x86_64-linux**.
> Build the x86_64 sidecar on an x86_64 Linux host (or the fleet's rpc-1), same recipe.

# The three bundled sidecars (Tauri `externalBin`s)

`binaries/citrate`, `binaries/node-agent`, `binaries/mem-mcp` — declared in the overlay
`src-tauri/tauri.bundle-node.conf.json` (NOT the base config), resolved at runtime by
`resolve_node_bin` / `resolve_agent_bin` / `resolve_mem_mcp_bin` or the `CITRATE_NODE_BIN`
/ `CITRATE_NODE_AGENT_BIN` / `CITRATE_MEM_MCP_BIN` overrides. Binaries are `.gitignore`d
(20+ MB each); copy per-host with the target-triple suffix. See `src-tauri/binaries/README.md`.

# Mac bring-up recipe (core team — Apple Silicon)

Run ON a Mac (the DGX has the rust apple-darwin targets but no macOS SDK/linker, so it
cannot produce a runnable Mach-O binary — this must be built on macOS).

```bash
# 1) The node — from citrate-chain, on the reroll branch (PR #91 sync + rotated genesis)
cd citrate-chain
git checkout reroll-prep/deployer-rotation        # or main once the reroll branch is merged
cargo build --release --bin citrate
TRIPLE=$(rustc -vV | sed -n 's/host: //p')          # aarch64-apple-darwin on Apple Silicon
cp target/release/citrate ../citrate-core/src-tauri/binaries/citrate-$TRIPLE

# 2) node-agent — from citrate-node-agent (source pin: citrate-federation planset/core-beta-wiring)
cd ../citrate-node-agent && cargo build --release --bin node-agent
cp target/release/node-agent ../citrate-core/src-tauri/binaries/node-agent-$TRIPLE

# 3) mem-mcp — from citrate-memories (mcp_serve example, rocksdb+transformer features)
cd ../citrate-memories && cargo build --release -p mem-mcp --example mcp_serve --features rocksdb,transformer
cp target/release/examples/mcp_serve ../citrate-core/src-tauri/binaries/mem-mcp-$TRIPLE

# 4) Build citrate-core (frontend verified; bundle uses the overlay so externalBin resolves)
cd ../citrate-core
npm install
npm run build
npx tauri build --config src-tauri/tauri.bundle-node.conf.json

# 5) Prove the bundled node actually syncs (spawns it the way NodeManager does)
src-tauri/scripts/c1_1_node_sync_proof.sh target/release/citrate   # or the copied binary
#   asserts: net_peerCount > 0, eth_blockNumber advances, data dir ciphertext-at-rest
```

Intel Macs: same, `TRIPLE=x86_64-apple-darwin`.

# ⚠️ Genesis-alignment gotcha (the #1 failure mode)

The bundled node's compiled genesis MUST equal the live chain's genesis, or it forms a
different genesis block and **never syncs** (peers reject / no common ancestor):

- **Before the 2026-07-20 reroll** (chain still on the old-deployer genesis): build the
  node from a commit with the OLD genesis (e.g. `main` before the rotation) + PR #91.
- **After the reroll** (rotated deployer `0x4fAB35c8`, new staker/operator set): build from
  `reroll-prep/deployer-rotation` (or `main` once merged). The pre-reroll binary will NOT
  sync the post-reroll chain and vice-versa.

Ship the Mac bundle built from the SAME source generation the live fleet is running. See
`citrate-chain/handoffs/REROLL_2026-07-20_DEPLOYER_ROTATION_RUNBOOK.md` for the reroll
timing + the full address book the app's `earnings.rs`/contract reads depend on.

# After the reroll — re-pin addresses the app reads

25 core/feature + AA factory/paymaster + registry/SBT/vault shift under the new deployer.
Re-run citrate-core's address sync (and confirm `earnings.rs` selectors/addresses match
`citrate-node-agent/crates/chainio/src/generated/addresses.json`) once the reroll is live.
