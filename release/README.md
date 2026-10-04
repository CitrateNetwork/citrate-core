---
created: 2026-10-01
branch: hup/n3-release-gates (updated on hup/n6-size-licence, 2026-10-04)
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Release gates: installer size budget

Work package **HUP-S11.0** (planset `.agentile/planset/2026-09-30-hermes-upskill/`, 05 row S11.0,
gate `g5-size`: "Core installer within the size budget on every OS").

## What is checked

`scripts/size-budget.mjs` reads a Tauri bundle dir (default `target/release/bundle`) and compares
what it finds with [`budgets.json`](budgets.json):

| Row kind | Id | Measured from |
|---|---|---|
| Installer / updater | `<os>-<arch>/dmg`, `app.tar.gz`, `appimage`, `appimage.tar.gz`, `deb`, `rpm`, `msi`, `nsis` | the file in `dmg/`, `macos/`, `appimage/`, `deb/`, `rpm/`, `msi/`, `nsis/` |
| Binary in the app | `<os>-<arch>/bin/<name>` | `Contents/MacOS/` (macOS), `usr/bin/` in the AppDir or deb payload (Linux) |
| Bundled resource | `<os>-<arch>/resources/<name>` | each top-level entry of `Contents/Resources/` (macOS) or `usr/lib/<product>/` (Linux): `llama`, `models`, `docs-corpus`, `capsules`, ... |
| Whole payload | `<os>-<arch>/app` | the `.app/Contents` or `usr/` tree |

Signatures (`.sig`) and build helpers (`bundle_dmg.sh`) are not artifacts. Symlinks count 0 bytes.
The `dup` column is the bytes held by byte-identical copies inside one component.

Exit codes: `0` within budget, `1` over budget (or a measured row with no budget under `--strict`),
`2` usage error, unreadable budgets file, no installer artifact in the bundle dir, or no artifact
name that carries the arch (pass `--arch`; the check does not guess, since an unknown arch would
match no budget row and pass). Same-size files are hashed in 8 MiB chunks to find duplicates, so a
bundled file over 2 GiB is measured like any other.

```sh
node scripts/size-budget.mjs                                  # target/release/bundle vs release/budgets.json
node scripts/size-budget.mjs --bundle-dir <dir> --arch x86_64 # another build; --arch when names carry none
node scripts/size-budget.mjs --markdown size.md --json size.json   # reports for a PR or release note
node scripts/size-budget.mjs --strict                         # also fail on unbudgeted rows
```

Windows installers are measured as artifacts only (the bundle dir has no unpacked payload).
Linux payload discovery (AppDir / deb `data/usr`) is covered by fixture tests; it has not yet run
against a real Linux build.

## 0.5.0-line measurement and re-set budgets (2026-10-04, pending owner sign-off)

A local bundle-lite build on Apple Silicon (`npx tauri build --config` the lite overlay with
`createUpdaterArtifacts: false`; Developer ID signed, not notarised) of core `hup/m2-core` @ 77e3bdf
plus this branch, with what 0.5.0 adds staged: the full knowledge corpus (digest `9709668…`, 99.7 MB
with BGE vectors), the 240 reviewed skills (8.7 MB), the M2 Hermes sidecar (runtime `hup/m2-runtime`
@ d10f0ee, release profile) and the licence texts (`resources/licenses`, 142 kB).

Against the 2026-10-01 budgets it **failed**: DMG 431.7 MB (104.0% of 415 MB), updater 435.1 MB
(113.0%), payload 852.4 MB (107.0%), `bin/hermes` 29.4 MB (122.7%), `bin/citrate-core` 35.9 MB
(123.6%). Those five rows (and the new `resources/licenses` row) were re-set from this build by the
same rules as before; the result is a strict PASS:

| Id | Measured | Budget | Use |
|---|---:|---:|---:|
| `macos-aarch64/dmg` | 431,684,435 | 455,000,000 | 94.9% |
| `macos-aarch64/app.tar.gz` | 435,081,567 | 460,000,000 | 94.6% |
| `macos-aarch64/app` | 852,410,676 | 938,000,000 | 90.9% |
| `macos-aarch64/bin/hermes` | 29,442,128 | 33,000,000 | 89.2% |
| `macos-aarch64/bin/citrate-core` | 35,851,840 | 40,000,000 | 89.6% |
| `macos-aarch64/resources/licenses` | 142,493 | 1,000,000 | 14.2% |

All other macOS rows stayed within their 2026-10-01 budgets. The bundle output was deleted after
measuring (disk).

Caveats, honestly:

- **Updater row is an estimate.** The build ran without the updater signing key, so the
  `app.tar.gz` row is `tar -czf` of the signed `.app` (gzip level 6, as Tauri uses), not Tauri's
  own artifact. Re-check it on the release build.
- **Four sidecars are the v0.4.2 builds** (`citrate`, `cluster-daemon`, `comms-member-daemon`,
  `node-agent`), and `mem-mcp` is the 2026-10-01 build with `import-corpus`, because the bundled
  sidecars have not been rebuilt from the merged mains yet (A45). Their rows may move when they are.
- **Linux and Windows** stay `null` until the DGX team measures the first builds.
- **Not acted on, two savings found:** `strip -x` on the Hermes sidecar takes it from 29.4 MB to
  25.4 MB; the llama dylib triplication noted below is still about 40 MB.

Owner sign-off: accept these budgets, or ask for the savings first and keep the 0.4.2 budgets.

## Budgets (proposed 2026-10-01, superseded for the rows above)

`maxBytes: null` means measured and reported, not gated. A row with no entry is reported as
`unbudgeted` and only fails under `--strict`.

| Id | Baseline | Budget | Basis |
|---|---:|---:|---|
| `macos-aarch64/dmg` | 394,782,331 | 415,000,000 | shipped v0.4.2 release asset, about +5% |
| `macos-aarch64/app.tar.gz` | 366,205,330 | 385,000,000 | shipped v0.4.2 updater asset, about +5% |
| `linux-x86_64/appimage`, `deb` | none | null | no Linux release yet |
| `windows-x86_64/msi`, `nsis` | none | null | no Windows release yet |
| components (17 rows) | local v0.4.2 build | baseline +10%, next MB | `target/release/bundle`, built 2026-09-30 |

Run against the local v0.4.2 build on 2026-10-01 (Apple Silicon, `node scripts/size-budget.mjs`):
PASS, 0 over budget, 0 unbudgeted. DMG 362.9 MB (87.5% of budget), updater 363.7 MB (94.5%),
payload 724.5 MB, of which `resources/models` (the bundled BGE embedder) is 438.7 MB and
`resources/llama` is 59.6 MB.

The locally built DMG (362,917,500 bytes) is smaller than the shipped v0.4.2 DMG
(394,782,331 bytes), so the artifact budgets are set from the shipped assets, the larger of the two.

### Observation, not yet acted on

`resources/llama` holds each llama.cpp dylib three times (`libggml.dylib`, `libggml.0.dylib`,
`libggml.0.23.0.dylib`, and so on), as regular files of the same size rather than symlinks. They
are not byte-identical (each copy is signed separately), so `dup` reports 0. If the loader only
needs one name per library, restoring the symlinks could save roughly two thirds of that 59.6 MB.
That needs a check of the install names before anyone changes the bundling.

## Where it runs

Locally, after a build, as step 5 of [`docs/RELEASE.md`](../docs/RELEASE.md) "Cutting a release".

In CI it is **manual only**: `.github/workflows/release.yml` has a `size_gate` dispatch input
(boolean, default `false`). A tag push never runs it and builds and publishes exactly as before. A
manual run with `size_gate: true` creates the release as a draft, runs
`node scripts/size-budget.mjs --bundle-dir target/aarch64-apple-darwin/release/bundle --arch aarch64`
into the job summary, and publishes the draft only if the gate passes; an over-budget build stays
a draft for review. Pinned by `scripts/release-size-gate.test.mjs`.

Before anyone dispatches it: `release.yml` builds the **node** overlay
(`tauri.bundle-node.conf.json`), which also bundles the Gemma 4 E4B GGUF, while the shipped
releases and these budgets are the **lite** build made on the release Mac. A CI run with the gate on
would fail the DMG and payload rows until the owner chooses which flavour CI releases (owner call).

Tests: `scripts/size-budget.test.mjs` (fixture bundle dirs with exact byte sizes for macOS and
Linux layouts, budget validation, row statuses at the boundary, CLI exit codes, and the committed
`budgets.json` against the v0.4.2 and 2026-10-04 numbers) and `scripts/release-size-gate.test.mjs`
(the manual-only wiring in `release.yml`).
