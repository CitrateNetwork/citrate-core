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

After `scripts/build-sidecar.sh`, run the sidecar directly and confirm it reaches
the tip. On a clean data dir it should find peers within seconds and sync at
roughly 900 blocks/min:

```bash
DD=$(mktemp -d)
src-tauri/binaries/citrate-<triple> --network testnet --data-dir "$DD" &
sleep 60
curl -s -XPOST -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
  http://127.0.0.1:8545
```

The strongest check is **state-root parity with the fleet** — this is what proves
you are not on a fork. Compare a few heights against the public RPC; they must be
identical:

```bash
for h in 0x64 0x7d0 0x1388; do
  echo "height $((h))"
  for url in http://127.0.0.1:8545 https://rpc.citrate.ai; do
    curl -s -XPOST -H 'content-type: application/json' \
      --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$h\",false],\"id\":1}" \
      "$url" | grep -o '"stateRoot":"0x[0-9a-f]*"'
  done
done
```

Reference result from the 2026-08-04 validation on Linux/aarch64: genesis
`0xd1a1941e…`, cold sync **genesis → head, 5,646 blocks in ~6 minutes**, state
roots identical at heights 100 / 2000 / 5000 / 5376 — including across the
validator-activation boundary at 2000.

## Running tests

```bash
cd src-tauri && cargo test --lib
```

Expect **272 passing**. Two tests use shell fixtures that invoke `python3 -`
(`stub_mem_mcp.sh`, `stub_node_agent.sh`). They pass with a normal python3 but
fail on hosts whose `python3` is a wrapper rejecting stdin scripts (e.g. some
uv-managed shims) — a fixture limitation, not a code failure. If you see exactly
those two fail, check `echo 'print(1)' | python3 -` before investigating further.

## Memory note

A follower currently keeps full history: DAG pruning is deliberately disabled
(`node.rs`, `NODE_DAG_PRUNE_RETAIN_ENV`) because it used to wedge cold sync at
block 54,600 on the *old* chain. Measured 2026-08-04: ~4.2 GB RSS to sync 5,646
blocks. That workaround is no longer strictly required — the re-rolled chain
enforces MP-DEPTH from genesis, so no valid block can cite a merge parent deeper
than 100 and the 54,600 wedge is impossible by construction — but re-enabling
pruning is a deliberate change that has not been made yet. Budget memory
accordingly on a laptop, and raise it before shipping to real members.
