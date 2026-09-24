# citrate-core

*Part of the **[Citrate Network](https://citrate.ai)** — own the means of computation. · [Docs](https://docs.citrate.ai) · [Run a node](https://citrate.ai/download) · [Contribute → free membership](https://github.com/CitrateNetwork/.github/blob/main/CONTRIBUTING.md)*

> The Citrate federation desktop app — a Tauri client that runs a full chain-40204 node as a bundled sidecar, so a member's laptop joins and helps run the network.

## What it is

Citrate Core is the "federation desktop house": a Tauri 2 + React desktop application that bundles and runs a full Citrate blockchain node (chain **40204**, testnet-beta, native currency SALT) as a sidecar. It is a trusted (T1) app for money, keys, identity, staking, and distribution — an embedded secp256k1 wallet with a single human-in-the-loop signing ceremony, OIDC sign-in against [citrate-identity](https://github.com/CitrateNetwork/citrate-identity), a membership SBT + staking, and a local AI stack (a bundled Gemma model served by a `llama-server` sidecar, with a hosted inference gateway as fallback).

A hard project rule is **no mocks**: every surface is either live against the real 40204 RPC or shows an honest error. Concept overview: https://docs.citrate.ai/core.

## Prerequisites

Core needs a **sibling `citrate-chain` checkout** to build its node sidecar — a plain `tauri build` is not sufficient.

```bash
# Rust stable (edition 2021; no rust-toolchain.toml pin) + rustfmt/clippy
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy

# Node 22 + npm, and python3 (used by build/verify scripts)
#   (install Node 22 via your version manager)

# Linux Tauri system packages:
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf libgtk-3-dev

# macOS is the primary released target (Apple Silicon, signed/notarized);
# Linux is supported for build/CI.
```

Building the Rust tree needs read access to the private `citrate-chain` repo (a git dependency, pinned by rev). GPU is optional — the app ships both CPU and CUDA ggml backends.

## Build from source

Read `BUILDING.md` first — it is the authoritative build doc. The short version:

```bash
git clone git@github.com:CitrateNetwork/citrate-core.git
git clone git@github.com:CitrateNetwork/citrate-chain.git    # sibling directory

cd citrate-core
npm install
scripts/build-sidecar.sh                       # builds the `citrate` node sidecar from ../citrate-chain
npx tauri build --config src-tauri/tauri.bundle-node.conf.json
```

- The node sidecar (`src-tauri/binaries/citrate-<target-triple>`, ~33 MB) is git-ignored — every machine builds its own. On a Mac, pass a target: `scripts/build-sidecar.sh --target aarch64-apple-darwin` (or `x86_64-apple-darwin`). `build-sidecar.sh` refuses to build if the chain's activation heights are non-zero (chain 40204 re-rolled with both = 0); **a stale sidecar silently forks the chain**, so rebuild it whenever `citrate-chain` moves.
- Artifacts land under `src-tauri/target/release/bundle/` (`.app`/`.dmg` on macOS, plus updater artifacts). The node overlay also bundles the Gemma GGUF model, BGE embeddings, and the docs corpus.
- Tests: `cd src-tauri && cargo test --lib` (expect ~274 passing).

## Run locally

```bash
npm run tauri dev
```

- Dev server runs on **`http://localhost:1420`** (fixed, `strictPort`).
- The local node's loopback RPC listens on `http://127.0.0.1:8545` (WS `127.0.0.1:8546`); P2P on `0.0.0.0:30303`.
- Dev without a built sidecar has no working node — build the sidecar (above) first for full function.

Verify the local node is up once running:

```bash
curl -s -X POST http://127.0.0.1:8545 \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}'
# expect: {"jsonrpc":"2.0","id":1,"result":"0x9d0c"}   # 0x9d0c = 40204
```

## Connect it locally

Out of the box the bundled sidecar node syncs against the public testnet bootnodes (`boot1/2/3.citrate.ai` + the `rpc.citrate.ai` sequencer), and the UI talks to `auth.citrate.ai` (identity) and `infer.citrate.ai` (inference fallback). To wire it to a **local** stack:

1. **Local chain (chain 40204)** — the app already runs its own full node as a sidecar. To point the app's remote-RPC seam at a local node instead of `rpc.citrate.ai`:

   ```bash
   export CITRATE_RPC_URL=http://127.0.0.1:8545
   ```

   Node config resolves in order `--config` → `$CITRATE_CONFIG` → `~/.citrate/node.toml` → `/etc/citrate/node.toml`; the compiled default is `src-tauri/config/member-node.toml` (`chain_id = 40204`, RPC `127.0.0.1:8545`, bootnodes list).

2. **Identity / auth** — OIDC sign-in and OAuth connectors go through `auth.citrate.ai`. Run [citrate-identity](https://github.com/CitrateNetwork/citrate-identity) locally and rebuild against its URL (the auth host is in the Tauri CSP `connect-src` allowlist, so a local host must be added there for a dev build).

3. **Inference** — the local `llama-server` sidecar serves the bundled Gemma model; the remote fallback is `https://infer.citrate.ai/v1`. To skip the ~4.6 GB first-run model download and point at a local/alternate model URL:

   ```bash
   export CITRATE_MODEL_URL=http://localhost:8000/gemma.gguf   # default: https://citrate.ai/download/model
   ```

For the full multi-repo bring-up see `LOCAL_STACK.md` in [citrate-docs](https://github.com/CitrateNetwork/citrate-docs).

## Configuration

No `.env` file — config is a compiled-in TOML (`src-tauri/config/member-node.toml`, written to `<app_data>/node/node.toml` on first launch) plus env-var seams:

| Variable | Default | Purpose |
|---|---|---|
| `CITRATE_RPC_URL` | `https://rpc.citrate.ai` | remote chain RPC seam |
| `CITRATE_CONFIG` | `~/.citrate/node.toml` | node config path override |
| `CITRATE_MODEL_URL` | `https://citrate.ai/download/model` | Gemma GGUF download URL (307 → HF) |
| `CITRATE_CHAIN_ID` | `40204` | chain id |

The app sets consensus env for the sidecar at spawn (`CITRATE_BLOCK_V2=1`, `CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000`, `CITRATE_VALIDATOR_REGISTRY=<addr from src-tauri/addresses/40204.json>`). First run downloads Gemma (`gemma-4-E4B-it-Q4_0.gguf`, 4,590,807,392 bytes, SHA-256-pinned, resumable). Contract addresses live in `src-tauri/addresses/40204.json`.

## Links

- Docs: https://docs.citrate.ai/core
- Depends on: [citrate-chain](https://github.com/CitrateNetwork/citrate-chain) (node sidecar + RPC, chain 40204) · [citrate-identity](https://github.com/CitrateNetwork/citrate-identity) (OIDC) · [citrate-inference-gateway](https://github.com/CitrateNetwork/citrate-inference-gateway) (inference fallback)
- Contributing (DCO): CONTRIBUTING.md · Security: SECURITY.md · License: LICENSE

## License

Source-available under the Business Source License 1.1 (see [`LICENSE`](LICENSE)); converts to Apache-2.0 on the Change Date stated in the license. This is the commercial application-layer / core tier of Citrate's open-core model; the infrastructure tier is Apache-2.0. Licensor: Citrate Inc.
