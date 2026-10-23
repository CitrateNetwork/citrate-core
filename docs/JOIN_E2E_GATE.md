---
created: 2026-10-23
branch: test/join-e2e-gate
author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
status: active
wp: Join-E2E (betanet board, DGX, due 2026-11-20)
---

# Join-E2E: the fresh-machine join gate

**The gate:** on a fresh machine, a person who gets a direct invite link ends up as a member of
the inviter's group, through the published path: **link → join page → installer receives the
invite → install → first run redeems the invite → member is in the inviter's group.** It has to
pass on macOS (Apple Silicon), Ubuntu x86_64 and the DGX (Ubuntu aarch64).

"Fresh machine" means no Citrate install, no Citrate data and no keyring entries: a new HOME,
session bus, keyring and display. On Linux the harness builds one per case inside a sealed
namespace. On macOS it's a new user account (see the [macOS runbook](JOIN_E2E_MACOS_RUNBOOK.md)).

The gate is **not met** while any step below is pending. Each step is "runtime-proven" only when
the harness observed it on that OS. "Wired" or "unit-tested" doesn't count.

## Steps, pass criteria, observables

| # | Step | Passes when | Observed by | Fails when (examples) |
|---|---|---|---|---|
| 0 | Inviter makes a direct invite | A direct code is reserved (`pending`), then a sealed envelope is attached (`valid`), through the landing join store | `join_codes` row: `kind=direct`, `status=valid`, `env_hash` = what attach returned | Quota or store refusal; attach `not_found`/`conflict` |
| 1 | Link → join page | `/join/<code>#<key>` in a real browser offers exactly `curl -fsSL https://citrate.ai/install.sh \| CITRATE_JOIN='<code>#<key>' sh` and the link `citrate://join/<code>#<key>`. No request leaves the site's origin. The key never reaches the server, and viewing the page spends no resolve | Headless Chromium DOM (`.join-command pre`, `a[href^="citrate://join/"]`). Request log of the page (every off-origin request is aborted and counted). The key is grepped in the landing log, the DB proxy log and the `join_codes` rows. `resolve_count` stays 0 | Expired, revoked, unknown or exhausted code (the page offers no command). Link missing its key. The page leaks the key (log, DB, an off-origin request). The page spends a resolve |
| 2 | Installer receives the invite | The page's command, run as given (only the URL swapped for the local copy), is accepted, and the installer hands the invite to the first run: Linux, as argv `citrate://join/<code>#<key>`; macOS, as the launch URL Apple Event (`open -a … <url>`). It never prints the key and never leaves `CITRATE_JOIN` in the app's environment | Linux: `/proc/<app>/cmdline`, `/proc/<app>/environ`, installer output. macOS: the app receives the URL (step 4b-i holds it), plus the installer output | The installer drops the invite (app started without it). Malformed `CITRATE_JOIN` rejected. Key printed. `CITRATE_JOIN` inherited by the app |
| 3 | Install | Linux: the release AppImage lands at the install dir with the published sha256, plus the `citrate-core` command link and an app-menu entry with `MimeType=x-scheme-handler/citrate;` and `Exec="<app>" %u`. The app starts its node sidecar, and quitting leaves no process or listening socket. macOS: `Citrate Core.app` passes the codesign + Team ID check and is installed and opened | File hashes and links, the `.desktop` entry, the process tree (sidecars), the K1-L quit and leftover scans | Checksum mismatch. No handler entry. The app exits during start-up. A sidecar or socket left after quit |
| 4a | Service side of the first run | `GET /api/join/<code>` returns a resolve signed under the **pinned** key (EdDSA, `kid` = pin, issuer, `sub` = code, not expired). Every response field equals its signed claim, `envHash` = SHA-256(env), and the envelope opens under the link key with AAD `citrate-join-v1\|direct\|<code>`. `resolve_count` becomes 1 | Harness-side reference check (`join-e2e/join_crypto.py`, written from the contract and checked against core's known answers) | Expired code (404). A key that isn't the pin. Tampered field or envelope |
| 4b-i | First run holds the invite | The cold start takes the link from its launch argument and holds it as the pending invite before the member does anything, and nothing is resolved yet | The private keyring has an item for account `join-pending-v1` (searched by attribute; the secret is never read). `resolve_count` = 0 | No pending invite (the link was lost on cold start). A resolve spent before the member acted |
| 4b-ii | First run redeems | The member's Review then Join in the app resolves (pin check in Rust), opens the envelope and starts the relay join | `resolve_count` 0→1 at Review. The member daemon's join enters `requesting`/`waiting` | Signature or pin refusal. Envelope doesn't open. The join is never started |
| 5 | Member is in the inviter's group | The inviter's daemon admits the request (online, or the next time it is online), the invitee's daemon joins the MLS group, and both see the same roster | Relay: the group's roster and epoch. Inviter and invitee: group list and member count | Request never delivered. Admit refused. Welcome expired. Epoch race |

## Status on 2026-10-23

| Step | DGX aarch64 (local, sealed namespace) | Ubuntu x86_64 (CI, `join-e2e-linux.yml`) | macOS |
|---|---|---|---|
| 0 inviter (through the store) | PASS | wired; needs `JOIN_E2E_LANDING_TOKEN` | MAC lane |
| 1 link → join page | PASS | as above | MAC lane |
| 2 installer receives the invite | PASS | as above | MAC lane |
| 3 install | PASS (0.5.1 AppImage) | as above | MAC lane |
| 4a service side of the first run | PASS | as above | MAC lane |
| 4b-i first run holds the invite | PASS with a test build of core#355 @ `e65a6688`; PENDING on the 0.5.1 release (no v2 join) | PENDING (no test build in CI) | MAC lane |
| 4b-ii first run redeems | PENDING | PENDING | PENDING |
| 5 member in the inviter's group | PENDING | PENDING | PENDING |

Negative controls, each of which must fail its own step (the harness self-test): an installer
that drops the invite (step 2), an expired invite (steps 1 and 4a), and a resolve checked against
a key that isn't the pin (step 4a). With a test build, the drop-invite case also fails 4b-i.

### What the pending steps wait on

- **4b-i on the release build.** The 0.5.1 release has no v2 join handling. 4b-i runs only
  against a test build (below) of a ref that has core#355. It becomes a plain gate step once
  #353 and #355 are in a release.
- **4b-ii.** core#353 (resolve/open in Rust), core#355 (pending invite, Review/Join), and core#375
  (the join goes through the rendezvous) must be on one ref (today #355 and #375 are siblings
  on #353). It also needs a driver for the member's explicit Review and Join clicks in the
  webview (an AT-SPI or WebDriver driver inside the namespace; not built yet).
- **5.** The relay and member-daemon rendezvous (citrate-comms tasks R1–R4 in
  `citrate-federation/.agentile/planset/2026-10-19-betanet/JOIN_GROW_S2_RELAY.md` §15; no PR
  yet), core#373/#374 (the inviter opens the rendezvous, states), core#375, and an inviter app or
  daemon on the local relay. The local relay already runs in the stack and answers `/health`.
- **Mint through the routes.** Step 0 goes through the landing store's `reserveCode` and
  `attachEnvelope` (the calls `POST /api/join` and `PUT /api/join/<code>` make), not through the
  routes. The routes check the bearer at `https://auth.citrate.ai/me`, a constant in
  `src/lib/join-auth.ts`, and the gate makes no production call. Covering them needs a local
  identity provider (citrate-identity) or a configurable `/me` URL in landing.

## Evidence

2026-10-23, DGX spark-2e01 (aarch64, kernel 6.14.0-1015-nvidia), harness at `87d5a95c`, landing
`feat/join-v2-page` @ `d0385a2`, relay from citrate-comms `main` @ `22b925a`, Chromium headless
shell 1243.

- **Release app** (0.5.1 AppImage `c9205242…`, from `pins.txt`), `--self-test`: good case steps
  1, 2, 3, 4a PASS; 4b and 5 PENDING; `SELF-TEST: PASS`. Each broken case failed exactly its own
  steps: drop-invite `[2]`, expired `[1 4a]`, rogue-key `[4a]`.
- **Test build** (core#355 @ `e65a6688`, repacked into the 0.5.1 AppImage, `79cdfad0…`),
  `--self-test`: as above plus **4b-i PASS**. The cold start held the invite in the keyring 1 s
  after start-up, with `resolve_count` 0. Drop-invite failed `[2 4b-i]`. `SELF-TEST: PASS`.
  The plain run (good case only) ended `RESULT: PASS (automated steps)  GATE: NOT MET while 2
  step(s) are pending`.
- **Red first:** at `5bb32b6c` the step verdicts are stubs. The same self-test went
  `SELF-TEST: FAIL (3 case(s) misjudged)`: every broken case passed. With the verdicts restored
  (`87d5a95c`) it passed. That stubbed-versus-real pair is the revert check.
- Every run: the 11 isolation probes passed before anything started (the 10 from K1-L, plus "nothing
  off loopback reachable"), and nothing was left in the namespace at the end.

## Refs under test

| Repo | Ref | Why |
|---|---|---|
| citrate-landing | `feat/join-v2-page` (landing#128, stacked on #123; #121 is merged) | v2 page, installer pass-through, mint/attach store |
| citrate-core (app) | release 0.5.1 AppImage (`linux-native-acceptance/pins.txt`); test build of `beta/join-11-12-ui` (core#355) for 4b-i | v2 join is unmerged |
| citrate-comms (relay) | `main` | rendezvous not written yet |

Move each ref to `main` as its PR merges. The harness takes the landing checkout, the relay
binary and the app as arguments, so moving a ref changes no code.

## Not covered

- The mint routes' bearer check (above). Real production keys (the gate pins a throwaway key per run or per test build).
- The OS dispatching a clicked `citrate://` link to the installed app (the "Open invite in
  Citrate" button). The handler entry is checked; `xdg-open` needs a desktop session the
  namespace doesn't have.
- The AppImage FUSE mount (it runs with `APPIMAGE_EXTRACT_AND_RUN=1`, as in K1-L) and the `.deb`
  route (the one-line installer installs the AppImage on Linux).
- Network failures between steps (the stack is local and healthy). Those belong in the app's own
  fault tests.

## Running it (Linux)

```bash
# landing checkout at the ref under test, with deps; relay from comms main; Chromium for the page
git -C citrate-landing worktree add --detach /tmp/je-landing origin/feat/join-v2-page
(cd /tmp/je-landing && npm ci && npx playwright install chromium-headless-shell)
git -C citrate-comms worktree add --detach /tmp/je-comms origin/main
(cd /tmp/je-comms && cargo build --release -p comms-relay)

scripts/ci/join-e2e.sh --self-test --landing /tmp/je-landing \
  --relay /tmp/je-comms/target/release/comms-relay
```

Run it **only** through the script on the DGX. The app starts nowhere but inside the namespace
(see [`LINUX_NATIVE_ACCEPTANCE.md`](LINUX_NATIVE_ACCEPTANCE.md) for the isolation probes it
inherits). The script adds one probe of its own: nothing off loopback is reachable, production
hosts included, with a positive control.

**Test build for 4b-i.** Until the key ceremony, `JOIN_PINNED_KEYS` is empty and `JOIN_API_BASE`
is production. `scripts/ci/join-e2e/build-test-app.sh` makes a throwaway key, patches exactly
those two constants in a throwaway worktree at the ref under test, builds the app binary and
repacks the release AppImage around it. Then add `--test-build <out>` to the command above. Never
publish or install a test build anywhere else.

## Files

- `scripts/ci/join-e2e.sh`: the harness. The outer phase stages; the inner phase is PID 1 of the namespace.
- `scripts/ci/join-e2e/neon-http-shim.mjs`: the Neon HTTP SQL endpoint the landing's `neon()` driver
  needs, over the local Postgres (TLS with a throwaway CA).
- `scripts/ci/join-e2e/seed-invite.mts`: step 0 through the landing store.
- `scripts/ci/join-e2e/page-check.mjs`: step 1 in headless Chromium.
- `scripts/ci/join-e2e/join_crypto.py`: throwaway keys, the envelope seal, and the 4a reference check.
- `scripts/ci/join-e2e/build-test-app.sh`: the 4b-i test build.
- `.github/workflows/join-e2e-linux.yml`: the x86_64 CI leg.
- [`JOIN_E2E_MACOS_RUNBOOK.md`](JOIN_E2E_MACOS_RUNBOOK.md): the macOS leg (MAC lane).
