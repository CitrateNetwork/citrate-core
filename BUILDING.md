---
created: 2026-08-04
branch: docs/mac-build-handoff
author: Claude Opus 5 (1M context), directed by @SaulBuilds
status: active
---

# Building citrate-core (including on macOS)

`git clone && npm run tauri build` is **not** sufficient. Two required pieces are
not in this repo, and getting either wrong fails in ways that look like something
else. Both were found on 2026-08-04 by running the app's own node spawn path on a
clean data dir.

## TL;DR

```bash
git clone git@github.com:CitrateNetwork/citrate-core.git
git clone git@github.com:CitrateNetwork/citrate-chain.git   # sibling directory

cd citrate-core
npm install
scripts/build-sidecar.sh                     # add --target on a Mac, see below
npx tauri build --config src-tauri/tauri.bundle-node.conf.json
```

## 1. The node sidecar is not in git — and a stale one forks the chain

`src-tauri/binaries/citrate-<target-triple>` is ~33 MB and `.gitignore`d. Only
the README beside it is tracked. Every build machine must produce its own.

**Do not copy a `citrate` binary from somewhere else.** Chain 40204 was
re-rolled on 2026-08-04 and both consensus activation heights were set to 0
(citrate-chain PR #157):

| constant | old | required |
|---|---|---|
| `VALUE_TRANSFER_ACTIVATION_HEIGHT` | 300_000 | **0** |
| `MERGE_DEPTH_ACTIVATION_HEIGHT` | 100_000 | **0** |

A sidecar built from an older citrate-chain connects, syncs, and looks perfectly
healthy — while computing **different state roots** below height 300,000 and
accepting merge blocks the fleet rejects. It is a silent fork.

`scripts/build-sidecar.sh` reads both constants out of the citrate-chain source
**before** building and refuses to continue if either is non-zero, so a stale
checkout fails fast with an actionable message instead of shipping a forking app.

```bash
# Apple Silicon
scripts/build-sidecar.sh --target aarch64-apple-darwin

# Intel Mac
scripts/build-sidecar.sh --target x86_64-apple-darwin

# citrate-chain somewhere other than ../citrate-chain
scripts/build-sidecar.sh --chain ~/src/citrate-chain --target aarch64-apple-darwin
```

**Binaries do not cross architectures.** A Mac build must be produced on a Mac
(or with a configured cross toolchain); the script will tell you if the artefact
for the requested triple is missing rather than installing the wrong one.

First build of citrate-chain takes several minutes. Later runs can reuse it:

```bash
scripts/build-sidecar.sh --skip-build --target aarch64-apple-darwin
```

## 2. The node config — how a member finds the network

The node takes bootnodes **only** from `bootstrap_nodes` in a `node.toml`. Its
resolution order is:

```
--config <path>  →  $CITRATE_CONFIG  →  ~/.citrate/node.toml
                 →  /etc/citrate/node.toml  →  an EMPTY default
```

and `bootstrap_nodes` defaults to `[]`. Before 2026-08-04 the app passed none of
these, so a fresh install had nothing to dial: on a clean data dir the node sat
at **height 0 with 0 peers, indefinitely**. It only appeared to work on developer
machines, which happen to have `~/.citrate/node.toml` from chain work.

This is now handled for you: `src-tauri/config/member-node.toml` is compiled into
the app, written to `<app_data_dir>/node/node.toml` on first launch, and passed
explicitly with `--config` (so a member can never silently inherit a developer's
config and join the wrong chain).

Things worth knowing about that file:

- **The sequencer entry is load-bearing.** `boot1/2/3.citrate.ai` are
  discovery-only and advertise height 0, so a fresh node never triggers sync from
  them alone. `rpc.citrate.ai` is what pulls it to the head. Do not "tidy" it out.
- **`[mining] enabled = false` must stay false.** citrate-chain does
  `if cli.mine { config.mining.enabled = true }` — the flag only ever forces
  mining ON, so the config is the base state. citrate-chain's own
  `node/config/testnet-beta.toml` ships `true` because it targets validator
  hosts; copying that here would make every member mine from first launch.
- **It is never overwritten.** Hand edits survive upgrades. Delete the file to
  regenerate it from the shipped default.

## Verifying you built the right thing

### You MUST pass the consensus env, or the node wedges at height 2000

The node does **not** read the §R' epoch-reward settings from `node.toml`. They
arrive as environment variables, and the app supplies them when it spawns the
sidecar (`src-tauri/src/node.rs`, `NODE_VALIDATOR_*` / `NODE_BLOCK_V2_*`).
Running the bare binary without them is **not** the same thing the app does:
validator rewards stay off, so an *empty* block 2000 settles to a different
state root, the receive-path check rejects it, and the node retries that block
forever at ~0 progress.

These three must match the fleet producer's systemd env exactly:

```bash
export CITRATE_BLOCK_V2=1
export CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000
export CITRATE_VALIDATOR_REGISTRY=$(python3 -c "import json;print(json.load(open('src-tauri/addresses/40204.json'))['addresses']['ValidatorRegistry'])")

DD=$(mktemp -d)
sed "s|{{DATA_DIR}}|$DD|" src-tauri/config/member-node.toml > "$DD/node.toml"
src-tauri/binaries/citrate-<triple> --config "$DD/node.toml" --network testnet > "$DD/node.log" 2>&1 &
```

Always pass `--config` explicitly. Without it the node falls back to
`$CITRATE_CONFIG` → `~/.citrate/node.toml`, and on a machine that has done
citrate-chain work that file exists — so you end up verifying a developer's
config instead of the one members actually get.

If you see this, you forgot the env above; the binary is fine:

```
REJECT block <hash> @ 2000 — state root mismatch (claimed …, computed …)
```

### The real proof is your own executor, not an RPC comparison

Comparing `eth_getBlockByNumber` state roots against `rpc.citrate.ai` **proves
almost nothing**. That call returns the root *claimed in the block header*, and
both sides return the same header because it is the same block fetched off the
same network. It matches even while your node is rejecting that very block.

What actually proves agreement is that **your node re-executed each block and
got the same root**. That verdict is in its log:

```bash
grep -c 'state root mismatch'  "$DD/node.log"   # MUST be 0 — this is the gate
grep -c 'root verified'        "$DD/node.log"   # >= height (reorgs re-apply, so it runs slightly ahead)
```

`state root mismatch` must be **exactly 0**. A wedged node still logs plenty of
`root verified` lines for the blocks below the divergence, so a large count on
its own is not the signal — the zero is.

Then confirm it converged on the fleet's head — equal heights, not merely "a
large number":

```bash
for url in http://127.0.0.1:8545 https://rpc.citrate.ai; do
  curl -s -XPOST -H 'content-type: application/json' \
    --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' "$url"
done
```

Reference result, 2026-08-04 on **macOS 15.6.1 / aarch64**, sidecar built from
citrate-chain `9b4c523`: cold sync **genesis → tip, 9,051 blocks**, local height
equal to the fleet's, **0 state-root mismatches**, ~1.25 GB RSS. Genesis
`0xd1a1941e…` / state root `0xd703e8c6…`. The earlier Linux/aarch64 run measured
5,646 blocks in ~6 minutes at ~4.2 GB RSS.

> **Those genesis values are the PRE-September chain.** 40204 re-rolled again on
> 2026-09-07 (1 trillion SALT supply + solc 0.8.36 + A001 genesis-identity
> binding). The current genesis is
> `0x98e0d72f422049606a6b29ca0a9bcfd2300753fd36526d2c8e9f9a2531b70c73`, state root
> `0x3d37893e1d0e03dc762511c2260c2f3703b9e984998c399919b564de4637b5dc` (verified
> against `rpc.citrate.ai` on 2026-09-09). Check block 0, not a height — a node on
> the wrong chain can still reach a large height.

### The env must match the FLEET, not this document

The three variables above are only correct if the fleet is running the same
values. They are not a property of the binary; they are a shared consensus
parameter, and the app hardcodes its side in `src-tauri/src/node.rs`.

`CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000` was verified against the fleet on the
pre-September chain. On the chain re-rolled 2026-09-07 it is **wrong**: a node
that settles §R' rewards at 2000 rejects block 2000 forever, while the live fleet
settles nothing at all (`ValidatorRegistry.emittedInEpoch` is 0 for every epoch
through the tip). Same binary, same chain, activation moved above the tip: syncs
clean. See `docs/FLEET_CONSENSUS_ENV_QUERY_2026-09-09.md`.

The lesson is that `build-sidecar.sh`'s lineage gate cannot catch this — it reads
the citrate-chain *source*, and the source is fine. **Only a full cold sync past
the activation height proves agreement.** Before any release, re-verify the env
against the fleet's systemd unit rather than trusting this file; a re-roll can
move it, and the failure presents as a sync that simply never finishes.

## Running tests

```bash
cd src-tauri && cargo test --lib
```

Expect **274 passing, 0 failed** (5 ignored) — measured 2026-08-04. Two tests use
shell fixtures that invoke `python3 -`
(`stub_mem_mcp.sh`, `stub_node_agent.sh`). They pass with a normal python3 but
fail on hosts whose `python3` is a wrapper rejecting stdin scripts (e.g. some
uv-managed shims) — a fixture limitation, not a code failure. If you see exactly
those two fail, check `echo 'print(1)' | python3 -` before investigating further.

## Memory note

A follower currently keeps full history: DAG pruning is deliberately disabled
(`node.rs`, `NODE_DAG_PRUNE_RETAIN_ENV`) because it used to wedge cold sync at
block 54,600 on the *old* chain. Measured 2026-08-04: ~4.2 GB RSS to sync 5,646
blocks on Linux, ~1.25 GB for 9,051 blocks on macOS. That workaround is no longer
strictly required — the re-rolled chain
enforces MP-DEPTH from genesis, so no valid block can cite a merge parent deeper
than 100 and the 54,600 wedge is impossible by construction — but re-enabling
pruning is a deliberate change that has not been made yet. Budget memory
accordingly on a laptop, and raise it before shipping to real members.
