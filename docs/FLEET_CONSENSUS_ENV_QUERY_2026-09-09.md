---
created: 2026-09-09
branch: build/reroll-rebuild-2026-09-09
author: Claude Opus 5 (1M context), directed by @SaulBuilds
status: active
---

# Mac team ⇒ DGX: what is the fleet's §R' consensus env on the re-rolled 40204?

**Blocking:** the citrate-core macOS release rebuild. The app is otherwise built and
tested at `citrate-chain@e68af83`; the DMG is deliberately NOT cut until this is
answered, because shipping on the current value wedges every member at height 2000.

## The one question

Read these off the fleet producer's systemd unit (`rpc.citrate.ai`, and the
`boot1/2/3` nodes if they differ) and send the literal values:

```bash
systemctl show -p Environment citrate    # or: cat /etc/systemd/system/citrate.service
```

| var | fleet value? |
|---|---|
| `CITRATE_VALIDATOR_ACTIVATION_HEIGHT` | **?** |
| `CITRATE_VALIDATOR_REGISTRY` | **?** |
| `CITRATE_BLOCK_V2` | **?** |
| `CITRATE_DAG_PRUNE_RETAIN` | **?** (expect unset) |

Also: **which citrate-chain commit is the fleet binary built from?** `web3_clientVersion`
returns only `citrate/v0.1.0`, which does not distinguish builds.

## What citrate-core currently sends

`src-tauri/src/node.rs` hardcodes these when it spawns the sidecar:

```
CITRATE_BLOCK_V2=1
CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000
CITRATE_VALIDATOR_REGISTRY=<ValidatorRegistry from src-tauri/addresses/40204.json>
                           = 0x2655d9fbbe599e75ff6e53790f99ebc9a20c93bf
```

`2000` was correct for the pre-September chain (see
`citrate-chain/handoffs/SRP_S2_REROLL_EXECUTION_STATUS.md`: "Systemd env on ALL
(verified): CITRATE_BLOCK_V2=1, CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000"). It is
**not** correct for the chain that re-rolled on 2026-09-07.

## Evidence

Controlled A/B on 2026-09-09, macOS 15.6.1 / aarch64. **One binary**
(`citrate-chain@e68af83`, built with `LZMA_API_STATIC=1`), **one chain**, two clean
data dirs. The only difference is the activation height.

| run | `CITRATE_VALIDATOR_ACTIVATION_HEIGHT` | result |
|---|---|---|
| A | `2000` (what the app ships) | **REJECT block 2000, forever.** 44 `state root mismatch` lines and climbing; local head pinned. |
| B | above the tip (rewards never settle) | Crossed 2000 without incident, **0 mismatches**, still climbing past 2,604. |

Run A's rejection:

```
WARN citrate::canonical_apply: execute-on-receive: REJECT block 880955c0 @ 2000
     — state root mismatch (claimed 8da9889b, computed cf77b617)
```

Block 2000 is **empty** (`gasUsed 0x0`, 0 transactions), so the only state delta at
that height is the §R' reward settlement. The two nodes disagree about whether it
happens at all.

Corroborating on-chain read — the fleet is paying **no** block subsidy anywhere:

```
ValidatorRegistry.emittedInEpoch(e)   # selector 0xb85a2983
  epoch 0,1,2,3,4,5,10,40,80,81  ->  0 SALT   (every epoch through the tip)

blockSubsidy()        = 10 SALT        # parameterised, never paid
priorityFeeShareBps() = 10000
maxEpochEmission()    = 10000 SALT
rewardMinter()        = 0x0000000000000000000000000000000050524950   ("PRIP", as specified)
validatorInfo(0xac5f4c60…).rewards = 0 SALT   # the block-2000 proposer, bonded 32,000, status 1
```

`emittedInEpoch == 0` everywhere means `creditReward` has never fired on this chain.
The registry itself is deployed and correctly parameterised — it is the execution
layer that is not settling.

Not the cause, ruled out explicitly:

- **Not the genesis.** The rebuilt node computes genesis
  `0x98e0d72f422049606a6b29ca0a9bcfd2300753fd36526d2c8e9f9a2531b70c73` / state root
  `0x3d37893e1d0e03dc762511c2260c2f3703b9e984998c399919b564de4637b5dc`, byte-identical
  to the live chain. It is on the right chain.
- **Not the missing-env mistake** documented in `BUILDING.md`. All three vars were
  verified present in the running process via `ps eww`.
- **Not the registry address.** Taken from the address book synced at `2d88191`;
  `eth_getCode` is non-empty and the node logs
  `synced validator set for epoch 1 at snapshot height 800 (4 validators)` and
  `epoch 2 at 1800`, so registry sync works.
- **Not the EIP-161 contract-nonce rule** (`ece71b5`). That activates at 30,000 and
  is byte-identical below it; the divergence is at 2000.
- **Not stale chain source.** `VALUE_TRANSFER_ACTIVATION_HEIGHT` and
  `MERGE_DEPTH_ACTIVATION_HEIGHT` are both 0, so `build-sidecar.sh`'s lineage gate
  passes.

## What we need back

1. The four values above, literal, from the running fleet.
2. Confirmation of the intended §R' plan on the re-rolled chain: is reward
   settlement meant to be **off for now** (activation parked above the head, to be
   moved later), or **on** at a specific height the fleet has not reached or has
   mis-set?

Once we have (1), citrate-core sets its constant to match exactly and the release
proceeds. The member node must agree with the fleet on this number or it forks —
and it forks *silently*, presenting as a sync that never finishes.

## Note on the lineage gate

`scripts/build-sidecar.sh` checks the two re-roll activation constants in the
citrate-chain *source*, which is why it passed here. It cannot see a **runtime**
env mismatch between the app and the fleet. Worth adding a fleet-parity check to
that gate once the canonical values are known — the failure mode this time was
invisible until a full cold sync ran.
