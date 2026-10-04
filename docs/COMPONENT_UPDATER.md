---
created: 2026-10-01T18:00:00Z
branch: hup/n4-components
author: Larry Klosowski + Claude Opus 5.5
status: draft (signing key and SLA values pending owner sign-off)
wp: HUP-S5.5, HUP-S6.1
---

# Signed component updater and the CVE SLA

Large or fast-moving parts of Citrate Core are installed on first run and kept current as
**signed components**, not shipped inside the installer: the dApp toolchain today, and the
managed browser, private search, skills and the docs graph as they land (planset
`2026-09-30-hermes-upskill`, red-team correction 15 and D-21). This page is the policy and the
runbook. The code is the `citrate-components` crate (`components/`) and
`src-tauri/src/components.rs`; the toolchain list is `components/toolchain-bundle.json`.

## State today

| Item | State |
|---|---|
| Verification, staging, verify-then-swap, rollback, recovery | Built and tested (`components/tests/`), TLC model `src-tauri/formal/ComponentSwap.tla` |
| Production component signing key | **Not set.** The slot `PRODUCTION_COMPONENT_PUBKEY` is empty, so the app refuses every update before any download. Set at a key ceremony (@rule8) |
| Published component manifest | None yet. The first one is signed at the ceremony |
| CVE SLA numbers below | **Placeholders, pending owner sign-off** |
| Toolchain bundle | macOS arm64 measured; other platforms listed with upstream URLs and marked to be measured |
| zip archives (Windows foundry and node) | Not unpacked yet; the updater refuses them honestly (Windows toolchain spike) |

Nothing here changes anything for members until the key is set: the Settings card says updates
are off and why, and its install buttons are disabled.

## How an update is verified

1. **Key first.** With the key slot empty the update command returns at once: no network, no
   disk.
2. **Manifest.** `manifest.json` and its detached minisign signature `manifest.json.minisig` are
   fetched over HTTPS (redirects must stay on HTTPS). The signature must be:
   - made by the pinned component key (never the app-updater key; the code refuses that key);
   - prehashed (`ED`, Ed25519 over BLAKE2b-512); legacy signatures are refused;
   - for this purpose: its trusted comment starts with `citrate-components-manifest`.
3. **Manifest checks**, only after the signature verifies: schema, field rules (names, versions,
   HTTPS URLs, 64-hex SHA-256, sizes up to 2 GiB, safe relative entrypoints, known platforms),
   `issued_at` not in the future (10 min skew), not expired, lifetime at most 31 days, and
   `sequence` not lower than the last one this machine recorded (the same sequence is accepted
   only for the same bytes). The newest manifest is recorded before any install starts, so an
   older one can never be replayed afterwards.
4. **Artifact.** Downloaded into `<components>/.staging/<name>-<nonce>/`, capped at the manifest
   size. One pass over the file checks size, SHA-256 and the artifact's own minisign signature
   (trusted comment `citrate-components-artifact ...`).
5. **Unpack.** raw, tar.gz or tar.xz. Paths must be relative with no `..`; hard links, devices
   and duplicates are refused; set-id bits are dropped; symlinks are created last and must stay
   inside the tree both as written and after resolution.
6. **Health check.** Every entrypoint the manifest names exists inside the unpacked tree.
7. **Swap.** The tree is renamed to `<components>/<name>/<version>-<sha12>/`, then `state.json`
   is replaced (write, fsync, rename). That rename is the only commit point: any failure before
   it leaves the current version untouched. The previous version stays on disk for rollback;
   older ones are removed.
8. **Recovery.** At the start of every update, leftover staging directories and version
   directories that `state.json` does not reference are removed.

## Manifest format

```json
{
  "schema": 1,
  "channel": "stable",
  "sequence": 7,
  "issued_at": 1790000000,
  "expires_at": 1791209600,
  "components": [{
    "name": "foundry",
    "version": "1.5.1",
    "kind": "toolchain",
    "license": "MIT OR Apache-2.0",
    "entrypoints": ["forge", "anvil", "cast", "chisel"],
    "artifacts": {
      "macos-arm64": {
        "url": "https://...",
        "sha256": "<64 hex>",
        "size": 71587260,
        "format": "tar.gz",
        "signature": "<the whole .minisig text>"
      }
    }
  }]
}
```

`kind` is one of `browser`, `toolchain`, `search`, `skills`, `docs-graph`, `library`. Platform
keys: `macos-arm64`, `macos-x64`, `linux-x64`, `linux-arm64`, `windows-x64`, and `any` for
platform-independent artifacts (the Solidity libraries). An artifact may carry its own
`entrypoints` when its top-level directory names the platform.

## Publishing (release ceremony)

The component key is a minisign key, separate from the app-updater key. Only its public key line
is committed, into `PRODUCTION_COMPONENT_PUBKEY` in `components/src/key.rs`, in a signed commit
that also updates the test that pins the empty slot.

1. Measure what is still `to_be_measured` in `components/toolchain-bundle.json` on a machine of
   that platform, and run each tool once from the unpacked archive.
   `citrate-components unpack --format <f> --archive <file> --dest <new dir>` uses the same
   unpacker as the app.
2. `citrate-components check-bundle --repo .` must print `bundle ok`.
3. Sign every measured artifact offline:
   `minisign -S -H -s <component.key> -m <artifact> -t "citrate-components-artifact <name> <version>"`,
   and save each signature as `<sha256>.minisig` in one directory.
4. `citrate-components manifest-from-bundle --repo . --sequence <n> --issued-at <unix> --expires-at <unix> --sigs <dir> > manifest.json`
   (sequence: one more than the last published; expiry: 14 days, at most 31).
5. Sign the manifest: `minisign -S -H -s <component.key> -m manifest.json -t "citrate-components-manifest <sequence>"`.
6. Check it: `citrate-components verify-manifest --manifest manifest.json --sig manifest.json.minisig --pubkey <key line>`.
7. Upload both files to `downloads/components/stable/` on the CDN (the URLs are in
   `src-tauri/src/components.rs`).

A manifest must be re-signed before it expires even when nothing changed; that is what keeps
installed clients fresh.

## CVE SLA (pending owner sign-off)

**Publisher side.** From a public advisory affecting a bundled component (the managed browser,
node, Python, the toolchain, search), a signed manifest with the fixed version is published
within:

| Severity | Deadline (placeholder) |
|---|---|
| Critical, or exploited in the wild (any browser engine RCE) | 72 hours |
| High | 7 days |
| Medium | 30 days |
| Low | 90 days, or the next scheduled manifest |

If no fixed upstream version exists in time, the component is withdrawn from the next manifest
or a mitigation ships in the app. How the app treats an already installed affected version is
not built yet (an owner decision below).
Advisory sources to watch: the upstream security advisories of each component, the GitHub
advisory database, and the Chromium release blog for the browser.

**Client side** (computed in code, `components/src/policy.rs`):

- a manifest older than **3 days** shows "updates are stale";
- an **expired** manifest, or none ever checked, is to keep the managed browser off the open
  web. Today this is computed (`browserMayOpenWeb` in `components_status`) and shown in
  Settings, but no managed browser reads it yet: the block is not enforced. Whoever wires it
  must not block members while the component key slot is still empty (every machine is
  "never checked" until the first signed manifest exists);
- the manifest lifetime is capped at 31 days, so a frozen feed cannot keep a client "current".

## Owner decisions

- The component signing key ceremony (@rule8): who holds it, offline storage, rotation.
- The SLA deadlines and the 3-day stale window above.
- The 31-day lifetime cap and the 14-day expiry used by the release step.
- Whether the browser block on an expired manifest is a block or a warning.
- How an installed version named in an advisory is treated (warn, disable, or remove).
- Whether to mirror upstream artifacts on the Citrate CDN (today the bundle points at upstream
  URLs; the signed manifest pins the bytes either way).
- Licence review of the bundled tools (slither and medusa are AGPL-3.0, aderyn and solc are
  GPL-3.0) and the source offer: gate `g3-licence`. The review, the draft source offer and the
  sign-off checklist are in [`LICENCE_REVIEW.md`](LICENCE_REVIEW.md); the inventory is
  `release/licences.json`, checked by `scripts/licence-inventory.mjs`.
