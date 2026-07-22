---
title: DGX Response — node-sync wedge at 2,580 is a launch-config mismatch, not a chain bug
created: 2026-07-22
branch: docs/dgx-node-sync-wedge-response-2026-07-22
author: Claude (DGX / chain-ops session)
status: dgx-diagnosed-app-action-required
audience: citrate-core app team
answers: docs/DGX_NODE_SYNC_WEDGE_HANDOFF_2026-07-22.md
related: docs/DGX_MONEY_PATH_COMPLETION_HANDOFF_2026-07-22.md
---

# DGX Response — the 2,580 wedge is an app-side launch-config mismatch

## TL;DR

The bundled node does **not** wedge because the producer runs a secret newer commit,
and there is **no missing activation/epoch the binary lacks**. The live chain is
**healthy** and reproducible by a fresh node. The app node wedges because
**`NodeManager` spawns `citrate --network testnet` WITHOUT the three
consensus-critical env vars the fleet producer runs** — most importantly
`CITRATE_VALIDATOR_REGISTRY`. Without it, the node's entire VALIDATOR-S1 / §R'
epoch-reward + registry-snapshot path is **disabled** (`node/src/main.rs:2506`:
*"OFF by default; activates on CITRATE_VALIDATOR_REGISTRY + _ACTIVATION_HEIGHT"*),
so once that path starts affecting state the app's computed `state_root` stops
matching the fleet's and the receive-path root check rejects the block.

**Fix is app-side and almost certainly env-only** (the v2 execute-on-receive code is
already in `citrate-chain` main `f9c1551` and defaults ON). Build the node from
`f9c1551` and launch it with the fleet's consensus env + boot peers (below).

## Proof the chain is healthy (so it is NOT a chain/producer bug)

- **A fresh node cold-syncs straight past 2,580.** Fleet node `boot1`
  (142.93.50.217) had its data dir wiped and recreated at **03:19 UTC today** and
  cold-synced from genesis to **height ~14,700 and climbing** — i.e. it crossed
  2,580 with zero mismatch. If 2,580 were a poisoned/unreproducible block, no fresh
  node could cross it. One does, continuously.
- **Fleet head ~37,000**, producing steadily at 2 s/block with **no timestamp gap**
  at 2,580 (the large deltas in a sparse sample are just skipped blocks × 2 s) — so
  there was no producer restart there, i.e. **not** a restart-poison at 2,580.
- **Every empty block 2,207→2,581 has a distinct state root** — this is the normal
  per-block basic block-reward settlement (10 SALT/block), not a one-off "activation
  at 2,580." There is nothing special about 2,580 in the block data; it is simply the
  first height at which the fleet's §R' path and the app's disabled path diverge.

## Root cause — the producer runs 3 consensus env vars the app node does not

Producer `rpc-1` (142.93.58.145) systemd `citrate-node`:

```
ExecStart=/home/citrate/bin/citrate-node --config /home/citrate/.citrate/node.toml
Environment=CITRATE_BLOCK_V2=1
Environment=CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000
Environment=CITRATE_VALIDATOR_REGISTRY=0x915DdE02831ebacFc57f329f60944492ebb0A095
```

The app spawns (from `src-tauri/scripts/c1_1_node_sync_proof.sh` and `NodeManager`):

```
CITRATE_STORAGE_KEY=… citrate --network testnet --data-dir <dir>
```

— i.e. **none** of the three. Consequences on the app node:

- `CITRATE_VALIDATOR_REGISTRY` absent → `main.rs:1315` sets `validator_registry =
  None` → the registry snapshot-sync, validator selector, and §R' reward-vesting are
  **all OFF** (`main.rs:2506`). The fleet has them **ON** (registry
  `0x915DdE02831ebacFc57f329f60944492ebb0A095`, activation 2000). This is the
  parameter that forks the state root.
- `CITRATE_VALIDATOR_ACTIVATION_HEIGHT` absent → defaults to `0` (`main.rs:1324`).
- `CITRATE_BLOCK_V2` absent → on `f9c1551` this now **defaults ON**
  (`main.rs:1423`, `.unwrap_or(true)`; comment: *"DEFAULT ON since the 2026-07-21
  SRP reroll"*), so v2 itself is fine — set it explicitly anyway for clarity.

## The variables to build/launch with

```bash
# 1) Build the node from citrate-chain main:
#    commit f9c1551 (v2 execute-on-receive; DEFAULT ON). The live fleet binary was
#    built from this line at the 2026-07-21 18:58 SRP-S3c reroll.

# 2) Launch it with the fleet's consensus env (align NodeManager's spawn):
CITRATE_BLOCK_V2=1
CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000
CITRATE_VALIDATOR_REGISTRY=0x915DdE02831ebacFc57f329f60944492ebb0A095

# (Peers are NOT your blocker — you cold-synced 2,580 blocks, which is impossible
#  without working peers, and f9c1551's embedded testnet-beta.toml already carries
#  all 4 fleet boot peers. Listed here only as reference / for a from-scratch node.
#  The load-bearing fix is the two VALIDATOR_* env vars above.)
noise_f356d3ebb07371eaad371b3960272f9d58fc457cde409c34549ef03776b78141@boot1.citrate.ai:30303
noise_4ed281386422f6a65b92d8760d24baa82bb1b476e9dd3e21d5a070d026802c07@boot2.citrate.ai:30303
noise_2b4924671e0babc9f52eb1695c72141a9c639e17ad95a2a2d2a715eae34a420e@boot3.citrate.ai:30303

# Chain identity for sanity checks:
chain_id = 40204
genesis (v1 hash) = 0xd1a1941ede584b26d6e41c7d4b5e1134813ad59f3b066fce0dfca84380276fe4
```

> **The single fix:** set `CITRATE_VALIDATOR_REGISTRY` +
> `CITRATE_VALIDATOR_ACTIVATION_HEIGHT` (and `CITRATE_BLOCK_V2=1` explicitly) in the
> node spawn. Everything else — peers, genesis, v2, the S1/S2/S3 code — is already
> correct on `f9c1551`. This is corroborated by the citrate-chain SRP notes, which
> independently flagged the same `NodeManager` gap (spawn sets only
> `--network testnet --data-dir` + `CITRATE_STORAGE_KEY`, no validator env).

## How to verify (app side)

1. Rebuild the aarch64 node from `f9c1551`, re-drop to
   `src-tauri/binaries/citrate-aarch64-apple-darwin`.
2. Fresh-data-dir cold sync with the env + peers above. Expect it to **cross 2,580
   without a mismatch** and track the tip (~37k+).
3. If it *still* wedges at 2,580 with the exact env above, the live producer binary
   is a hair ahead of `f9c1551` in execution rules (not just env) — reply and DGX
   will diff the running producer commit and pin the exact SHA to build from. (This
   is the low-probability branch; the boot1 cold-sync proves the *chain* is fine, so
   any residual gap is purely "which binary/env the app builds," which DGX can pin.)

## What was ruled OUT (don't re-chase these)

- **Cross-architecture (aarch64 vs x86_64):** refuted in writing, twice, in the
  citrate-chain SRP docs — a controlled two-arch experiment computed **identical**
  roots. Arch is not the axis; **launch-config** is.
- **Poisoned / unsyncable block at 2,580:** refuted by boot1's live fresh cold-sync
  past it today.
- **Missing producer commit / missing activation:** refuted — `f9c1551` already
  contains SRP-S1/S2/S3 and v2 execute-on-receive (default ON). The gap is env, not
  code.

## Remediation (durable — prevents the whole class)

The real footgun is that VALIDATOR-S1 consensus params come **only** from env
(`CITRATE_VALIDATOR_REGISTRY` / `_ACTIVATION_HEIGHT` default to `None`/`0`) — so a
fresh node launched "the obvious way" (as the app does) silently runs *different
consensus rules than the fleet* and forks, with no error. This is a **citrate-chain**
change DGX will carry:

1. **Make `--network testnet` self-sufficient:** bake the fleet's activation height
   and registry address into the embedded testnet preset (peers are already there) so
   a bare `citrate --network testnet` reproduces the fleet with **zero** env — which
   is what the app already assumes.
2. **Fail loud, not silent:** when `--network testnet` is selected but the resolved
   validator registry/activation is empty, **hard-error at boot** instead of running
   a divergent v1-style chain. A node that would fork should refuse to start.
3. **Sync tripwire:** a CI/ops check that a from-genesis cold-sync on the *shipped*
   app binary crosses the current activation height, run on each node-binary bump.

Until (1) lands, the app must set the env + peers above explicitly.
