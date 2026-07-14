---
created: 2026-07-13
branch: feat/core-c1-1-node
author: Claude Fable 5, directed by @SaulBuilds
status: active
---

# src-tauri/binaries — bundled sidecars: citrate-node (D-C1-1) + node-agent (C1.2)

Two sidecars are bundled as **Tauri `externalBin`s**: the `citrate` node (C1.1)
and the `node-agent` (C1.2). Both externalBin declarations live in a SEPARATE
overlay config, `tauri.bundle-node.conf.json`
(`bundle.externalBin: ["binaries/citrate", "binaries/node-agent"]`), NOT in the
base `tauri.conf.json`.

**Why an overlay and not the base config:** `tauri-build` (the `build.rs` step)
validates every `externalBin` path *on every `cargo build`* — so putting it in
the base config would fail plain `cargo build` / `cargo test` in CI whenever the
(uncommitted, ~20 MB) binary is absent. The overlay is merged only at packaging
time via `--config`, when the binary has been copied in:

```bash
npx tauri build --config src-tauri/tauri.bundle-node.conf.json
```

At runtime the app resolves each sidecar from the bundled resource dir
(`node::resolve_node_bin` / `agent::resolve_agent_bin`) or from an override env
var (`CITRATE_NODE_BIN` / `CITRATE_NODE_AGENT_BIN`), so dev and tests never need
the bundle.

Tauri resolves the platform binary by target-triple suffix, so this directory
must contain, per build host:

```
binaries/citrate-aarch64-apple-darwin       # Apple Silicon macOS (node)
binaries/citrate-x86_64-apple-darwin         # Intel macOS (node)
binaries/citrate-x86_64-unknown-linux-gnu    # Linux (node)
binaries/citrate-x86_64-pc-windows-msvc.exe  # Windows (out of beta scope, O-4)
binaries/node-agent-aarch64-apple-darwin     # Apple Silicon macOS (node-agent)
binaries/node-agent-x86_64-apple-darwin      # Intel macOS (node-agent)
binaries/node-agent-x86_64-unknown-linux-gnu # Linux (node-agent)
```

**The binaries are NOT committed** (each is 20+ MB) — they are `.gitignore`d
(`src-tauri/binaries/*` with a `!README.md` exception). Only this README is
tracked.

## Getting the binaries for local dev / packaging

Build each from its pinned source and copy it here with the target-triple
suffix.

The **node** (bin `citrate`, package `citrate-node`, from citrate-chain):

```bash
cargo build --release --bin citrate
TRIPLE=$(rustc -vV | sed -n 's/host: //p')
cp target/release/citrate \
   /path/to/citrate-core/src-tauri/binaries/citrate-$TRIPLE
```

The **node-agent** (bin `node-agent`, from citrate-node-agent; the source pin is
tracked via a Rule-12 `[[drift]]` entry on
citrate-federation planset/core-beta-wiring):

```bash
# from citrate-node-agent:
cargo build --release --bin node-agent
TRIPLE=$(rustc -vV | sed -n 's/host: //p')
cp target/release/node-agent \
   /path/to/citrate-core/src-tauri/binaries/node-agent-$TRIPLE
```

## Running without a bundle (dev + tests)

For `tauri dev` and headless runs bypass the bundle by pointing the app at
already-built binaries:

```bash
export CITRATE_NODE_BIN=/absolute/path/to/citrate
export CITRATE_NODE_AGENT_BIN=/absolute/path/to/node-agent
```

`resolve_node_bin` / `resolve_agent_bin` honour the override env first, then fall
back to the bundled resource dir.

## CI cross-build (S7)

Per-platform cross-compilation + notarization of these sidecar binaries is an
**S7** release-hardening item, not part of C1.x. CI cannot build the node-agent
(heavy, private repo), so its cross-build joins the node's at S7. Until then,
packaging is a per-host manual copy as above.
