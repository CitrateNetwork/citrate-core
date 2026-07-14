---
created: 2026-07-13
branch: feat/core-c1-1-node
author: Claude Fable 5, directed by @SaulBuilds
status: active
---

# src-tauri/binaries — bundled citrate-node sidecar (D-C1-1)

The `citrate` node is bundled as a **Tauri `externalBin`**. The externalBin
declaration lives in a SEPARATE overlay config, `tauri.bundle-node.conf.json`
(`bundle.externalBin: ["binaries/citrate"]`), NOT in the base `tauri.conf.json`.

**Why an overlay and not the base config:** `tauri-build` (the `build.rs` step)
validates every `externalBin` path *on every `cargo build`* — so putting it in
the base config would fail plain `cargo build` / `cargo test` in CI whenever the
(uncommitted, ~20 MB) binary is absent. The overlay is merged only at packaging
time via `--config`, when the binary has been copied in:

```bash
npx tauri build --config src-tauri/tauri.bundle-node.conf.json
```

At runtime the app resolves the node either from the bundled resource dir
(`node::resolve_node_bin`) or from the `CITRATE_NODE_BIN` override, so dev and
tests never need the bundle.

Tauri resolves the platform binary by target-triple suffix, so this directory
must contain, per build host:

```
binaries/citrate-aarch64-apple-darwin     # Apple Silicon macOS
binaries/citrate-x86_64-apple-darwin       # Intel macOS
binaries/citrate-x86_64-unknown-linux-gnu  # Linux
binaries/citrate-x86_64-pc-windows-msvc.exe# Windows (out of beta scope, O-4)
```

**The binaries are NOT committed** (each is ~20+ MB) — they are `.gitignore`d.
Only this README is tracked.

## Getting a binary for local dev / packaging

Build it from the pinned citrate-chain source (bin name `citrate`, package
`citrate-node`) and copy it here with the target-triple suffix:

```bash
# from citrate-chain (the source pin is tracked via a Rule-12 [[drift]] entry on
# citrate-federation planset/core-beta-wiring):
cargo build --release --bin citrate
TRIPLE=$(rustc -vV | sed -n 's/host: //p')
cp target/release/citrate \
   /path/to/citrate-core/src-tauri/binaries/citrate-$TRIPLE
```

## Running without a bundle (dev + tests)

For `tauri dev` and headless runs you can bypass the bundle entirely by pointing
the app at an already-built binary:

```bash
export CITRATE_NODE_BIN=/absolute/path/to/citrate
```

`node::resolve_node_bin` honours `CITRATE_NODE_BIN` first, then falls back to the
bundled resource dir.

## CI cross-build (S7)

Per-platform cross-compilation + notarization of these sidecar binaries is an
**S7** release-hardening item, not part of C1.1. Until then, packaging is a
per-host manual copy as above.
