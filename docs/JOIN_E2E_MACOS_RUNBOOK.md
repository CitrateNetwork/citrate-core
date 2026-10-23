---
created: 2026-10-23
branch: test/join-e2e-gate
author: Larry Klosowski + Claude Opus 5.5 (DGX lane), for the MAC lane
status: active (to be run by the MAC lane; not run yet)
wp: Join-E2E (betanet board, DGX, due 2026-11-20)
---

# Join-E2E on macOS: runbook and checklist (MAC lane)

The gate, its steps and pass criteria are in [`JOIN_E2E_GATE.md`](JOIN_E2E_GATE.md). This runbook
is the macOS leg. It runs on the MAC lane's Apple Silicon Mac, by hand, with the same local stack
and helper scripts as the Linux harness (`scripts/ci/join-e2e/`). The DGX lane can't run it.
Record results in the PR thread (or the sprint file) as `PASS` / `FAIL` / `PENDING` per step, with
the observable you saw.

## Ground rules

- **Fresh machine = a new macOS user account** made for the run (System Settings → Users &
  Groups, a Standard user, e.g. `je-fresh`), deleted afterwards. Never your daily account: the
  app's Keychain items and Application Support data must start empty.
- **Offline once the stack is up.** Turn Wi-Fi off and unplug Ethernet before step 1. Loopback
  keeps working, and nothing (the node sidecar, the updater, model download) can reach production.
  Probe: `curl -sS --max-time 5 https://www.citrate.ai >/dev/null && echo ONLINE-STOP || echo offline-ok`.
- **Throwaway keys only.** The test build pins a key made for this run. No real invite key, no
  `JOIN_SIGNING_JWK` from any environment.
- **Test build is local.** Signed with your Developer ID (the installer requires Team ID
  `DDHUG44QC7`), never notarized, uploaded or shared, and deleted with the account.

## 0. Prepare (in your normal account)

1. Checkouts and deps (same refs as the gate's ref table):
   ```bash
   git -C citrate-landing worktree add --detach /tmp/je-landing origin/feat/join-v2-page
   (cd /tmp/je-landing && npm ci && npx playwright install chromium-headless-shell)
   git -C citrate-comms worktree add --detach /tmp/je-comms origin/main
   (cd /tmp/je-comms && cargo build --release -p comms-relay)
   python3 -m venv /tmp/je-py && /tmp/je-py/bin/pip install pynacl   # for join_crypto.py
   brew install postgresql@16                                        # initdb/postgres/psql
   ```
2. **Test build** of the app at the ref under test (today `beta/join-11-12-ui`, core#355). In a
   throwaway worktree, make a key and patch exactly the two constants that `build-test-app.sh`
   patches on Linux:
   ```bash
   git -C citrate-core worktree add --detach /tmp/je-app origin/beta/join-11-12-ui
   mkdir -m 0700 /tmp/je-keys && /tmp/je-py/bin/python -I scripts/ci/join-e2e/join_crypto.py keygen /tmp/je-keys
   read -r kid x < /tmp/je-keys/pin.txt
   # kit/src/join.rs: each line must match exactly once
   #   pub const JOIN_PINNED_KEYS: &[(&str, &str)] = &[];            -> &[("$kid", "$x")];
   #   pub const JOIN_API_BASE: &str = "https://www.citrate.ai/api/join"; -> "http://127.0.0.1:3317/api/join";
   ```
   Then build and sign the `.app` the way you build releases, with sidecars (`npx tauri build
   --bundles app` plus your signing config). Check: `codesign --verify --deep --strict` passes,
   `codesign -dv` shows `TeamIdentifier=DDHUG44QC7`, and `strings` on the main binary contains `$x`.
3. **Loopback CDN layout** (any directory, served with `python3 -m http.server 8717 --bind 127.0.0.1`):
   ```
   downloads/updater/latest.json   {"version":"0.0.0-join-e2e","platforms":{"darwin-aarch64":{"url":"http://127.0.0.1:8717/downloads/vtest/Citrate-Core-darwin-aarch64.app.tar.gz"}}}
   downloads/vtest/Citrate-Core-darwin-aarch64.app.tar.gz   (tar -czf … "Citrate Core.app")
   downloads/vtest/SHA256SUMS      "<sha256>  Citrate-Core-darwin-aarch64.app.tar.gz"
   ```
4. **Stack** (mirror `start_stack` in `scripts/ci/join-e2e.sh`). Postgres on a unix socket in a
   short temp dir. Migrations `0001`… from the landing checkout (skip `0000_init`, as the harness
   does). The TLS cert for `127.0.0.1` from a throwaway CA. `neon-http-shim.mjs`, copied into
   `/tmp/je-landing/.join-e2e/` and run from there. It needs `127.0.0.1:443`; if macOS refuses the
   bind, run only the shim with `sudo`. Then `next dev -H 127.0.0.1 -p 3317` with
   `DATABASE_URL=postgresql://join_e2e@127.0.0.1/join_e2e`,
   `NODE_EXTRA_CA_CERTS=<ca.pem>`, `JOIN_SIGNING_JWK="$(cat /tmp/je-keys/signing.jwk)"`,
   `JOIN_MINTER_SALT=$(openssl rand -hex 24)`, `ENCRYPTION_KEY=$(openssl rand -base64 32)`. The relay
   from `/tmp/je-comms` on `127.0.0.1:18787`, with a 0600 master-key file and owner
   `0x…01`. Check: `curl -s http://127.0.0.1:3317/api/join/jwks` shows `$x`, and the relay's
   `:18788/health` answers.
5. **Invite** (step 0): `seed-invite.mts reserve 3600`, then `join_crypto.py seal <code> <dir>`
   (a well-formed `{g,t,k,n}` on stdin, as in `seed_invite`), then `seed-invite.mts attach <code>
   <dir>/env.txt`. Keep `<code>` and `<dir>/key.txt` for the steps below.

Now go offline (see the ground rules) and log in to the fresh account for steps 1–4. The stack
keeps running in your account; loopback is shared between accounts.

## Checklist (in the fresh account)

| # | Do | Pass when (observable) | Result |
|---|---|---|---|
| probe | `curl -sS --max-time 5 https://www.citrate.ai` | fails (offline); `curl -s http://127.0.0.1:3317/api/join/jwks` works | |
| probe | `security find-generic-password -a join-pending-v1` | not found (fresh Keychain) | |
| 1 | `node page-check.mjs <chrome-headless-shell> "http://127.0.0.1:3317/join/<code>#<key>" page.json` (from the landing dir) | `state=offer`, command = `curl -fsSL https://citrate.ai/install.sh \| CITRATE_JOIN='<code>#<key>' sh`, app link = `citrate://join/<code>#<key>`, `offOrigin=[]`. The key isn't in the landing log or in `select * from join_codes`. `resolve_count=0` | |
| 2 | Make a stand-in `open` in a temp dir (the installer's test mode requires one): `#!/bin/sh` / `printf '%s\n' "$@" >> /tmp/je-open.log` / `exec /usr/bin/open "$@"`, `chmod +x`, and put that dir first on `PATH`. Run the page's command with the URL swapped: `curl -fsSL http://127.0.0.1:3317/install.sh \| CITRATE_JOIN='<code>#<key>' CITRATE_CDN=http://127.0.0.1:8717/downloads CITRATE_INSTALL_DIR=$(mktemp -d)/apps HOME=$(mktemp -d) sh` | Installer exit 0. `/tmp/je-open.log` shows `-a`, `<install dir>/Citrate Core.app`, `citrate://join/<code>#<key>`. The key isn't in the installer's output | |
| 3 | (same run) | The output shows "Checking the Apple code signature" with no error. `Citrate Core.app` is in the install dir. The app opens and its node sidecar runs (`pgrep -lf 'Citrate Core.app/Contents/MacOS/citrate'`). After Citrate Core > Quit: nothing left (`pgrep -lf 'Citrate Core.app/Contents/MacOS'` empty, and no Citrate listener in `lsof -nP -iTCP -sTCP:LISTEN`) | |
| 4b-i | While the first run from step 2 is still open (do this before the quit in step 3), within ~60 s: `security find-generic-password -s ai.citrate.core -a join-pending-v1 >/dev/null && echo held` (never `-w` or `-g`, which print the secret) | `held` (the Apple Event carried the link into the cold start). `resolve_count` still 0 | |
| 4a | `curl -s http://127.0.0.1:3317/api/join/<code> > r.json && join_crypto.py verify r.json /tmp/je-keys/pin.txt <dir>/key.txt` (run 4b-ii first if you do it by hand: this spends a resolve) | `ok kid=join-1 kind=direct …`, `resolve_count=1` | |
| 4b-ii | In the app: Review, then Join on the pending invite | Review: `resolve_count` 0→1, the group name shown is "Join E2E Group". Join: the app shows the join as requesting/waiting. **Expected PENDING** until the relay rendezvous exists (step 5) | |
| 5 | — | **PENDING**: needs citrate-comms R1–R4, core#373/#374/#375, and an inviter on the local relay | PENDING |
| neg | Repeat 1 with a fresh code expired by `UPDATE join_codes SET expires_at = now() - interval '1 minute' WHERE code = '<code2>'` | The page offers no command, and 4a answers `not found` | |

Cleanup: quit the app, log out, delete the `je-fresh` account (and its home), stop the stack,
remove `/tmp/je-*`, and remove the test build. Turn the network back on.

## Note for MAC

- Steps 1, 2, 3, 4a and the negative controls run end to end on Linux in the sealed namespace
  today, and 4b-i ran green on the DGX with a test build of core#355 at `e65a6688` (cold start
  from argv). On macOS the hand-off is the `open -a … <url>` Apple Event instead of argv, so 4b-i
  on macOS is the first real check of `getCurrent()` / `onOpenUrl` from a launch URL.
- 4b-ii is the first place a human can go further than the Linux harness (it can't click Review
  and Join in the webview yet). If you do it, record whether Review spends exactly one resolve
  and what the join shows without a rendezvous-capable relay.
- If a step fails, file it against the PR that owns it (landing#128 for steps 1–2, core#355 for
  4b-i/ii) with the observable, not a guess at the cause.
