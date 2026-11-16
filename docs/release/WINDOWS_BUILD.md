---
created: 2026-11-12
branch: docs/053-windows-build
author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
status: active (hand-off brief for the Windows / Microsoft Store build, 0.5.3 on r1007)
related: docs/RELEASE_WINDOWS.md (the step-by-step sidecar and NSIS runbook), docs/release/053-NETWORK.md, docs/release/053-UPDATER.md
---

# Building Citrate Core for Windows and the Microsoft Store (0.5.3, r1007)

This brief is for the engineer building the Windows version (Kurt). It covers:

1. what to install;
2. which package format to submit to the Store;
3. how the r1007 network ends up inside the build;
4. how to prove the build is right;
5. what to hand back.

It does **not** repeat the sidecar build. That is [docs/RELEASE_WINDOWS.md](../RELEASE_WINDOWS.md),
steps 0 to 5. Follow that file for the commands, and this one for the decisions around them.

Wherever this file says **verify**, the statement is about current Microsoft Store or Partner Center
policy, or about tool behaviour that was not checked on a Windows machine for this release. Confirm
it against Microsoft's current documentation before relying on it. Do not ship on our word.

## 0. Facts this build must match

| Item | Value | Where it comes from |
|---|---|---|
| App identifier | `ai.citrate.core` | `src-tauri/tauri.conf.json` |
| Product name | `Citrate Core` | `src-tauri/tauri.conf.json` |
| Version | `0.5.3` | `src-tauri/tauri.conf.json` on the 0.5.3 release commit; the Windows build uses the same commit as macOS and Linux |
| Network | r1007, chain id 40204 (`0x9d0c`) | `src-tauri/network/profiles.json` (`active: r1007` after the cut) |
| Block 0 (genesis hash) | `0xcf68634502b4fca1e977ac8dd35aa006bb40c04208d5cd55f1c2b508bba96631` | `release/network-cut.json` `genesisHash`; live at `https://rpc.citrate.ai` since 2026-11-12T14:00Z |
| Chain source for the node (`citrate.exe`) | the cut's `chainRev` (the r1007 genesis commit, `a53bac64…` at the time of writing) | `release/network-cut.json` `chainRev` |
| Consensus fingerprint | the cut's `consensusFingerprint` | `release/network-cut.json`; the live fleet reported `0x5d26c4ab5b0be144d26916b9070a70a4` on 2026-11-12, so **verify** it equals the cut's value once the cut is filled |

Build only from the tagged 0.5.3 release commit. A commit from before the network cut is applied
still names r1006, which is the dead network, and the app would sync to nothing.

## 1. Prerequisites on Windows

Use a Windows 11 x64 machine that you control and that is not shared, because the build handles
signing material. The full list and commands are in [RELEASE_WINDOWS.md § 0](../RELEASE_WINDOWS.md#0-host-prerequisites-windows-11-x64):

- **Visual Studio 2022 Build Tools** with the "Desktop development with C++" workload (MSVC and the
  Windows SDK);
- Rust stable `x86_64-pc-windows-msvc`;
- Node 20 LTS or later;
- Git, CMake, 7-Zip.

Add these for this release:

- **Git Bash**. The repository's checks are bash scripts, run here exactly as on Linux:
  `scripts/check-network-profile.sh`, `scripts/ci/verify-runtime-deps.sh` and
  `python3 scripts/network-cut.py`.
- **Python 3.11 or later** on `PATH`, for `network-cut.py`.
- **`gh`**, signed in with read access to `CitrateNetwork/citrate-core` (for the `runtime-deps`
  assets) and read access to `CitrateNetwork/citrate-chain` at the cut's `chainRev`.
- **Windows SDK `signtool.exe`**, whichever package format you choose (section 2).
- For the MSIX route only: the **MSIX Packaging Tool** or the SDK's **`makeappx.exe`** (section 2.2).
  **Verify** which Microsoft currently recommends for packaging an existing Win32 app.
- A **second, clean Windows 11 machine or VM** for the first-run test (section 4.3). It must never
  have run Citrate Core before.

## 2. Package format: MSIX, or a signed MSI/EXE

Tauri 2 builds two Windows installer formats: **NSIS** (`.exe`, which our config already uses:
`src-tauri/tauri.bundle-windows.conf.json`, per-user, with an installer hook that CI tests) and
**MSI** (WiX). Tauri has no MSIX target of its own (**verify** for the Tauri version in
`package-lock.json`, currently `@tauri-apps/cli` 2.11.x). The Store accepts both routes below. They
differ in who signs, who hosts the file, and who updates the app.

### 2.1 MSI or EXE submitted as a Win32 app (the NSIS installer we already build)

- **Signing.** The installer and the binaries in it must be Authenticode-signed with a code-signing
  certificate that chains to a root in the **Microsoft Trusted Root Program**: an OV or EV
  certificate bought from a public CA, or Azure Trusted Signing. **Verify** the current certificate
  requirement in the Partner Center docs for MSI/EXE apps. Microsoft does not sign this route for us.
  The certificate is an **owner-gated purchase** that does not exist yet. Ask the owner, and do not
  self-sign.
- **Hosting.** You give Partner Center an **HTTPS URL** to the installer, and the Store downloads it
  from there. The file at that URL must never change: a new version gets a new URL (**verify**).
  Under the owner decision of 2026-11-12, our installers are **private** objects in Spaces
  `citrate-cdn` (`downloads/<file>`), so this route needs an owner decision on a URL the Store can
  reach.
- **Silent install.** The Store runs the installer with silent switches. NSIS takes `/S`. Record the
  switch in Partner Center (**verify** which fields are required).
- **WebView2.** Our NSIS config downloads the WebView2 bootstrapper during install if the runtime is
  missing (`webviewInstallMode: downloadBootstrapper`). **Verify** whether Store policy for Win32
  installers allows an installer that downloads components. If it does not, switch to
  `offlineInstaller` or `embedBootstrapper` in a Store-only overlay; do not change the shared config.
- **Updates.** Updates come from us, not the Store, so the in-app updater stays in charge. In 0.5.3
  the feed lists no Windows platform, so a Windows install is offered no update. That is safe, but
  the Windows build then has no update path until we add a `windows-x86_64` entry (see section 3.3).

### 2.2 MSIX submitted to Partner Center (Microsoft signs it)

- **Signing.** You upload an unsigned MSIX, or one signed with a test certificate. **Microsoft
  re-signs the package** with the Store certificate when it passes certification, so no purchased
  certificate is needed for Store distribution (**verify**). A package sideloaded outside the Store
  still needs our own trusted signature.
- **Identity.** The manifest's `Identity Name` and `Publisher` must be exactly the values Partner
  Center shows after you reserve the app name. Copy them from Partner Center → Product identity,
  and never invent them.
- **A full-trust desktop app.** The manifest must declare the `runFullTrust` restricted capability
  with `EntryPoint="Windows.FullTrustApplication"`, because the app starts eight sidecar processes
  (node, agent, memory, model server, IPFS, comms, cluster, Hermes). **Verify** that a restricted
  capability needs a written justification at submission (it usually does). The node listens for
  peers on TCP 30303 and serves local RPC, so also declare `internetClientServer` and
  `privateNetworkClientServer` (**verify** the exact capabilities a full-trust app needs for
  inbound connections).
- **The container changes where files go.** An MSIX app's install folder is read-only, and writes
  to `%AppData%`/`%LocalAppData%` may be redirected into the package's private store (**verify**
  the current virtualisation rules for full-trust MSIX). The node keeps a large chain database, and
  the app downloads a 4.7 GB model on first run. Check that both land where the app expects, and
  that uninstall behaves as intended (a redirected store is deleted on uninstall).
- **WebView2.** An MSIX cannot run the WebView2 bootstrapper at install time. Windows 11 ships the
  runtime, but on Windows 10 it must already be present, or be a declared dependency (**verify**).
- **Updates.** The Store updates MSIX apps. A Store-installed app must not replace itself, so a
  Store MSIX needs the in-app updater **off** (**verify** the policy wording). That is a build
  change, made red-first in its own PR, and is not in 0.5.3.
- **Packaging.** Package the NSIS build's installed output, or `target\release` plus the binaries and
  resources, with the MSIX Packaging Tool or with `makeappx pack` and a hand-written
  `AppxManifest.xml`. Commit the manifest under `src-tauri/windows/` in a PR, so the identity and
  capabilities are reviewed and never edited on one machine only.

### 2.3 Content policy to check before either route

**Verify** the Microsoft Store Policies that apply to an app that runs a blockchain node, holds a
wallet, and contributes compute. In particular, check the policy on **cryptocurrency mining** and
on apps that use device resources for third parties, and the financial-services and age-rating
questions. If a policy rules out a Store listing, stop and report it; do not reword the listing
to get past it.

### 2.4 Recommendation, and what it rests on

The NSIS EXE (2.1) is the installer CI already builds and tests, and it runs the app exactly as on
the other platforms, outside a container. It needs a purchased code-signing certificate and a
Store-reachable installer URL, and both are owner decisions. MSIX (2.2) needs no certificate, but it
puts eight sidecars, a P2P listener and a large data directory inside a container we have never
tested. Unless the owner rules out the certificate, prefer 2.1. Either way, report back before
submitting.

## 3. Getting the r1007 network into the build

The network is not a setting you choose at build time. It is compiled in from the release commit.

### 3.1 What carries it

On the 0.5.3 release commit, after `scripts/network-cut.py apply` (see
[053-NETWORK.md](053-NETWORK.md)), these files carry the network:

- `src-tauri/network/profiles.json`: `active: r1007`, the genesis hash, the bootnodes, the
  `nodeCheckpoint` values rendered into the node's `node.toml`, and the address book digest;
- `src-tauri/addresses/40204.canonical.json`: the r1007 contract address book;
- `src-tauri/Cargo.toml` and `kit/Cargo.toml`: the citrate-chain crate revisions;
- `release/components.json`: the citrate-node source revision.

Build from that commit, unmodified. Do not edit any of these files on the Windows machine.

### 3.2 The node binary

Build `citrate.exe` from citrate-chain **at the cut's `chainRev`, and nothing else**. The steps are
in [RELEASE_WINDOWS.md § 2](../RELEASE_WINDOWS.md#2-build-the-eight-sidecars-for-x86_64-pc-windows-msvc).

The cut pins node bytes for macOS arm64 and Linux only. There is no Windows pin yet. Record
`citrate.exe`'s sha256 and its `chainRev` in your hand-back (section 5), so the next cut can pin it.

A node built from another revision connects and looks healthy while computing different state.
See "The one rule that matters most" in RELEASE_WINDOWS.md.

### 3.3 The updater feed

The 0.5.3 feed (`https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/latest.json`)
lists `darwin-aarch64`, `linux-x86_64` and `linux-x86_64-deb` only. A Windows install finds no entry
and is offered no update (tauri-plugin-updater 2.10.1, `TargetsNotFound`).

Do not add a Windows entry yourself. Adding one means signing a Windows update file with the release
updater key, which never leaves the release Mac. Which route updates Windows (the feed for 2.1, the
Store for 2.2) is decided with the owner first.

### 3.4 Checks to run before packaging (Git Bash, in the citrate-core checkout)

```bash
python3 scripts/network-cut.py check --release                  # the cut is complete and applied; asks rpc.citrate.ai
scripts/check-network-profile.sh --release --require-active r1007
node scripts/check-staged-corpus.mjs --pins src-tauri/runtime-deps.sha256
```

All three must pass. If one fails, report the output. Do not edit files to make it pass.

## 4. Verifying the build

### 4.1 CI on the release commit

Run these three workflows with `workflow_dispatch` on the release commit, and attach each run's link:

| Workflow | Job | What has to pass |
|---|---|---|
| `lifecycle-platforms` | `core-kit supervisor tests (windows-latest)` | `test result: ok. 56 passed`, plus the K6 compiled-source check step |
| `src-tauri lifecycle platforms` | `src-tauri lifecycle (Windows)` | shutdown 2, owned cleanup 9, comms 34, cluster 22 (total 67) |
| `windows-installer-hook` | `windows installer hook (NSIS build + test)` | the hook builds and `test-stop-own-sidecars.ps1` passes |

The expected counts are the ones in the workflow files on the release commit. Read them there, in
case they have changed.

### 4.2 The artefacts

For each file you hand back, give the sha256:

```powershell
Get-FileHash <file> -Algorithm SHA256
```

Hash all eight sidecars, the installer (or `.msix`), and `citrate.exe` separately.

For the signed route (2.1), also run `signtool verify /pa /v <file>` on the installer and on
`citrate.exe`, and keep the output.

### 4.3 First run on the clean machine

Install with the same switches the Store will use. Then:

1. The app starts, and the Node view begins syncing.
2. Find the local node's RPC address in the rendered `node.toml`, under `[rpc] listen_addr`, in the
   app's data directory. Ask the node:
   ```powershell
   $b = '{"jsonrpc":"2.0","id":1,"method":"citrate_nodeIdentity","params":[]}'
   Invoke-RestMethod -Uri http://<listen_addr> -Method Post -ContentType application/json -Body $b
   ```
   The answer must show:
   - `chainId` `0x9d0c`;
   - `genesisHash` `0xcf68634502b4fca1e977ac8dd35aa006bb40c04208d5cd55f1c2b508bba96631`;
   - `consensusFingerprint` equal to the cut's;
   - `syncState.state` reaching `synced`, with `appliedHeight` within a few blocks of
     `eth_blockNumber` on `https://rpc.citrate.ai`.

   Any other genesis means the build is wrong. Stop, and do not submit it.
3. The Models view shows the first-run download state (no model is bundled).
4. Quit the app from its menu. No `citrate.exe` or other sidecar may still be running: check Task
   Manager, or `Get-Process citrate, node-agent, mem-mcp, llama-server, ipfs, comms-member-daemon,
   cluster-daemon, hermes`.
5. Uninstall, and record what is left in `%AppData%` and `%LocalAppData%`.

## 5. What to hand back

Post all of the following in the release thread. Do not publish, upload to the website, or submit
to the Store before the owner has reviewed it.

- The release commit the build is from, and the `chainRev` `citrate.exe` was built from.
- The route you took (2.1 or 2.2), and why.
- File names and sha256 of the installer or `.msix`, `citrate.exe` and the other seven sidecars.
- Signed or unsigned. For 2.1, the `signtool verify` output and the certificate subject. For 2.2,
  the Partner Center identity values used.
- The three CI run links (4.1), all green.
- The first-run results (4.3): the `citrate_nodeIdentity` output, the time to `synced`, the
  sidecar-exit check, and what uninstall left behind.
- Each **verify** item in this file that you checked, with what Microsoft's current documentation
  says and a link to it.
- Anything that failed to build on MSVC, by crate and error, unpatched (RELEASE_WINDOWS.md, "Known
  bring-up risks").
