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
repo** at `~/.citrate-updater/citrate-core-v2.key{,.pub}` (passphrase-protected;
the passphrase is in the release maintainer's macOS Keychain as `citrate-core-updater-v2`). The **public** key is
already pinned in `tauri.conf.json` (`plugins.updater.pubkey`). Add the **private**
key to repo secrets — never commit it.

@rule8: this key is the trust root for auto-update. Whoever holds it can push an
update the app will trust. Store it in the org secret manager; the CI secret is the
only copy CI needs. Losing it means shipping a new pinned pubkey (a hard cutover
for already-installed apps), so back it up.

**Key handling.** The updater private key must be passphrase-protected (set a strong
passphrase when running `tauri signer generate`) and stored in the org secret manager
and the repo secrets below, not on developer machines. Key rotation (including the
pubkey cutover release) follows the private release runbook.

**Signing a local release.** Export the key and passphrase for the build only:
```sh
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.citrate-updater/citrate-core-v2.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -s citrate-core-updater-v2 -a citrate-updater -w)"
```
Update artifacts for every platform (macOS, Linux, Windows) are signed with this one
key. Artifacts built on other hosts are signed here with `tauri signer sign`, so the
private key never leaves the signing machine.

### 2. GitHub Actions secrets
| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | contents of `~/.citrate-updater/citrate-core-v2.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the updater key passphrase |
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

The staged llama runtime carries each dylib three times (`libggml.dylib`, `libggml.0.dylib`,
`libggml.0.23.0.dylib`) plus llama.cpp tool libraries llama-server never loads. Before a build,
`node scripts/prune-llama-runtime.mjs --dir src-tauri/llama` keeps only llama-server's dependency
closure (read with `otool -L`; it refuses if a needed library is missing) and saves about 34 MB.
release.yml runs it after extracting the tarball; run it on a local staging too (HUP-S11.0).
`scripts/build-hermes.sh` installs the Hermes sidecar with `strip -x` (about 4 MB; `--no-strip`
keeps the symbols for debugging).
Refresh this release whenever a sidecar or the model changes (e.g. a DGX node rebuild).

#### The BGE embedding GGUF (`bge-base-en-v1.5-f16.gguf`, HUP-S1.2 / US-1.4)

Hermes ranks the tools it offers per request, and the skills it surfaces per turn, with a second
loopback `llama-server` the app starts in embedding mode (`--embeddings --pooling cls`, its own
API key; `src-tauri/src/embed_serve.rs`). It loads the same BGE weights as `models/bge-base-en-v1.5`,
converted to GGUF. The conversion is deterministic and checked against the pin (already in
`src-tauri/runtime-deps.sha256`, pending owner sign-off):

```bash
# a llama.cpp checkout at tag b8640, with its converter's Python packages installed
LLAMA_CPP_DIR=/tmp/llama.cpp scripts/build-bge-gguf.sh /tmp/bge-base-en-v1.5 /tmp/bge-gguf
gh release upload runtime-deps /tmp/bge-gguf/bge-base-en-v1.5-f16.gguf --clobber -R CitrateNetwork/citrate-core
```

Without it (for example a lite bundle) the app still runs: Hermes sessions rank lexically and say so.

#### The Hermes knowledge corpus (`knowledge-corpus.tar.gz`, HUP-S3.1)

The corpus Hermes imports on first run is built in citrate-memories from the
federation checkouts (format `citrate-corpus/2`; spec `corpus/hermes-knowledge.toml`):

```bash
cd ../citrate-memories
scripts/fetch-corpus-refs.sh ..                 # Solady + Foundry book at pinned commits
# EMBED_BGE_DIR = the exact BGE files the release bundles (extract bge-base-en-v1.5.tar.gz):
# every node is embedded once here, so members do not embed it on their CPU.
EMBED_BGE_DIR=/tmp/bge-base-en-v1.5 \
  scripts/build-corpus.sh .. /tmp/knowledge-corpus   # deterministic; verifies before exit
tar -czf /tmp/knowledge-corpus.tar.gz -C /tmp knowledge-corpus
gh release upload runtime-deps /tmp/knowledge-corpus.tar.gz --clobber -R CitrateNetwork/citrate-core
```

Pin it like every other asset. Ship it together with a `mem-mcp` built from
citrate-memories with `import-corpus` (`--features rocksdb,transformer`): the
release step runs `scripts/stage-knowledge-corpus.mjs`, which refuses an older
`mem-mcp` and any corpus that does not match its own manifest. A local build stages
the same way:

```bash
node scripts/stage-knowledge-corpus.mjs /tmp/knowledge-corpus \
  --bge-dir src-tauri/models/bge-base-en-v1.5 \
  --mem-mcp src-tauri/binaries/mem-mcp-aarch64-apple-darwin
```

Build the release corpus from clean checkouts. mem-corpus records a source whose work tree had
local changes as `<commit>-dirty` and a source outside any git work tree as `unpinned`; the stager
refuses a corpus with either on an included source (its text and NOTICE cannot be reproduced from
the recorded commit); `--allow-dirty` stages it for a dev build, with a warning.

The corpus depends on the bundled BGE model, so `--bge-dir` is required. The stager
refuses a missing or partial model (the first-run import would be skipped as
`not-semantic`) and any tenant whose vectors were not made with exactly the bundled
`model.safetensors` (the importer would ignore them and embed every node on the
member's CPU, about two hours for the full corpus on an Apple M2 Max). A dev build may
pass `--allow-unembedded`; the stager then warns how many nodes members will embed.

Without a staged corpus `tauri build` itself still succeeds (the committed
`src-tauri/knowledge-corpus/README.md` keeps the resource glob valid) and the app reports
`skipped: no-bundle` on first run, so a release build must not rely on the build failing. The
release path fails closed instead:

- `scripts/ci/verify-runtime-deps.sh` refuses a pins manifest without a
  `knowledge-corpus.tar.gz` pin, and any call that stages a `mem-mcp` without the corpus.
- The stager writes a record beside the directory (`src-tauri/knowledge-corpus.staged.json`,
  git-ignored: bundle digest, node count, the input asset's sha256). Right before bundling,
  on every platform, run

  ```bash
  node scripts/check-staged-corpus.mjs --pins src-tauri/runtime-deps.sha256
  ```

  It refuses a README-only directory, a corpus the stager did not record or that changed after
  staging (digest, node count, file hashes), a dev-staged corpus (`--allow-dirty`,
  `--allow-unembedded`; pass `--allow-dev` for a dev build), and one not staged from the pinned
  `knowledge-corpus.tar.gz`. release.yml runs it before `tauri-action`, and
  `scripts/ci/check-release-pins.sh` fails CI if that step is removed or moved after the build.
  Linux and Windows: [`RELEASE_LINUX.md`](RELEASE_LINUX.md) and
  [`RELEASE_WINDOWS.md`](RELEASE_WINDOWS.md) step 2b.

For a local Mac DMG, stage from the pinned asset exactly as release.yml does:

```bash
gh release download runtime-deps -p knowledge-corpus.tar.gz -D /tmp/rd -R CitrateNetwork/citrate-core
node scripts/stage-knowledge-corpus.mjs /tmp/rd/knowledge-corpus.tar.gz \
  --bge-dir src-tauri/models/bge-base-en-v1.5 \
  --mem-mcp src-tauri/binaries/mem-mcp-aarch64-apple-darwin
node scripts/check-staged-corpus.mjs --pins src-tauri/runtime-deps.sha256
```

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
5. Size gate (HUP-S11.0): on the machine that built the bundle, run
   `node scripts/size-budget.mjs` (reads `target/release/bundle`, compares with
   `release/budgets.json`, exits 1 when anything is over budget). In CI it is manual only:
   dispatch `release` with `size_gate: true` and the release stays a draft unless the gate
   passes; tag pushes never run it. See [`release/README.md`](../release/README.md).
6. Licence check (gate g3-licence): `node scripts/licence-inventory.mjs --corpus
   src-tauri/knowledge-corpus` must pass (every sidecar, resource, tool, library, skills and
   corpus source has a licence entry and its texts ship in `licenses/`). Before the first
   public 0.5.0 release add `--require-sign-off`, which fails until the owner signs
   [`LICENCE_REVIEW.md`](LICENCE_REVIEW.md).
7. Third-party notices (g3-licence, LICENCE_REVIEW 4.7): with the sidecar sources checked out at
   the revisions the bundled sidecars were built from, regenerate the notices of every crate and Go
   module compiled into the app, the sidecars and Kubo, then check them:
   ```sh
   cargo install cargo-about --locked --features cli            # once; 0.9.2 used 2026-10-04
   go install github.com/google/go-licenses/v2@v2.0.1           # once
   node scripts/third-party-notices.mjs collect --federation-root ..   # about 5 min per Rust component
   node scripts/third-party-notices.mjs render                  # writes src-tauri/licenses/THIRD-PARTY-NOTICES.txt
   node scripts/third-party-notices.mjs check
   node scripts/licence-inventory.mjs --require-notices
   ```
   `render` refuses a third-party package under a licence outside its permissive list, so a new
   copyleft dependency is a reviewed change. What is scanned is `release/notices.json`.

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
