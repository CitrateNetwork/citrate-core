---
created: 2026-11-24
branch: feat/059-windows
author: Larry Klosowski (Windows lane, Citrate Core 0.5.9; Claude Opus 5.5)
status: active (bring-up record; nothing published)
related: docs/release/WINDOWS_BUILD.md (decisions), docs/RELEASE_WINDOWS.md (manual runbook), .github/workflows/release-windows.yml (the build)
---

# Citrate Core 0.5.9 for Windows x64

This is what the hosted Windows build makes, what a member sees, and what is still open. The build is
`.github/workflows/release-windows.yml` on GitHub-hosted Windows runners. It publishes nothing.
Distribution, signing and the update feed are owner decisions; see the last section.

## What the build is

- **Installer:** `Citrate-Core_0.5.9_x64-setup.exe`, NSIS, per-user (`currentUser`: no admin prompt).
  It installs to `%LOCALAPPDATA%\Citrate Core` and downloads the WebView2 runtime during install
  when the PC lacks it. It is unsigned: there is no Windows code-signing certificate yet.
- **Source:** the 0.5.9 release commit c3f10793 plus Windows build plumbing only.
- **Sidecars:** every sidecar is built on a GitHub-hosted windows-2022 runner, at its pin in
  `release/components.json`, with the release build scripts. Three of them build from Windows-only
  revs, each the macOS and Linux rev plus one Windows fix. Each rev is kept reachable by a `pin/` branch:

  | sidecar | Windows rev | macOS / Linux rev | why |
  |---|---|---|---|
  | node (`citrate`) | citrate-chain 98c513ec (`pin/windows-0.5.9-node`) | 68434069 | #492's open-file-limit code did not compile on Windows; #494 gates it with cfg(unix), touching no consensus path |
  | cluster-daemon | citrate-cluster b014f70a (`pin/windows-0.5.9-cluster-daemon`) | 755cf217 | bound the wrong named-pipe name on Windows (#19) |
  | mem-mcp | citrate-memories 451857e0 (main after #34 and #36) | 5aeda317 | bound the wrong named-pipe name on Windows (#36; #34 clears the zip advisories) |

- **No VC++ runtime needed:** every executable imports only DLLs that ship with Windows 10 and 11
  (the cargo sidecars and llama-server link the C runtime statically, and the app links the VC
  runtime statically). A PC without the Visual C++ Redistributable runs it.
- **Node consensus:** the Windows node reports consensus fingerprint 0x2b0f96ee…, not the cut's
  0xa2f16115…, because the fingerprint hashes the git commit together with every consensus input. Recomputed
  with the base commit (`scripts/ci/consensus-fingerprint.py`, a required smoke check), it is
  exactly 0xa2f16115438794f0410b187c99a14e34: every consensus input is the macOS and Linux node's.
  Nothing compares the fingerprint at run time: not the app, the P2P handshake, staking or block
  production. A fleet view that lists `gitSha` or the fingerprint shows Windows nodes as
  98c513ec47d8 / 0x2b0f96ee…; that is expected.

## What a member sees

1. **The download.** The browser may warn that the file "isn't commonly downloaded". In Edge: `...`,
   then Keep. In Chrome: Keep.
2. **SmartScreen, every first run of an unsigned installer:** a blue "Windows protected your PC"
   dialog, "Microsoft Defender SmartScreen prevented an unrecognized app from starting. Running this
   app might put your PC at risk." Click **More info**, which shows "Publisher: Unknown publisher",
   then **Run anyway**. Organisations that block unknown publishers by policy cannot install it until
   it is signed.
3. **The installer** runs for the current user with no UAC prompt. If WebView2 is missing (older
   Windows 10), it downloads it, which needs internet access during install.
4. **First launch: two Windows Defender Firewall prompts**, for `citrate.exe` (the node, P2P on
   TCP 30303) and `ipfs.exe` (TCP/UDP 4001). Both listen on all interfaces for inbound peers.
   - Allowing needs an administrator. A standard user can click Cancel.
   - After Cancel, outbound connections still work: the node still syncs from the boot peers.
     Only inbound peers are refused.
   - Local RPC (127.0.0.1:8545 and 8546) is loopback only and causes no prompt.
5. **Sync:** the node starts within seconds, connects to boot1 to boot3 and rpc.citrate.ai, and
   reports "catching up" while it applies the chain. The network is at about 528k blocks, so a cold
   sync takes hours, as on other platforms.
6. **Uninstall** (Settings, Apps, Citrate Core) removes the program files and the uninstall entry.
   The member's data (`%APPDATA%\ai.citrate.core`: wallet vault, node database, settings) stays, as
   on macOS and Linux.

## The pinned build

Release-windows run 45525459608 built every sidecar from scratch, then the installer, then the smoke,
all green. It produced:

- the installer `Citrate-Core_0.5.9_x64-setup.exe`, sha256
  `17a3852fe8167cf29cb9f123fe47798371fa8a5d6717b52a78d32cf25e3883aa`;
- the six sidecars, pinned in `release/components.json` and `src-tauri/runtime-deps.sha256`;
- for each sidecar, a new runtime-deps asset `<bin>-x86_64-pc-windows-msvc-<rev12>` and its
  `.PROVENANCE.txt`, both uploaded and verified by download.

The Windows node does not rebuild byte for byte from run to run (as on Linux, chain#427), so the pin
is this run's bytes.

## Smoke test on the hosted runners

The `smoke` job runs on fresh windows-2022 and windows-2025 runners. Windows Server 2022 shares the
Windows 10 21H2 code base, and Server 2025 shares Windows 11 24H2's. Run 45472309246 tested the
installer of run 45466078872, and run 45525459608 tested the pinned build. These passed on both:

- silent per-user install (48 to 54 s);
- launch, and the node up in 10 to 12 s;
- chain id 0x9d0c and the r1007 genesis 0xcf686345…6631;
- established connections to all four r1007 boot peers;
- applied height moving;
- quit (window close), which ended the app and every sidecar;
- the memory daemon listening on the per-install pipe the app connects to (and not on the old bare name);
- silent uninstall.

The node log had no WARN or ERROR line. The smoke does not exercise the UI, Groups (cluster-daemon), chat (comms-member-daemon), Hermes or the
model download: those start on demand from the UI. The Groups and chat daemons use the same
per-install pipe naming as the memory daemon (citrate-cluster#19, citrate-comms#24), but that is not
proven on Windows at run time.

## Windows versions

- **Windows 11 x64:** supported.
- **Windows 10 x64:**
  - Version 1803 or later, the WebView2 floor. Rust's MSVC target also requires Windows 10.
  - Any supported 21H2 or 22H2 machine qualifies.
  - The local IPC uses named pipes, not AF_UNIX, so nothing needs a newer Windows 10 build.
  - Windows 10 reached end of support in October 2025, so members on it should still update Windows.
- **Windows 11 on ARM:** the x64 build runs under Prism emulation. Untested. Nothing in the sidecars
  is x64-only beyond the CPU code paths:
  - llama-server loads the best of its CPU variants at run time;
  - RocksDB and OpenSSL use baseline x86-64 code;
  - expect slower local inference and sync.
- **Windows 10 on ARM** cannot run x64 apps (it emulates 32-bit x86 only), so it is not supported.
- **Windows 7, 8 and 8.1:** not supported.

## Updates, signing and distribution (owner decisions; nothing is done yet)

- **Signing.** Recommended: Azure Trusted Signing, or an OV/EV certificate, which takes days. Wire it
  through `bundle.windows.signCommand`, so every exe and the installer are signed in the build. Until
  then, every member goes through SmartScreen as described above.
- **In-app updates.** Windows is not in the update feed, so a Windows install is offered no update.
  The app treats "no platform in the feed" as up to date (`src/shell/updater.ts`, `isNoPlatformInFeed`).
  A forced minimum version in a later feed does not lock Windows out: the signed policy is only applied
  after the updater returns an update for this platform. To add Windows:
  1. build with `createUpdaterArtifacts: true`;
  2. sign the `-setup.exe` with the same updater key (minisign, `npx tauri signer sign`, on the release Mac, which holds the key);
  3. add `windows-x86_64` to `scripts/updater-feed.mjs` `PLATFORM_FILES`, pointing at that file;
  4. merge its fragment into `latest.json` at the next release ceremony.

  The updater runs the NSIS installer itself. An update is downloaded by the app, not a browser, so
  it carries no mark of the web and gets no SmartScreen prompt.
- **Distribution today**, if the owner says go: the workflow artifact (7 days), mirrored to the CDN
  under a new name, with its sha256 and this page's install steps.
