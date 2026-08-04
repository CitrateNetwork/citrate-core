---
created: 2026-07-13
branch: feat/core-c1-1-node
author: Claude Fable 5, directed by @SaulBuilds
status: active
---

# src-tauri/binaries — bundled sidecars: citrate-node (D-C1-1) + node-agent (C1.2) + mem-mcp (C3)

Three sidecars are bundled as **Tauri `externalBin`s**: the `citrate` node (C1.1),
the `node-agent` (C1.2), and the `mem-mcp` memory daemon (C3). All externalBin
declarations live in a SEPARATE overlay config, `tauri.bundle-node.conf.json`
(`bundle.externalBin: ["binaries/citrate", "binaries/node-agent", "binaries/mem-mcp"]`),
NOT in the base `tauri.conf.json`.

**Why an overlay and not the base config:** `tauri-build` (the `build.rs` step)
validates every `externalBin` path *on every `cargo build`* — so putting it in
the base config would fail plain `cargo build` / `cargo test` in CI whenever the
(uncommitted, ~20 MB) binary is absent. The overlay is merged only at packaging
time via `--config`, when the binary has been copied in:

```bash
npx tauri build --config src-tauri/tauri.bundle-node.conf.json
```

At runtime the app resolves each sidecar from the bundled resource dir
(`node::resolve_node_bin` / `agent::resolve_agent_bin` / `memory::resolve_mem_mcp_bin`)
or from an override env var (`CITRATE_NODE_BIN` / `CITRATE_NODE_AGENT_BIN` /
`CITRATE_MEM_MCP_BIN`), so dev and tests never need the bundle.

## Producing the `citrate` node sidecar — use the script

```bash
../../scripts/build-sidecar.sh                            # host triple
../../scripts/build-sidecar.sh --target aarch64-apple-darwin
```

**Do not hand-copy a `citrate` binary into this directory.** Chain 40204 was
re-rolled 2026-08-04 and both consensus activation heights went to 0
(citrate-chain PR #157: `VALUE_TRANSFER_ACTIVATION_HEIGHT` 300_000 → 0,
`MERGE_DEPTH_ACTIVATION_HEIGHT` 100_000 → 0). A binary built from an older
citrate-chain **silently forks the chain**: it connects, syncs and reports
healthy while computing different state roots below height 300,000 and accepting
merge blocks the fleet rejects.

`build-sidecar.sh` reads those two constants out of the citrate-chain source
before building and refuses to proceed if either is non-zero, so a stale checkout
fails fast instead of shipping a forking app. Binaries do NOT cross
architectures — build a Mac sidecar on a Mac.

Full instructions, macOS targets and the state-root parity check that proves you
are not on a fork: [`BUILDING.md`](../../BUILDING.md).

**mem-mcp is SPAWNED, not a Cargo dependency** — no `mem-*` workspace crate
appears in `src-tauri`'s `cargo tree` (the lean-tree invariant). The heavy
rocksdb + transformer build and the ~440 MB bge embedding model are an **S7**
bundle item; until then the memory daemon is bundled/resolved like the other two
sidecars and its live graph is a documented proof (`scripts/c3_live_proof.sh`).

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
binaries/mem-mcp-aarch64-apple-darwin        # Apple Silicon macOS (mem-mcp)
binaries/mem-mcp-x86_64-apple-darwin         # Intel macOS (mem-mcp)
binaries/mem-mcp-x86_64-unknown-linux-gnu    # Linux (mem-mcp)
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

The **mem-mcp** memory daemon (bin `mem-mcp`, package `mem-mcp`, from
citrate-memories), built with the `rocksdb,transformer` features:

```bash
# from citrate-memories:
cargo build --release -p mem-mcp --bin mem-mcp --features rocksdb,transformer
TRIPLE=$(rustc -vV | sed -n 's/host: //p')
cp target/release/mem-mcp \
   /path/to/citrate-core/src-tauri/binaries/mem-mcp-$TRIPLE
```

It used to be the `mcp_serve` **example** (`target/release/examples/mcp_serve`);
that target no longer exists — it was promoted to a real bin in
citrate-memories#15. The old command fails with "no example target named
`mcp_serve`".

**Build it from `ff12cab` (`feat/bge-bundle-and-fresh-store-bootstrap`), not from
`main`.** `memory.rs` spawns the daemon with `CITRATE_BGE_MODEL_DIR` (load the
bundled BGE weights offline) and `CITRATE_MEM_EMBED=bge` (force BGE on a store
with no embedder yet). Both env vars exist only on that branch. A `main` build
ignores them, silently falls back to the `HashingEmbedder`, and a fresh store is
then **permanently** locked to lexical vectors — semantic recall is dead and
nothing reports an error. Re-check this once that branch merges.

## Running without a bundle (dev + tests)

For `tauri dev` and headless runs bypass the bundle by pointing the app at
already-built binaries:

```bash
export CITRATE_NODE_BIN=/absolute/path/to/citrate
export CITRATE_NODE_AGENT_BIN=/absolute/path/to/node-agent
export CITRATE_MEM_MCP_BIN=/absolute/path/to/mcp_serve
```

`resolve_node_bin` / `resolve_agent_bin` / `resolve_mem_mcp_bin` honour the
override env first, then fall back to the bundled resource dir.

## CI cross-build (S7)

Per-platform cross-compilation + notarization of these sidecar binaries is an
**S7** release-hardening item, not part of C1.x. CI cannot build the node-agent
(heavy, private repo), so its cross-build joins the node's at S7. Until then,
packaging is a per-host manual copy as above.
