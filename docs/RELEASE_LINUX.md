---
created: 2026-09-01
branch: grow/tri-platform-release
author: Claude Opus 4.8, directed by @SaulBuilds
status: active
---

# Building the citrate-core Linux light client

This is the runbook for producing the **Linux light client** (AppImage + `.deb`)
that the website serves at `citrate.ai/download/linux`. "Light" = the 4.7 GB Gemma
GGUF is **not** bundled; the app downloads it on first run (identical behaviour to
the Mac light DMG). See `RELEASE.md` for the Mac path and the shared release
contract; this doc covers only what differs on Linux.

The Linux packaging config is **`src-tauri/tauri.bundle-linux.conf.json`** — the
Linux twin of `tauri.bundle-lite.conf.json`. It declares the same eight sidecars
and the same resources as the Mac light build, minus the `.gguf`, and targets
`appimage` + `deb`.

## Golden rule (why this is not `npm run tauri build`)

`git clone && npm run tauri build` produces an app **with no node**, and a wrong
node **silently forks chain 40204**. Every build host must produce its own
sidecars from pinned source. The `citrate` node sidecar in particular is gated:
`scripts/build-sidecar.sh` refuses to build from a citrate-chain checkout that
predates the 2026-08-04 re-roll (`VALUE_TRANSFER_ACTIVATION_HEIGHT` and
`MERGE_DEPTH_ACTIVATION_HEIGHT` must both read `0`). Do not hand-copy a `citrate`
binary from anywhere. Read `BUILDING.md` §1 once before your first build.

**Binaries do not cross architectures.** Build the Linux artefacts on a Linux
x86_64 host (a clean Ubuntu 22.04 box or CI runner). The target triple is
`x86_64-unknown-linux-gnu`; every sidecar filename carries that suffix.

## 0. Host prerequisites (Ubuntu 22.04 / Debian 12)

```bash
sudo apt-get update && sudo apt-get install -y \
  build-essential curl wget file pkg-config libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev

# Rust (pinned toolchain to match the Mac build)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
rustup target add x86_64-unknown-linux-gnu

# Node 20 + the repo deps
#   (nvm or your distro's node20; then:)
cd citrate-core && npm install
```

AppImage tooling (`linuxdeploy`, `appimagetool`) is fetched automatically by the
Tauri bundler; the host just needs `wget`/`file` and network access on first run.

## 1. Lay out the sibling source repos

Clone next to `citrate-core` (the scripts default to `../<repo>`):

```
work/
├── citrate-core            # this repo
├── citrate-chain           # → citrate node   (bin: citrate)
├── citrate-node-agent      # → node-agent
├── citrate-memories        # → mem-mcp   (branch ff12cab, features rocksdb,transformer)
├── citrate-comms           # → comms-member-daemon
├── citrate-cluster         # → cluster-daemon
└── citrate-agent-runtime   # → hermes (agent-sidecar)
```

Check out each at the SHA pinned in `citrate-federation/manifest.toml` (fail
closed — fetch the exact object, do not `git pull` a pin forward).

## 2. Build the eight sidecars for `x86_64-unknown-linux-gnu`

Four have wrapper scripts (they run the correct build + provenance stamp). They
default to the sibling source dir, so with the layout above you only pass
`--target`. Run each on the Linux host so it produces the Linux triple natively:

```bash
cd citrate-core
TRIPLE=x86_64-unknown-linux-gnu
scripts/build-sidecar.sh        --target "$TRIPLE"   # citrate node (gated; --chain to override dir)
scripts/build-comms-daemon.sh   --target "$TRIPLE"   # comms-member-daemon (--comms to override)
scripts/build-cluster-daemon.sh --target "$TRIPLE"   # cluster-daemon      (--cluster to override)
scripts/build-hermes.sh         --target "$TRIPLE"   # hermes/agent-sidecar (--agent-runtime to override)
```

Note `build-sidecar.sh` refuses a host≠target mismatch unless the artefact for
`$TRIPLE` already exists — which is exactly why you run this ON the Linux box
(host == target), not cross from a Mac.

The remaining four are plain `cargo build` / third-party binaries. Copy each with
the target-triple suffix into `src-tauri/binaries/` (provenance in
`src-tauri/binaries/README.md`):

```bash
TRIPLE=x86_64-unknown-linux-gnu
DEST=citrate-core/src-tauri/binaries

# node-agent — from citrate-node-agent
( cd ../citrate-node-agent && cargo build --release --bin node-agent )
cp ../citrate-node-agent/target/release/node-agent "$DEST/node-agent-$TRIPLE"

# mem-mcp — from citrate-memories @ ff12cab, rocksdb+transformer features
( cd ../citrate-memories && cargo build --release -p mem-mcp --bin mem-mcp --features rocksdb,transformer )
cp ../citrate-memories/target/release/mem-mcp "$DEST/mem-mcp-$TRIPLE"

# llama-server — llama.cpp Linux x64 build (match the version bundled on Mac;
#   see src-tauri/llama/ for the pinned LICENSE/sha256). Build from source or use
#   the official ggml-org/llama.cpp release for linux-x64, then:
cp /path/to/llama-server "$DEST/llama-server-$TRIPLE"

# ipfs — kubo daemon for linux-amd64 (dist.ipfs.tech), same major as the Mac bundle:
cp /path/to/kubo/ipfs "$DEST/ipfs-$TRIPLE"

chmod 755 "$DEST"/*-"$TRIPLE"
```

Sanity-check the set before packaging:

```bash
ls src-tauri/binaries/*-x86_64-unknown-linux-gnu    # expect 8 files
```

Missing any → `npx tauri build` fails honestly (the overlay validates every
`externalBin` path at packaging time). That is the intended behaviour, not a bug.

## 3. Package the AppImage + deb

```bash
cd citrate-core
npx tauri build --config src-tauri/tauri.bundle-linux.conf.json
```

Artefacts land under `src-tauri/target/release/bundle/`:

```
bundle/appimage/citrate-core_0.1.0_amd64.AppImage
bundle/deb/citrate-core_0.1.0_amd64.deb
```

## 4. Rename to the release contract + verify

The website + updater expect stable, platform-tagged asset names (mirroring
`Citrate-Core-macos-arm64.dmg`):

```bash
V=0.1.0-alpha.1
cp bundle/appimage/citrate-core_0.1.0_amd64.AppImage  Citrate-Core-linux-x86_64.AppImage
cp bundle/deb/citrate-core_0.1.0_amd64.deb            Citrate-Core-linux-x86_64.deb
shasum -a 256 Citrate-Core-linux-x86_64.* > Citrate-Core-linux-x86_64.sha256
chmod +x Citrate-Core-linux-x86_64.AppImage
```

Smoke test on a **second, clean** Linux box (not the build host — that catches the
Mac liblzma-style "works only where it was built" class of bug):

```bash
./Citrate-Core-linux-x86_64.AppImage
```

Expect: app launches, Node view begins cold-sync from `rpc.citrate.ai`, and the
Models view shows the first-run "download model" state (no bundled GGUF). The
headless equivalent of the Mac cold-sync gate is the sidecar sync check — run the
node sidecar the way the app does (config + consensus env, `BUILDING.md` §2–3) and
confirm it climbs past height 2000 rather than wedging.

## 5. Publish

Attach both assets to the GitHub release `v0.1.0-alpha.1` alongside the Mac DMG:

```bash
gh release upload v0.1.0-alpha.1 \
  Citrate-Core-linux-x86_64.AppImage \
  Citrate-Core-linux-x86_64.deb \
  Citrate-Core-linux-x86_64.sha256
```

Then ping the DGX team in `docs/GROW_COORDINATION.md` to point
`citrate.ai/download/linux` → `releases/latest/download/Citrate-Core-linux-x86_64.AppImage`
(the `.deb` link can sit beside it for apt users). That closes the Linux half of
the tri-platform download contract.

## Deferred (owner / infra gated)

- **Auto-update.** The Linux config ships `createUpdaterArtifacts: false`. Flip it
  to `true` — and add the `.sig`/`latest.json` publish step — only once
  `TAURI_SIGNING_PRIVATE_KEY` is provisioned (same key that gates the Mac
  updater). Until then Linux updates are manual re-download, like the Mac alpha.
- **`.deb` signing / apt repo.** Out of scope for the alpha; the AppImage is the
  primary Linux artefact.
- **Model CDN.** When the DGX model bucket is live, set `CITRATE_MODEL_URL` in the
  build env so first-run pulls from the Citrate mirror instead of Hugging Face.
