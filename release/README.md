---
created: 2026-10-01
branch: hup/n3-release-gates
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

## Budgets (proposed 2026-10-01)

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
It is **not** wired into `.github/workflows/release.yml` and no workflow runs it automatically:
CI minutes are spent only when someone asks. Adding it to the release job is a single step
(`node scripts/size-budget.mjs --markdown "$GITHUB_STEP_SUMMARY"` style) and is an owner call.

Tests: `scripts/size-budget.test.mjs` (fixture bundle dirs with exact byte sizes for macOS and
Linux layouts, budget validation, row statuses at the boundary, CLI exit codes, and the committed
`budgets.json` against the v0.4.2 numbers).
