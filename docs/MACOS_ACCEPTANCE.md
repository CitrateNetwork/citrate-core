---
created: 2026-10-27
branch: rc52/macos-acceptance
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: active
wp: WP-H-MACOS (RC52 gate G1, g1-macos)
---

# macOS packaged acceptance (test machine only)

`scripts/ci/macos-acceptance.mjs` checks the **packaged** macOS build of Citrate Core, the RC DMG,
the way a member gets it. It is the macOS port of the Linux K1-L checks
(`scripts/ci/linux-native-acceptance.sh`, `docs/LINUX_NATIVE_ACCEPTANCE.md`, core#384). Gate G1
`g1-macos` of the RC52 planset (`citrate-federation`, `.agentile/planset/2026-10-26-rc052/gates.yaml`)
reads: "macOS fresh install of the RC DMG on a test machine (never the release Mac): Gatekeeper,
first launch, quit leaves nothing."

> **Never run this on the release Mac or on any machine where you use Citrate Core.** A full run
> installs and launches the app. The harness refuses to start without an explicit test-machine
> marker, and it also refuses when it finds Citrate Core or its data on the machine, but the
> marker is the rule: only set it on a machine you can wipe.

## What it checks

| Mode | What runs | Checks |
|---|---|---|
| `inspect` (default, dry run) | Reads the DMG file. No mount, no copy, no launch. | `dmg.sha256` matches; `dmg.verify` (image checksum); reported: `dmg.format`, `dmg.signature` (DMG signature, staple, Gatekeeper "open" assessment) |
| `verify` | `inspect`, then mounts the DMG read-only (`-nobrowse`). No copy, no launch. | `bundle.identity` (one `Citrate Core.app`, id `ai.citrate.core`, version = `release/components.json` `release`); `bundle.structure` (Contents/MacOS is exactly the main executable plus the externalBins of `src-tauri/tauri.bundle-lite.conf.json`, the overlay release.yml builds with); `sidecar.digests` (each sidecar's code equals its pinned bytes, see below); `sign.codesign` (deep, strict); `sign.developer_id` (Developer ID Application, team, hardened runtime; `--team-id` to pin the team); `sign.sidecars` (every sidecar signed by the same team, hardened runtime); `gatekeeper.spctl` (accepted, source "Notarized Developer ID"); `notarization.staple` (ticket stapled to the app); reported: `gatekeeper.syspolicy` |
| `full` | `verify`, then installs into the run directory (never `/Applications`), with an isolated HOME and a throwaway keychain. | `install.copy`; `gatekeeper.quarantined` (the installed copy with a browser's quarantine flag is accepted as notarized; `spctl --assess` does not read the quarantine flag, so this repeats `gatekeeper.spctl` on the copy, and the dialog itself is step 6); `launch.ready` (the required sidecars, default `citrate`, run under the app and the app is still alive after the settle time); `launch.rpc` (on 127.0.0.1:8545, `eth_chainId` and the genesis block hash from `eth_getBlockByNumber 0x0` both equal the active profile in `src-tauri/network/profiles.json`; a rerolled genesis keeps chain id 40204, so the chain id alone is not enough); `launch.listeners_loopback` (every socket of the app is loopback, connected (outbound) UDP, or matches a rule in `listener-policy.txt` for that sidecar, port and bind scope; see "LAN discovery" below); `restart.sidecar` (SIGKILL the node sidecar, a new one starts under the app); `quit.clean` + `quit.listeners` (Cmd-Q equivalent, `NSRunningApplication.terminate`: no process and no listener of the app left); `reap.orphans` (SIGKILL the app only; the next launch stops the sidecars it orphaned); `uninstall.processes` (bundle removed, nothing of it runs); `data.locations` (everything the app wrote in its HOME is under `data-locations.txt`); `uninstall.outside_files` (no Citrate file outside the run dir, except documented locations under your real home); `uninstall.keychain` (only the services in `keychain-allowlist.txt` were added) |

Only a `full` run with `overall: pass` is G1 evidence: the verdict says so in `g1_evidence`.
`inspect` and `verify` passing is useful (it proves the bytes and the signatures) but is not the gate.

**Sidecar digests.** Tauri re-signs each sidecar with the Developer ID, so a sidecar in the app
never has the sha256 of its pin. The harness compares a signature-independent code digest
instead: sha256 of the Mach-O bytes before the signature, with the three size fields that depend
on the signature zeroed (`__LINKEDIT` vmsize/filesize, `LC_CODE_SIGNATURE` datasize; universal
binaries slice by slice). The pinned file itself is first matched by its sha256 to the pin in
`release/components.json` / `src-tauri/runtime-deps.sha256`, and `sign.sidecars` proves the code
the signature covers is the code in the file. A sidecar that is missing, extra, unsigned or
changed fails.

**LAN discovery (mDNS) is expected.** Owner decision 2026-10-27: IPFS LAN discovery stays on in
0.5.2. The ipfs sidecar holds, besides its swarm on 4001 (TCP and UDP, all interfaces):
- UDP `*:5353` (mDNS);
- unconnected UDP sockets on random ephemeral ports (>= 49152), on all interfaces;
- the same on each LAN-side address (private or link-local IPv4, link-local or unique-local IPv6,
  or the machine's own addresses).

`scripts/ci/macos-acceptance/listener-policy.txt` allows exactly these for `ipfs` only, and
allows `citrate` its P2P port 30303.
- The same port held by another sidecar fails.
- A TCP listener off the policy fails.
- UDP below the ephemeral range on a non-policy port fails.
- Anything on a public address that is not the machine's own fails.

The unit tests pin the 0.5.1 app's real socket list (read with lsof in the core#558 review) and
these failure cases.

**Not covered** (as on Linux): sign-in (so Hermes, the local model, comms and cluster are not
started), the update path from 0.5.1 (G2), peering and sync (G4). The first-launch Gatekeeper
*dialog* is not automated: the harness checks Gatekeeper's decision, and step 6 below is the one
manual look.

## The test machine

Any Apple-silicon Mac (macOS 14 or later) that you can wipe: a spare Mac, a fresh macOS user
account on a spare Mac, or a macOS VM (UTM, Tart). It must not have Citrate Core installed, any
`~/Library/Application Support/ai.citrate.core`, a node on 8545/8546/30303, or a "Developer ID
Application" signing identity in its keychain. The harness checks all of these and refuses.
A GitHub-hosted `macos-14` runner is also a throwaway test machine; see "CI" below.

Needs: Node 22 (`brew install node@22` or nodejs.org), `git`, `gh` (only to fetch the pinned
bytes), and the Xcode command line tools for the self-test (`xcode-select --install`).

## Runbook

1. **Get the harness at the RC's commit**, so `release/components.json` matches the DMG:

   ```sh
   git clone https://github.com/CitrateNetwork/citrate-core.git && cd citrate-core
   git checkout <RC tag or freeze SHA>
   ```

2. **Mark this machine as a test machine** (both are needed, every time):

   ```sh
   mkdir -p ~/.config/citrate
   hostname >> ~/.config/citrate/macos-acceptance-test-hosts
   chmod 600 ~/.config/citrate/macos-acceptance-test-hosts
   export CITRATE_MACOS_ACCEPTANCE_TEST_MACHINE=1
   ```

   The hosts file must be owned by you and not group- or world-writable, and must list this
   machine's `hostname` (case and a trailing `.local` do not matter).

3. **Self-test the harness** (about 10 minutes). It builds a stand-in app (a bash script and a
   small C program; never Citrate Core) into real DMGs and runs it in a good mode and nine broken
   ones; each broken mode must fail exactly its own check, and release-mode runs of the ad hoc
   stand-in must fail the signing checks.

   ```sh
   node scripts/ci/macos-acceptance.mjs --self-test
   ```

   The last line must be `SELF-TEST PASS`. If not, stop: the harness cannot be trusted here.

4. **Fetch the RC DMG and the pinned sidecar bytes.** Take the DMG and its sha256 from the RC
   release (`SHA256SUMS` or the release notes), never from the DMG itself.

   ```sh
   mkdir -p ~/macacc/pinned && cd ~/macacc
   curl -fLO <RC DMG URL>
   node -e 'const m=require(process.argv[1]);for(const c of m.components){const a=c.artifacts["aarch64-apple-darwin"];console.log(a.runtime_deps_asset?`asset ${a.runtime_deps_asset}`:`url ${a.url} ${a.archive_sha256} ${a.member}`)}' \
     <checkout>/release/components.json > pins.txt
   while read -r kind a b c; do
     if [ "$kind" = asset ]; then gh release download runtime-deps --repo CitrateNetwork/citrate-core -p "$a" -D pinned
     else curl -fsSL -o archive.tgz "$a" && echo "$b  archive.tgz" | shasum -a 256 -c - && tar -xzf archive.tgz "$c" && cp "$c" "pinned/$(basename "$c")-upstream"; fi
   done < pins.txt
   ```

   The harness matches each file in `pinned/` to its pin by sha256, so names do not matter and a
   wrong file is simply not found.

5. **Run it**, dry run first, then the full run:

   ```sh
   cd <checkout>
   node scripts/ci/macos-acceptance.mjs --dmg ~/macacc/<file>.dmg --sha256 <sha256>                    # inspect
   node scripts/ci/macos-acceptance.mjs --dmg ~/macacc/<file>.dmg --sha256 <sha256> --mode verify \
     --pinned-dir ~/macacc/pinned --team-id <Apple team id>
   node scripts/ci/macos-acceptance.mjs --dmg ~/macacc/<file>.dmg --sha256 <sha256> --mode full \
     --pinned-dir ~/macacc/pinned --team-id <Apple team id> --log-dir ~/macacc/logs
   ```

   It prints one line per check and ends with `verdict: pass mode=full g1_evidence=true (...)`.
   Exit codes: 0 pass, 1 a check failed, 2 error or refused. A full run takes 5 to 15 minutes.

6. **The one manual look: the first-launch dialog.** In Finder, open the DMG, drag Citrate Core to
   Applications, open it. macOS shows "Citrate Core is an app downloaded from the Internet. Are
   you sure you want to open it?" with the developer named and no "cannot be checked for
   malicious software" warning. Quit it with Cmd-Q, check Activity Monitor shows nothing named
   Citrate, `citrate`, `ipfs`, `mem-mcp` or `node-agent`, then delete the app and
   `~/Library/Application Support/ai.citrate.core`. (This step uses the machine's real HOME;
   do it after the harness, since the harness refuses to run once that data exists.)

7. **Record the evidence** for G1 `g1-macos`: the `verdict.json` from `--log-dir` (machine,
   macOS version, harness commit `repo_rev`, DMG sha256, every check), the self-test's last line,
   and one line for step 6. Attach them to the RC52 board item; the lead flips the gate.

## Safety design

- **Marker first.** Before it runs any command the harness checks the env marker and the hosts
  file; without both it exits 2 having run nothing. There is no flag that skips this, and no CI
  bypass: a CI job writes the marker for its own throwaway runner. A test runs the real CLI
  without the marker and requires exactly that refusal.
- **Then read-only probes**, and it refuses (exit 2) on: not macOS; root; `/Applications/Citrate
  Core.app` or `~/Applications/Citrate Core.app`; data under the account's real home (Application
  Support, Caches, Logs, WebKit, HTTPStorages, Preferences, Saved Application State, `~/.citrate*`) (from the
  password database, so `HOME=` cannot hide it); a Citrate Core process; a listener on 8545, 8546
  or 30303; an `ai.citrate.core` keychain item; a Developer ID Application signing identity. A probe
  that fails counts as a refusal.
- **Guarded runner.** Every command that changes anything (mount, copy, launch, keychain, signals)
  goes through a runner that throws until the guards pass; read-only commands are an explicit
  list, which refuses the secret-revealing or writing forms (`security … -g`/`-w`, `dump-keychain
  -d`/`-r`, `plutil` with any `-o` other than `-o -`). Signals only ever go to processes inside the installed copy or started by this run (a
  reused pid is not), never by name or pattern.
- **Isolation in a full run.** The app runs from `<work>/run.*/Applications` with
  `HOME`/`CFFIXED_USER_HOME`/`TMPDIR` inside the run directory. The user's default keychain is a
  throwaway one for the length of the run; the previous default and search list are restored on
  exit, also on Ctrl-C. The run directory is removed afterwards (`MACOS_ACC_KEEP=1` keeps it).

Unlike K1-L, macOS has no namespaces, so the app shares the machine's network, WindowServer and
launchd. That is why the marker is mandatory and why the machine must be one you can wipe.

## Files

- `scripts/ci/macos-acceptance.mjs`: the harness (Node standard library only).
- `scripts/ci/macos-acceptance.test.mjs`: unit tests of its logic with fake dependencies, plus
  the tripwires (refusal without the marker, documented keychain services and data roots, the
  release overlay and pins, workflow safety). Runs in the normal `npm test`.
- `scripts/ci/macos-acceptance/`: `listener-policy.txt` (the non-loopback sockets each sidecar
  may hold), `keychain-allowlist.txt` (keychain services that may stay after uninstall),
  `data-locations.txt` (where the app may write under HOME).

## CI

`.github/workflows/macos-acceptance.yml` runs the self-test on `macos-14` for harness changes and on
dispatch. A dispatch with `dmg_url` and `dmg_sha256` also runs the harness against that DMG in the
chosen mode (default `full`) on a fresh GitHub-hosted runner, with the pinned bytes fetched as in
step 4. Whether a GitHub-hosted run counts for `g1-macos` instead of a physical test machine is the
owner's call; step 6 is manual either way.

Timeouts: `MACOS_ACC_LAUNCH_TIMEOUT` (s, default 240), `MACOS_ACC_RESTART_TIMEOUT` (120),
`MACOS_ACC_QUIT_TIMEOUT` (60), `MACOS_ACC_SETTLE_SECS` (15).
