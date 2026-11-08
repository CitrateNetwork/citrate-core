---
created: 2026-11-08
branch: rel/053-network
author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
status: approved plan (owner decisions of 2026-11-07, relayed by the MAC lead); executed at the 0.5.3 release
related: citrate-landing#174 docs/DOWNLOAD_GATE.md §9 (GATE lane); citrate-core#604 (first-run access check)
---

# How 0.5.3 reaches existing installs

## Owner decisions (2026-11-07)

1. **Now:** the CDN stays fully private. No bucket or object ACL changes before the 0.5.3
   release. In-app updates of 0.5.0 and 0.5.1 installs stay broken until then (below, §1).
2. **From 0.5.3 on, approved:** installers live only on the private CDN, behind per-person signed
   download links (the GATE design, citrate-landing#174, `docs/DOWNLOAD_GATE.md` §9). **Update
   files are public on the CDN.** GitHub releases carry only `latest.json`, checksums and
   signatures.
3. **First-run access check:** citrate-core#604 adds it (off unless `CITRATE_ACCESS_GATE_URL` is
   set at build time). 0.5.3 is built with
   `CITRATE_ACCESS_GATE_URL=https://www.citrate.ai/api/access/verify`. This PR does not touch the
   access check; #604 owns it.

## 1. What installed apps do today (facts, checked 2026-11-08)

- 0.5.0, 0.5.1 and 0.5.2 all carry the same two feeds and the same updater public key (v2), fixed at
  build time in `src-tauri/tauri.conf.json`, in this order:
  1. `https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/latest.json`
  2. `https://github.com/CitrateNetwork/citrate-core/releases/latest/download/latest.json`
- The app checks at launch and every 6 h (`src/shell/updater.ts`), sends no credential, and takes
  an update only if its minisign signature verifies against that key. Restart is always the
  member's click.
- tauri-plugin-updater 2.10.1 (`updater.rs` `check()`) tries the feeds in order: a non-2xx answer
  is logged and the next feed is tried; the first 2xx that parses as a release ends the loop.
- Feed 1 answers 403 (bucket private since 2026-11-03), so every install reads feed 2. Feed 2 is
  the newest non-prerelease GitHub release, v0.5.2, whose `latest.json` names 0.5.2 and points every
  platform at `citrate-cdn.../downloads/v0.5.2/...`, which is 403. So:
  - **0.5.0 and 0.5.1** are offered 0.5.2 and the download fails: broken until 0.5.3 (decision 1);
  - **0.5.2** is told it is up to date.
- Manifest platforms: `darwin-aarch64` (`Citrate-Core.app.tar.gz`), `linux-x86_64` and
  `linux-aarch64` (AppImage), `linux-x86_64-deb` and `linux-aarch64-deb`.

## 2. Why the order matters at the r1007 cutover

When `rpc.citrate.ai` serves r1007, a 0.5.2 (or older) app reads a different block 0 and shows
"Update required": it refuses to sign and its node cannot follow the network. The updater does not
depend on the chain and keeps working. So 0.5.3's feeds must be live **before** (or with) the RPC
cutover, so "Update required" has an update to offer. On the first 0.5.3 start the node data dir
moves to r1007 and the old chain is kept in `node/chain.r1006.bak/` (CORE-N3,
[053-NETWORK.md](053-NETWORK.md)); the wallet, pins, Hermes settings and Store are untouched.

## 3. The release steps (approved plan)

1. **Build and sign** 0.5.3 as for 0.5.2, from the tree after the network cut, with
   `CITRATE_ACCESS_GATE_URL=https://www.citrate.ai/api/access/verify` in the build environment
   (#604). An updating install already has `config.json`, so #604 records it as an existing
   install and never asks it for a code.
2. **Installers** (`.dmg`, AppImages, `.deb`) go to the private CDN under `downloads/v0.5.3/`.
   They stay private; members get them only through the signed links (GATE).
3. **Update files** (`Citrate-Core.app.tar.gz`, both AppImages, both `.deb`, each with its `.sig`)
   go to `downloads/updater/v0.5.3/`, and **only those objects** are made public-read. This is the
   one ACL change, made at the 0.5.3 release and not before (decision 1).
4. **`latest.json`**: version 0.5.3, each platform's `url` one of step 3's public objects, each
   `signature` the `.sig` content. Published twice, byte-identical:
   - public-read at `downloads/updater/latest.json` (feed 1);
   - as an asset of the **v0.5.3 GitHub release** (feed 2). That release carries only
     `latest.json`, the checksums (`SHA256SUMS`, `SHA256SUMS-linux`) and the signatures (`*.sig`),
     no installers and no update files (they would be permanent and public). The node builds'
     PROVENANCE records ship as release assets next to the node bytes on the runtime-deps release,
     pinned by the cut (lead decision on #605; [053-NETWORK.md](053-NETWORK.md) N1).
5. **Verify anonymously** (no credentials, outside the org network): both feeds answer 200 with
   0.5.3; every manifest `url` answers 200 and matches `SHA256SUMS`; every installer under
   `downloads/v0.5.3/` answers 403. Then RC52 G8 (the plugin's own check and download with
   signature verification) on test machines with 0.5.0, 0.5.1 and 0.5.2 installed, macOS and Linux,
   never the release Mac.
6. **Publish the GitHub release last.** Marking it latest moves feed 2 for every install, and a
   GitHub release cannot be edited back; feed 1 can be fixed in place.

With both feeds on 0.5.3, every install from 0.5.0 on updates: feed 1 now answers 200, and feed 2
is the same manifest if feed 1 is unreachable.

## 4. Hazards and rollback

- **A stale public feed 1 stops updates.** The plugin stops at the first 2xx, so a public
  `downloads/updater/latest.json` that is older than feed 2 hides feed 2. Update both together,
  feed 1 first only within the release steps above.
- **Feed 2 is permanent.** A bad v0.5.3 GitHub release needs a v0.5.4; that is why it goes last.
- **Never point the feeds back at 0.5.2 after the cutover.** A 0.5.2 build on an r1007 data dir
  deletes the r1007 chain (0.5.2 deletes on a genesis change; only 0.5.3 sets it aside), and its
  network is retired anyway. Ship a fixed 0.5.4.
- A member who skips 0.5.3 gets the same switch from any later build (the set-aside runs on any
  genesis change).

## 5. Tests that cover the path

| Piece | Test |
|---|---|
| The plugin's check and download, signature refusals | `upgrade_updater_feed_tests` (RC52 G8, CI) |
| Packaged N to N+1 install and restart | K1-L `update-e2e` (CI, Linux x86_64 and aarch64) |
| The data dir switch r1006 to r1007, nothing lost | `upgrade_r1007_tests`, `node_genesis` `n3_*` (CI) |
| Feeds and update files public, installers gated | step 5 (manual, at release) |
| First-run access check on an updating install | #604's tests |

## 6. Left open

- For 0.5.4 and later (GATE §9 step 7): a credentialed feed added by a later build, so update files
  can go private once every install has it. Not in 0.5.3.
