---
created: 2026-07-26
branch: feat/w2-in-app-auto-updates
author: Claude (Opus 4.8), directed by @SaulBuilds
status: active
---

# Releasing Citrate Core (in-app auto-update pipeline — W2)

Every tagged release is built, Developer-ID signed, notarized, stapled, and its
**signed updater artifacts** (`*.app.tar.gz` + `*.sig`) plus a `latest.json` feed
are published to a GitHub Release. The shipped app runs `tauri-plugin-updater`,
which polls that feed, verifies the Ed25519 signature against the **public** key
pinned in `src-tauri/tauri.conf.json`, downloads, and installs on restart. Testers
never reinstall a DMG for a patch.

## One-time setup

### 1. Updater signing key
Generated with `tauri signer generate` (Ed25519). The keypair lives **outside the
repo** at `~/.citrate-updater/citrate-core.key{,.pub}`. The **public** key is
already pinned in `tauri.conf.json` (`plugins.updater.pubkey`). Add the **private**
key to repo secrets — never commit it.

@rule8: this key is the trust root for auto-update. Whoever holds it can push an
update the app will trust. Store it in the org secret manager; the CI secret is the
only copy CI needs. Losing it means shipping a new pinned pubkey (a hard cutover
for already-installed apps), so back it up.

**PBA-L7b-006 (2026-09-24 pre-bounty audit) — current state and the OWNER re-key.**
The live key at `~/.citrate-updater/citrate-core.key` was generated **without a
passphrase** and sits on the maintainer laptop with mode `0644`; releases are signed
locally (the CI release job has never run). Any same-user process or unencrypted
backup can mint an update signature. Re-key (owner only; an agent cannot do this):

1. `tauri signer generate -w ~/.citrate-updater/citrate-core-v2.key` and set a strong
   passphrase when prompted. `chmod 600` the new key.
2. Put the new private key + passphrase in the org secret manager and in the repo
   secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Ship ONE release, signed with the **old** key, whose `tauri.conf.json`
   `plugins.updater.pubkey` is the **new** public key (installed apps verify that
   release with the old key, then trust only the new one).
4. After that release is out, securely delete the old key and every laptop copy
   (`rm -P` / secure-erase; check backups), and keep only the secret-manager copy.

### 2. GitHub Actions secrets
| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | contents of `~/.citrate-updater/citrate-core.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the updater key passphrase (the current key is password-less — re-key per PBA-L7b-006 above) |
| `APPLE_CERTIFICATE` | base64 of the Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Larry Klosowski (DDHUG44QC7)` |
| `KEYCHAIN_PASSWORD` | any throwaway string (CI builds a temp keychain) |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | notarization (use an app-specific password) |
| `CITRATE_CHAIN_READ_TOKEN` | fine-grained PAT, Contents:read on `citrate-chain` |

### 3. Runtime deps prerelease (`runtime-deps`) — digest-pinned (PBA-L7b-005)

The prerelease is **mutable**, so the release workflow does not trust it: every asset
is downloaded to a scratch dir and checked against the committed manifest
`src-tauri/runtime-deps.sha256` (`scripts/ci/verify-runtime-deps.sh`) **before** it is
staged, bundled, signed or notarized. An unpinned or changed asset fails the release.
When you upload a new asset, pin it in the same PR:

```bash
shasum -a 256 <each asset file> >> src-tauri/runtime-deps.sha256   # "<hex>  <name>"
```

CI's `release pin tripwire` step (`scripts/ci/check-release-pins.sh`) fails if
release.yml ever downloads an asset that bypasses this check.

The ~4.3 GB Gemma model, the llama runtime dylibs, and the three sidecars
(`citrate`, `mem-mcp`, `node-agent`) are **not in git**. Upload them once to a
GitHub prerelease tagged `runtime-deps`; the release workflow pulls them each build:

```bash
gh release create runtime-deps --prerelease --title "Runtime deps (bundle inputs)" \
  src-tauri/binaries/citrate-aarch64-apple-darwin \
  src-tauri/binaries/mem-mcp-aarch64-apple-darwin \
  src-tauri/binaries/node-agent-aarch64-apple-darwin \
  src-tauri/binaries/llama-server-aarch64-apple-darwin \
  src-tauri/models/gemma-4-E4B-it-Q4_0.gguf
# llama dylibs as one tarball:
tar -czf /tmp/llama-runtime-arm64.tar.gz -C src-tauri/llama .
gh release upload runtime-deps /tmp/llama-runtime-arm64.tar.gz
```
Refresh this release whenever a sidecar or the model changes (e.g. a DGX node rebuild).

## Cutting a release
1. Bump `version` in `src-tauri/tauri.conf.json` **and** `src-tauri/Cargo.toml`
   (must match; the updater compares this to the feed's version).
2. Tag and push:
   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```
   (or run the `release` workflow manually with the tag input).
3. The workflow builds, signs, notarizes, and publishes the release + `latest.json`.
4. Verify: an older installed build shows the **Update available** card within its
   check interval (or on next launch), downloads with real byte progress, and
   restarts into the new version.

### Distribution — the DO Space is the public origin

The updater endpoint is a **DigitalOcean Space**, not a GitHub URL, because
citrate-core is private and `releases/…/download/…` 404s for end users
(`src-tauri/tauri.conf.json` → `plugins.updater.endpoints`, primary =
`https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/latest.json`,
GitHub kept only as an authed fallback). The pinned Ed25519 pubkey verifies the
payload, so serving it from a public CDN is safe by construction.

The GitHub Release is the signed **source**; DGX mirrors it to the public Space.
Per release (owner-gated — needs DO Spaces keys), after step 3:

1. **Member DMG:** upload `Citrate-Core-macos-arm64.dmg` to
   `downloads/Citrate-Core-macos-arm64.dmg`, **verify sha256 == the release asset**,
   flush the CDN. `citrate.ai/download/mac` 307s to it.
2. **Updater feed:** upload `Citrate-Core.app.tar.gz` (+`.sig`) to
   `downloads/updater/`, then **rewrite `latest.json`'s
   `platforms.darwin-aarch64.url`** from the GitHub asset URL to the Space URL
   (the inlined signature is unchanged), upload `latest.json` to
   `downloads/updater/latest.json`, flush the CDN for those paths.

First run of this path: v0.1.0 (2026-09-09), verified live — public DMG 200 at the
release sha256, Space `latest.json` carrying the byte-identical signed tarball.

## Marking a patch critical
Put `[critical]` (or a leading `critical:`) in the release body/notes. The app then
**auto-downloads** that update (still an explicit **Restart now** — the app is never
force-quit under the user).

## First-run caveat (honest)
This pipeline has not yet had a first live tagged run. The Apple signing +
notarization legs and the `runtime-deps` staging must be proven by the first `v*`
tag with the secrets in place. The app-side updater (plugin, config, key, UX) is
built and unit-tested; producing a signed feed is what the first tag validates.
