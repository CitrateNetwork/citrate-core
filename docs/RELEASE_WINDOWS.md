---
created: 2026-09-01
branch: grow/tri-platform-release
author: Claude Opus 4.8, directed by @SaulBuilds
status: active (runbook for the Windows-side agent)
---

# Building the citrate-core Windows light client — partner runbook

This is a **self-contained runbook** for the Windows-side agent. It assumes no
prior context on this repo. Follow it top to bottom on a Windows 11 x64 machine to
produce a Windows **light installer** (`.exe`, NSIS) matching the Mac and Linux
light builds. "Light" = the 4.7 GB Gemma model is **not** bundled; the app
downloads it on first run.

Windows was explicitly **out of scope** for the beta (marker O-4 in the repo), so
treat this as a first bring-up: expect to spend most of your time in Step 2
(sidecars), and report back anything that does not build cleanly rather than
patching around it silently.

The packaging config is committed for you: **`src-tauri/tauri.bundle-windows.conf.json`**.
It targets NSIS, declares the eight sidecars + the light resource set (no
`.gguf`), installs per-user (no admin prompt — friendlier for viral sharing), and
downloads the WebView2 runtime at install time if the machine lacks it.

## The one rule that matters most

A wrong `citrate` node binary **silently forks chain 40204** — it connects, syncs,
and looks healthy while computing different state. So:

- Build the node sidecar from a **current** `citrate-chain` checkout (the SHA
  pinned in `citrate-federation/manifest.toml`). `scripts/build-sidecar.sh` has a
  lineage gate that reads two consensus constants and refuses a stale source; on
  Windows you may need to run the equivalent check by hand (see Step 2).
- **Never** copy a `citrate.exe` from anywhere else.
- **Binaries do not cross architectures or OSes.** A Windows build must be made on
  Windows. Do not try to cross-compile the sidecars from Mac/Linux.

The two consensus constants that MUST both read `0` in your citrate-chain source:

| constant | file | required |
|---|---|---|
| `VALUE_TRANSFER_ACTIVATION_HEIGHT` | `core/execution/src/executor.rs` | `0` |
| `MERGE_DEPTH_ACTIVATION_HEIGHT` | `core/consensus/src/ghostdag.rs` | `0` |

If either is non-zero, your checkout predates the 2026-08-04 re-roll — update it
(`git fetch origin && git checkout main`) before building anything.

## 0. Host prerequisites (Windows 11 x64)

Install, in this order:

1. **Visual Studio 2022 Build Tools** with the "Desktop development with C++"
   workload (gives you the MSVC toolchain + Windows SDK). Required by Rust's
   `msvc` target and by llama.cpp.
2. **Rust** via `rustup` (https://rustup.rs). Confirm the target:
   ```powershell
   rustup default stable-x86_64-pc-windows-msvc
   rustc -vV        # host: should read x86_64-pc-windows-msvc
   ```
3. **Node 20 LTS** (https://nodejs.org) — `node -v` ≥ 20.
4. **Git**, **CMake**, and **7-Zip** (or tar) on `PATH`.
5. **WebView2 runtime** — usually already present on Win11; the installer bundles
   the bootstrapper anyway, so no action needed here.

```powershell
cd citrate-core
npm install
```

## 1. Lay out the sibling source repos

Clone next to `citrate-core`, each checked out at its manifest-pinned SHA:

```
work\
├── citrate-core            # this repo
├── citrate-chain           # → citrate.exe        (bin: citrate)
├── citrate-node-agent      # → node-agent.exe
├── citrate-memories        # → mem-mcp.exe   (features: rocksdb,transformer)
├── citrate-comms           # → comms-member-daemon.exe
├── citrate-cluster         # → cluster-daemon.exe
└── citrate-agent-runtime   # → hermes.exe    (package: agent-sidecar)
```

## 2. Build the eight sidecars for `x86_64-pc-windows-msvc`

Tauri resolves each sidecar by filename suffix, so every binary must land in
`citrate-core\src-tauri\binaries\` named `<name>-x86_64-pc-windows-msvc.exe`.

The repo's helper scripts are bash (`scripts/build-*.sh`) — run them under **Git
Bash** if you have it; otherwise run the underlying `cargo` commands directly in
PowerShell as shown below. The target triple is `x86_64-pc-windows-msvc`.

```powershell
$TRIPLE = "x86_64-pc-windows-msvc"
$DEST   = "citrate-core\src-tauri\binaries"

# --- citrate node (from citrate-chain) — VERIFY LINEAGE FIRST ---
# Confirm both constants read 0 (see table above), then:
cd ..\citrate-chain
cargo build --release --bin citrate --target $TRIPLE
Copy-Item target\$TRIPLE\release\citrate.exe "..\$DEST\citrate-$TRIPLE.exe"

# --- node-agent (from citrate-node-agent) ---
cd ..\citrate-node-agent
cargo build --release --bin node-agent --target $TRIPLE
Copy-Item target\$TRIPLE\release\node-agent.exe "..\citrate-core\src-tauri\binaries\node-agent-$TRIPLE.exe"

# --- mem-mcp (from citrate-memories, rocksdb+transformer) ---
cd ..\citrate-memories
cargo build --release -p mem-mcp --bin mem-mcp --features rocksdb,transformer --target $TRIPLE
Copy-Item target\$TRIPLE\release\mem-mcp.exe "..\citrate-core\src-tauri\binaries\mem-mcp-$TRIPLE.exe"

# --- comms-member-daemon (from citrate-comms) ---
cd ..\citrate-comms
cargo build --release -p comms-member-daemon --target $TRIPLE
Copy-Item target\$TRIPLE\release\comms-member-daemon.exe "..\citrate-core\src-tauri\binaries\comms-member-daemon-$TRIPLE.exe"

# --- cluster-daemon (from citrate-cluster) ---
cd ..\citrate-cluster
cargo build --release -p cluster-daemon --target $TRIPLE
Copy-Item target\$TRIPLE\release\cluster-daemon.exe "..\citrate-core\src-tauri\binaries\cluster-daemon-$TRIPLE.exe"

# --- hermes / agent-sidecar (from citrate-agent-runtime) ---
cd ..\citrate-agent-runtime
cargo build --release -p agent-sidecar --target $TRIPLE
Copy-Item target\$TRIPLE\release\citrate-agent-sidecar.exe "..\citrate-core\src-tauri\binaries\hermes-$TRIPLE.exe"
```

Third-party binaries — fetch the Windows x64 builds and copy them in:

```powershell
# llama-server — llama.cpp Windows x64 (ggml-org/llama.cpp releases). Match the
# version bundled on Mac (see citrate-core\src-tauri\llama\ for the pinned sha256).
Copy-Item path\to\llama-server.exe "citrate-core\src-tauri\binaries\llama-server-$TRIPLE.exe"

# ipfs — kubo for windows-amd64 (https://dist.ipfs.tech/kubo). Same major as the
# Mac bundle.
Copy-Item path\to\kubo\ipfs.exe "citrate-core\src-tauri\binaries\ipfs-$TRIPLE.exe"
```

Confirm all eight are present before packaging:

```powershell
Get-ChildItem citrate-core\src-tauri\binaries\*-x86_64-pc-windows-msvc.exe   # expect 8
```

If any are missing, `npx tauri build` fails on purpose — the packaging overlay
validates every sidecar path. That is the honesty gate, not a bug to work around.

### Known bring-up risks (report, don't paper over)

- The `citrate` node and some daemons were developed on Unix. If a crate fails on
  MSVC (e.g. a `unix`-only dependency, a path assumption, or rocksdb build flags),
  **stop and report the crate + error** in `docs/GROW_COORDINATION.md`. Do not
  swap in a stub or a non-MSVC target to get a green build — a fake node is worse
  than no Windows build.
- rocksdb (mem-mcp) on MSVC needs the C++ toolchain from Step 0; if it can't find
  a compiler, the VS Build Tools workload is incomplete.

## 3. Package the NSIS installer

```powershell
cd citrate-core
npx tauri build --config src-tauri\tauri.bundle-windows.conf.json
```

Output:

```
src-tauri\target\release\bundle\nsis\Citrate Core_0.1.0_x64-setup.exe
```

## 4. Code signing (Authenticode)

The alpha may ship **unsigned** — Windows SmartScreen will show a "Windows
protected your PC" prompt that the user clicks through via *More info → Run
anyway*. That is acceptable for the alpha but hurts trust and conversion, so sign
as soon as the org Authenticode cert is available (this is an owner-gated item —
the signing cert is tracked alongside the Mac/updater keys).

With a cert (`.pfx` or a hardware/EV token) available:

```powershell
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
  /f citrate-codesign.pfx /p <pfx-password> `
  "src-tauri\target\release\bundle\nsis\Citrate Core_0.1.0_x64-setup.exe"
signtool verify /pa "src-tauri\target\release\bundle\nsis\Citrate Core_0.1.0_x64-setup.exe"
```

(Tauri can also sign automatically during `tauri build` via the `windows.signCommand`
config field — wire that once the cert lands so signing is part of the build, not
a manual afterthought.)

## 5. Rename to the release contract + verify

```powershell
Copy-Item "src-tauri\target\release\bundle\nsis\Citrate Core_0.1.0_x64-setup.exe" `
          "Citrate-Core-windows-x86_64-setup.exe"
Get-FileHash Citrate-Core-windows-x86_64-setup.exe -Algorithm SHA256 |
  Out-File Citrate-Core-windows-x86_64-setup.exe.sha256
```

Install on a **second, clean** Windows machine (not the build host). Expect:

- Installer runs per-user with no admin prompt; app launches.
- **Node** view begins cold-sync from `rpc.citrate.ai` and climbs past height 2000
  (not stuck at height 0 / 0 peers — that would mean the bundled node config
  didn't ship).
- **Models** view shows the first-run "download model" state (no bundled GGUF).
- **Groups → Join**: paste a `https://citrate.ai/join/...` link and confirm the
  invite parses (this is the cluster-growth path the whole release exists for).

## 6. Hand off

Do **not** publish to the website yourself. Attach the signed (or, for the alpha,
unsigned) `.exe` + `.sha256` to the GitHub release the Mac/Linux team names, and
post in `docs/GROW_COORDINATION.md`:

- the artefact names + sha256,
- signed vs unsigned,
- any crate that failed to build on MSVC (per the risks above),
- the WebView2 / clean-machine smoke-test result.

The Mac/DGX team wires `citrate.ai/download/windows` → the release asset once your
artefact is verified. Windows auto-update stays off until the shared
`TAURI_SIGNING_PRIVATE_KEY` is provisioned (same gate as Mac/Linux).
