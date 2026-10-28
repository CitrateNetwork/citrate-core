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

The sidecars, the llama runtime dylibs, the BGE models and the knowledge corpus are **not in
git**. Each sidecar is pinned in [`release/components.json`](../release/components.json) (source
repo, exact rev, toolchain, build command, sha256 per target). On the release Mac, build each one
at its pin; the scripts install it only if its sha256 matches:

```bash
scripts/build-sidecar.sh --target aarch64-apple-darwin          # citrate-node (lineage gates)
scripts/build-component.sh node-agent --target aarch64-apple-darwin
scripts/build-component.sh mem-mcp --target aarch64-apple-darwin
scripts/build-comms-daemon.sh --target aarch64-apple-darwin
scripts/build-cluster-daemon.sh --target aarch64-apple-darwin
scripts/build-hermes.sh --target aarch64-apple-darwin           # stripped; the pin is of the stripped bytes
scripts/build-component.sh kubo --target aarch64-apple-darwin   # upstream Kubo, archive + binary checked
```

Upload the six cargo sidecars under their pinned asset names (`<bin>-aarch64-apple-darwin-<rev12>`,
the `runtime_deps_asset` of each component), which release.yml downloads and verifies:

```bash
mkdir -p /tmp/rd
for f in citrate-aarch64-apple-darwin-ad11ed9df0d8 node-agent-aarch64-apple-darwin-1d8cb8e4e24a \
  mem-mcp-aarch64-apple-darwin-5aeda3172369 comms-member-daemon-aarch64-apple-darwin-9cd12c41ae1c \
  cluster-daemon-aarch64-apple-darwin-755cf2172260 hermes-aarch64-apple-darwin-05243c72bd52; do
  cp "src-tauri/binaries/${f%-*}" "/tmp/rd/$f"
done
grep -E 'aarch64-apple-darwin-[0-9a-f]{12}$' src-tauri/runtime-deps.sha256 | (cd /tmp/rd && shasum -a 256 -c --strict)
gh release upload runtime-deps /tmp/rd/* -R CitrateNetwork/citrate-core
```

To move a pin: build with `--print-digest`, put the digest in `src-tauri/runtime-deps.sha256` (a new
asset name per rev) or the artifact's `sha256`, and open a PR; `node scripts/component-pins.mjs
check` fails CI if the manifest, the scripts, the overlays, these pins and release.yml disagree.
macOS digests reproduce only on the machine and build root that recorded them (the linker derives
the Mach-O UUID from the output path), so the release Mac records the macOS pins.

When a pin moves, the Linux digests of the old rev no longer apply, and the DGX records new ones
on its own hosts. Until it does, the component lists the target under `pending`, with who builds
it and how, instead of an artifact: `build-component.sh` refuses that target and names the reason,
and `node scripts/component-pins.mjs pending` lists everything still owed. The macOS target is
never pending.

The Gemma model is **not bundled**: macOS ships `bundle-lite`, and the app downloads Gemma on first
run and refuses it unless it matches `MODEL_SHA256` and `MODEL_SIZE_BYTES` in
`src-tauri/src/model.rs`. `release/components.json` records the same digest with a
revision-pinned Hugging Face URL, and the check keeps the two equal.

#### The llama runtime (`llama-server` + dylibs), built with the web UI off (v0.5.0-rc.4)

The macOS llama runtime is llama.cpp **b10909** (commit `a2878d30d`) built from source with the
options of upstream's `macos-arm64` release job (its `.github/workflows/release.yml` at that tag)
plus the web UI switched off. llama-server's built-in web UI is an npm bundle with no notices;
the app never serves it (`--no-webui`), so it is not shipped at all. At b10909
`-DLLAMA_BUILD_WEBUI=OFF` alone is not enough: the default `LLAMA_USE_PREBUILT_UI=ON` downloads
the prebuilt UI and embeds it, so turn that off too. The build must say `UI: embedded 0 assets`.

```bash
git clone https://github.com/ggml-org/llama.cpp.git && cd llama.cpp && git checkout b10909
# full history: the build number (10909) is `git rev-list --count HEAD`
cmake -B build \
  -DGGML_METAL_EMBED_LIBRARY=ON -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3 \
  -DCMAKE_INSTALL_RPATH='@loader_path' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
  -DLLAMA_FATAL_WARNINGS=ON -DLLAMA_BUILD_BORINGSSL=ON \
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_BUILD_SERVER=ON -DGGML_RPC=ON \
  -DLLAMA_BUILD_WEBUI=OFF -DLLAMA_USE_PREBUILT_UI=OFF \
  -DGGML_MACHINE_SUPPORTS_i8mm=OFF -DGGML_MACHINE_SUPPORTS_sve=OFF -DGGML_MACHINE_SUPPORTS_sme=OFF
cmake --build build --config Release -j "$(sysctl -n hw.logicalcpu)"
```

The three `GGML_MACHINE_SUPPORTS_*=OFF` pin ggml's native CPU probe to what upstream's M1 runner
found (dotprod, no i8mm, SVE or SME). Built natively on an M2 or later the probe adds i8mm, and
the library then crashes on M1 Macs; the SVE probe also hangs on some Macs. Check with
`otool -tv build/bin/libggml-cpu.0.dylib | grep -cw smmla` (must be 0).

Package the flat dependency closure of llama-server (what `scripts/prune-llama-runtime.mjs`
keeps) and a sidecar copy with the app's rpath, each Developer ID signed:

```bash
ID="Developer ID Application: Larry Klosowski (DDHUG44QC7)"
mkdir -p rt && cp build/bin/llama-server rt/ && cp LICENSE rt/LICENSE-llama.cpp.txt
for f in libllama-server-impl.dylib libllama-common.0.dylib libmtmd.0.dylib libllama.0.dylib \
  libggml.0.dylib libggml-cpu.0.dylib libggml-blas.0.dylib libggml-metal.0.dylib \
  libggml-rpc.0.dylib libggml-base.0.dylib; do cp -L "build/bin/$f" rt/; done
for f in rt/*.dylib rt/llama-server; do codesign --force --options runtime --timestamp --sign "$ID" "$f"; done
(cd rt && shasum -a 256 $(ls | sort) > llama-server.sha256)
cp build/bin/llama-server llama-server-aarch64-apple-darwin-b10909-nowebui
install_name_tool -add_rpath @loader_path/../Resources/llama llama-server-aarch64-apple-darwin-b10909-nowebui
codesign --force --options runtime --timestamp --sign "$ID" llama-server-aarch64-apple-darwin-b10909-nowebui
# a reproducible tarball (sorted entries, fixed mtime, uid/gid 0, gzip mtime 0) of rt/ as
# llama-runtime-arm64-b10909-nowebui.tar.gz, then upload both under NEW names and pin them:
gh release upload runtime-deps llama-runtime-arm64-b10909-nowebui.tar.gz \
  llama-server-aarch64-apple-darwin-b10909-nowebui -R CitrateNetwork/citrate-core
```

A local build stages them as release.yml does: extract the tarball into `src-tauri/llama/` and
copy the sidecar to `src-tauri/binaries/llama-server-aarch64-apple-darwin`. Before pinning, check
with a small model in a scratch directory that `GET /health` answers, a chat completion returns
text, `GET /` returns 404 (no UI even without `--no-webui`) and `--list-devices` shows Metal.
Windows builds llama-server from source at the same tag with `-DLLAMA_BUILD_WEBUI=OFF
-DLLAMA_USE_PREBUILT_UI=OFF` and upstream's `windows-cpu` x64 options (docs/RELEASE_WINDOWS.md);
the Linux build (b9438) is already web-UI-free.

A runtime packed from a whole CMake install tree (as before rc.4) carries each dylib three times
(`libggml.dylib`, `libggml.0.dylib`, `libggml.0.23.0.dylib`) plus llama.cpp tool libraries
llama-server never loads; the rc.4 tarball is already the closure. Before a build,
`node scripts/prune-llama-runtime.mjs --dir src-tauri/llama` keeps only llama-server's dependency
closure (read with `otool -L`; it refuses if a needed library is missing) and saves about 34 MB.
release.yml runs it after extracting the tarball; run it on a local staging too (HUP-S11.0).
`scripts/build-hermes.sh` installs the Hermes sidecar with `strip -x` (about 4 MB). The pin is of
the stripped bytes; for a debuggable build run cargo in citrate-agent-runtime and point
`CITRATE_HERMES_BIN` at it.
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

## Dry run
Dispatch `release` with `dry_run: true` from any branch (the `tag` input is only a label then). It
builds the six cargo sidecars from their pinned revs on a clean runner with the scripts above,
stages the pinned llama runtime, BGE models, corpus and Kubo, bundles the app **unsigned** with no
updater artifacts, and writes the sidecar digests (against their pins), the bundle checksums and
the size report to the run summary. It reads no secret, signs, notarizes, uploads and publishes
nothing, and creates no tag or release; `component-pins.mjs check` fails CI if a signing or
publishing step loses its `env.DRY_RUN != 'true'` guard. The runner's sidecar digests differ from
the pins (another build root), so a dry run proves the sources build and bundle, not the pinned
bytes.

## First-run caveat (honest)
Every tag run of this workflow so far (v0.4.2 to v0.5.1, and the Windows tags) failed at staging,
before any build: v0.5.0-rc.1 found no skills bundle on `runtime-deps`, the later runs no Gemma GGUF
("no assets match the file pattern"). The repository also has no signing secrets. macOS releases are cut by the local ceremony on the release Mac. A tag run needs,
in order: the six pinned sidecar assets uploaded to `runtime-deps`, then the secrets above. The
Apple signing + notarization legs and the signed feed are what the first green tag run proves.
