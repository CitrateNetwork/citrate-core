---
created: 2026-10-26
branch: ci/rc52-updater-e2e
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: active
wp: RC52 G8 (updater end to end)
---

# Updater end to end (RC52 G8)

The in-app updater (tauri-plugin-updater 2.10.1, `src/shell/updater.ts`) is the only way a 0.5.1
install learns about 0.5.2. Two automated layers now cover it, and one release step stays manual.

| Layer | What it proves | Where it runs |
|---|---|---|
| Unit: `upgrade_updater_feed_tests` | The plugin's own `check()` and `download()` (download verifies the signature before it returns a byte) against goldens signed with a throwaway key, served from loopback in a mock app: a newer signed update is offered and verifies; an altered signature, another key's signature and an altered artifact are refused with the verifier's error; a feed that is not newer offers nothing and downloads nothing. Plus the existing 0.5.1 contract tests (pinned v2 key and endpoints, the published 0.5.1 feed) and a tripwire (below). | `cargo test -p citrate-core --lib upgrade_updater_feed` on macOS and Linux only (CI `ci`, `src-tauri lifecycle (macOS-14)`). On Windows the mock-runtime plugin tests are compiled out (tauri's `test` runtime does not load there; Windows ships no updater artifacts); the golden, guard and tripwire tests still run. |
| Packaged: K1-L `update-e2e` | The packaged AppImage, in the K1-L sealed namespace: a test build N finds N+1 on a loopback feed, verifies it, installs it over its own AppImage and restarts into it. The same five refusals leave N running, the AppImage unchanged and no restart. | CI `linux-native-acceptance` → `updater e2e (x86_64)` and `(aarch64)`; or by hand on the DGX |
| Manual: release checklist §3 rehearsal and §8 human checks | A real 0.5.1 install updates to the real RC through a staging feed signed with the real v2 key, on macOS and Linux. | A test machine, never the release Mac |

Fixtures: `fixtures/updater-e2e/` (two throwaway public keys, an 8 KiB stand-in artifact, its
signatures by each key, a `latest.json` in the 0.5.1 feed shape). The private keys were deleted
after signing; nothing in this repo can sign an update.

## How the packaged scenario works

1. `scripts/ci/linux-native-acceptance/update-e2e-build.sh --out DIR` generates two throwaway
   keys (A, B) with `tauri signer generate`, writes two `--config` overlays and builds two
   AppImages from this tree with the `updater-e2e` cargo feature:
   - N (version 0.5.1) pins key A and the endpoint `http://127.0.0.1:18477/latest.json`, with
     `dangerousInsecureTransportProtocol` (plain http to loopback only);
   - N+1 (version 0.5.2), signed with A (`tauri signer sign`, as the release signs its Linux
     artifacts) and, for the other-key case, with B.
   The app will not start without its bundled sidecars, so both builds bundle the sidecars of the
   published 0.5.1 `.deb` (`--sidecars-from`; the package K1-L fetches, sha256-checked against
   `pins.txt`). The private keys are deleted when the script exits. `TAURI_SIGNING_*` is cleared first, and the
   shipping `tauri.conf.json` is not changed. `--overlay-only` writes the overlays and stops (any OS).
2. `scripts/ci/linux-native-acceptance.sh --update-e2e DIR` enters the K1-L namespace
   ([LINUX_NATIVE_ACCEPTANCE.md](LINUX_NATIVE_ACCEPTANCE.md): no host processes, files, display or
   network), serves the feed with `python3 -m http.server` on 127.0.0.1 inside it, and runs six
   cases, each from a fresh copy of N. The app, the reference extraction of N+1, the feed server
   and the feed and signature helpers all start through the harness's `CONFINE` prefix, like
   every other process after the seal (the self-test's `confine-check.py` enforces this):

   | Case | Feed | Expected |
   |---|---|---|
   | `same-version` | version = N, valid signature | `no-update`; artifact not downloaded |
   | `older-version` | version 0.0.1 | `no-update`; artifact not downloaded |
   | `altered-signature` | N+1, one signature bit flipped | `refused … The signature verification failed` |
   | `other-key` | N+1 signed by key B | `refused … The signature was created with a different key than the one provided` |
   | `altered-artifact` | N+1 with its last byte changed, A's signature | `refused … The signature verification failed` |
   | `signed-update` | N+1 signed by A | installs and restarts into N+1 |

   For each refusal it also checks, after `K1L_UE_SETTLE` seconds: N is still running, it started
   once, and the AppImage file is byte-identical to N. For the update it checks: the AppImage file
   is byte-identical to N+1, the old process is gone, the new process runs the `citrate-core`
   binary of N+1 (sha256 against an extraction of N+1, independent of the app's log), and N+1 on
   the same feed reports itself up to date. Every case ends with the K1-L quit check (no process
   and no listening socket left).

### The driver

The update flow in the app is click-driven ("Update available", Install, Restart), and K1-L has no
click driver. A build with the `updater-e2e` feature (`src-tauri/src/updater_e2e.rs`) makes the same
plugin calls itself once at start-up and prints one `updater-e2e:` line per step:

| UI step (`src/shell/updater.ts`) | Driver call |
|---|---|
| `check()` | `app.updater()?.check()` |
| `downloadAndInstall()` | `update.download_and_install()` (verifies, then installs) |
| `relaunch()` (process plugin) | `app.request_restart()` (what `relaunch` calls) |

It does nothing unless `CITRATE_UPDATER_E2E` is set, the pinned key is neither release key (v1
or v2, compared by key id and key bytes), and every endpoint is on loopback (unit-tested in `updater_e2e::tests`). The banner and buttons
themselves are covered by `src/shell/updater.test.ts`, not by this scenario.

### Tripwire

`no_release_config_or_release_path_relaxes_the_updater_or_enables_the_e2e_driver` fails if:

- any `src-tauri/tauri*.conf.json` carries a `dangerous*` updater flag, names `updater-e2e`, or (as
  an overlay) overrides the updater key or endpoints;
- any feature in `src-tauri/Cargo.toml` turns `updater-e2e` on, directly or through other
  features (`default` included);
- any workflow under `.github/workflows/` or any file under `scripts/` (all subdirectories) names
  `updater-e2e`, except the K1-L harness (`linux-native-acceptance.yml`,
  `scripts/ci/linux-native-acceptance.sh`, `scripts/ci/linux-native-acceptance/update-e2e-build.sh`);
- `release.yml` or any file under `scripts/` builds with `--all-features`.

Its reach is textual: a build command assembled at run time from pieces, a workflow outside
`.github/workflows/`, or a manual `--features updater-e2e` on the release Mac would not be seen.
The driver's own guard is the second line: it refuses to act in any build that pins the v1 or v2
release key (compared by key id and key bytes, not by text) or polls a feed off loopback.

## Running it

CI: the `update-e2e` job of `.github/workflows/linux-native-acceptance.yml` runs on pull requests
that touch the harness, the driver, the updater config, `src-tauri/Cargo.toml`, `Cargo.lock` or the
fixtures, and on `workflow_dispatch`. It uploads logs only (`summary.txt`, the app's log per case,
the feed server's request log); the AppImages and keys are not uploaded.

By hand on the DGX (aarch64), from a checkout with `npm ci` done, never in the desktop session
outside the harness:

```sh
scripts/ci/linux-native-acceptance.sh --fetch aarch64 --cache "$PWD/target/k1l-cache" --fetch-only
scripts/ci/linux-native-acceptance/update-e2e-build.sh --out "$PWD/target/k1l-ue" \
  --sidecars-from "$PWD/target/k1l-cache/to.deb"
scripts/ci/linux-native-acceptance.sh --update-e2e "$PWD/target/k1l-ue" \
  --work "$PWD/target/k1l-work" --log-dir "$PWD/target/k1l-ue-logs"
```

Do not run the built AppImages outside the harness: they install whatever a loopback feed offers.

## Mapping to the release checklist

The 0.5.2 checklist is `citrate-federation` `.agentile/planset/2026-10-19-betanet/RELEASE_052_CHECKLIST.md`.

| Checklist item | Covered here | Still manual |
|---|---|---|
| §3 "Update path rehearsal: a machine on 0.5.1 pointed at a staging feed signed by the v2 key updates to the RC and restarts into it" | The mechanism, on Linux AppImage: feed parse, version compare, signature check against the pinned key, install over the running AppImage, restart into the new binary, and the refusals. | The real pair: a 0.5.1 build from the release (not a test build), the RC artifacts, the v2 key, the staging feed over https, macOS `.app.tar.gz` and Linux `.deb`. Do it on a test machine; never on the release Mac's own install, and never run install scripts on the release Mac. |
| §4 "Verify every `.sig` with `minisign -V`" | `the_golden_signature_verifies_over_the_artifact_as_minisign_v_checks_it` checks the same thing with the plugin's verifier crate, on the goldens. | The real `.sig` files against the v2 pubkey. |
| §8 "a 0.5.1 install shows 'Update available' and restarts into 0.5.2" | The restart into the newer version (Linux AppImage). | The banner itself on a real 0.5.1 install, macOS and Linux, against the published feed. |
| §8 CDN `latest.json` = GitHub fallback; every URL byte-identical | Not covered. | Manual parity check. |

## Limits

- **Linux AppImage only.** macOS (`.app.tar.gz` replace + relaunch) and the `.deb` path (`pkexec
  dpkg -i`) are not exercised; both stay in the manual rehearsal.
- **Test builds, not the release pair.** N and N+1 are built from the same tree with a throwaway
  key. A difference between the shipped 0.5.1 binary and this tree's updater code (the plugin is
  pinned at 2.10.1 in both) would not show here; the manual rehearsal covers that.
- **Plain http to loopback.** The test feed has no TLS (`dangerousInsecureTransportProtocol`, test
  overlay only); the production endpoints are https, enforced by the plugin in release builds.
- **AppImage runs extracted** (`APPIMAGE_EXTRACT_AND_RUN=1`), as in the rest of K1-L; the FUSE
  mount path is not covered. The install writes the AppImage file itself, which is the same in both.
- **Sidecars from 0.5.1.** The test builds bundle the published 0.5.1 sidecars, whatever this tree
  pins; the scenario covers the updater, not the sidecars (K1-L `update` covers those).
