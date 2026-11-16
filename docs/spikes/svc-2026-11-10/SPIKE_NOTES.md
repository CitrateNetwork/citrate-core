---
created: 2026-11-11
branch: v60/svc-spike
author: Larry Klosowski + Claude Opus 5.5
status: complete (Linux runtime-proven on the build host; macOS static validation only)
wp: V60 S0-SVC-SPIKE
---

# S0-SVC-SPIKE: the node and kubo headless under a per-user unit

Planset: fed `.agentile/planset/2026-11-07-v060/02_ARCHITECTURE.md` §9 (background service),
`04_FEATURES_BDD.md` F-B1, `05_SPRINTS_AND_WPS.md` S0-SVC-SPIKE. Owner decision 2026-11-07: the
service is on by default, with an opt-out at setup that says a closed app won't produce blocks.

The question for the spike: can a per-user unit run the node and kubo with no app and no login
session, restart them after a crash, and stop them cleanly, before `citrate-svc` exists? And what
does the real service have to solve that the planset does not say yet?

## What ran

`citrate-svc` does not exist yet (SVC-1), so the unit runs a stand-in, [`svc-spike.sh`](svc-spike.sh).
It starts kubo (`ipfs daemon --offline`) and the bundled node, and exits non-zero when either child
dies, so the unit's restart policy is what brings them back. On SIGTERM it drains: SIGINT to the
node (its shutdown path), then SIGTERM to kubo.

The spike node runs on a local network only: `--network local --bootstrap`, loopback RPC and P2P
(`127.0.0.1:18545`, `127.0.0.1:31303`), a throwaway coinbase, kubo offline with an empty
bootstrap list and `Routing.Type none`. It cannot dial any fleet host.

### Linux (runtime-proven)

Host: `citrate-core-linux-build` (Ubuntu 22.04, systemd 249). A throwaway user `citsvcspk`
with lingering on. The binaries are the ones bundled in the 0.5.2 rc2 AppImage
(`/root/w052rc2/out/Citrate-Core-linux-x86_64.AppImage`, extracted): node sha256
`697e0753…5120`, kubo 0.42.0. Unit: [`citrate-svc-spike.service`](citrate-svc-spike.service) in
`~/.config/systemd/user/`.

| Check | Command | Result |
|---|---|---|
| Static validation | `systemd-analyze --user verify citrate-svc-spike.service` (as the user) | exit 0, no warnings |
| Runs with no login session | `loginctl show-user citsvcspk` → `State=lingering`, `Sessions=` (empty); `eth_blockNumber` twice, 15 s apart | `0x27` → `0x36`: the head advances with no session at all |
| Restart after a crash | `kill -9` on the node pid | stand-in exits 1, unit restarts it in 5.2 s (`RestartSec=5`), `NRestarts=1`, `Result=success`, the head keeps advancing |
| Survives a logout | `ssh citsvcspk@127.0.0.1` (a real logind session), then exit | during: one session, unit `active`; after: `Sessions=` empty, unit `active`, head `0x10` and advancing |
| Lingering is load-bearing | `loginctl disable-linger citsvcspk` with no session open | the user manager stops within 5 s and the node is gone |
| Clean stop | `systemctl --user stop` | 0.21 s; node logs `Shutting down...`, kubo logs `Received interrupt signal`; no process left for the user |
| Data survives a stop and start | stop, start, `eth_blockNumber` | `0xb` before and after: the chain data dir is kept |

A note on the node binary: the first run used an older `citrate 0.4.0` build from `/root/gcand`.
On SIGINT it panicked in tokio's runtime drop ("Cannot drop a runtime in a context where
blocking is not allowed") after logging `Shutting down...`. The 0.5.2 rc2 node shuts down
without that panic, so the result above uses it. Not a 0.6.0 finding, recorded so nobody
re-measures with the old binary.

Not tested: a reboot of the build host (it is shared with other lanes). With lingering on, the
user manager starts at boot and starts `default.target.wants`, which is the documented systemd
behaviour, not something this spike observed.

### macOS (static validation only)

No LaunchAgent was loaded on the Mac (house rule: never install a LaunchAgent on the owner's Mac
and never launch the app). [`ai.citrate.svc.spike.plist`](ai.citrate.svc.spike.plist) passes
`plutil -lint` on macOS 15.6.1. A runtime check needs a separate test Mac with a signed build.

## Findings the real service has to solve

1. **The node storage key is in the OS keyring, and headless access differs by platform.**
   `NodeManager::start` reads the node's at-rest storage key from the keyring and fails closed
   when the backend is unreachable. The spike node has no key (it is a throwaway local chain),
   so this is a reading of the code and the host, not a measured failure:
   - Linux: the build host has no Secret Service on the user bus (`busctl --user list` shows no
     `org.freedesktop.secrets`). On a desktop the keyring is unlocked by the login session, so a
     lingering service that starts at boot, before anyone logs in, cannot read the key until the
     member logs in. The service must hold the node start ("waiting for the keyring") and retry,
     not crash-loop and not fall back to a plaintext key. Running the node while logged out (owner
     answer 4) needs an owner decision on where the key lives for that case.
   - macOS: the item was created by the app. A legacy-keychain item's access list trusts the
     creating application, so a second binary (`citrate-svc`) reading it may raise a keychain
     prompt from a background process. Needs a test on a signed build: either the app adds the
     service binary to the item's trusted applications when it creates or first reads it, or the
     service reads it through the app's own executable (see 3).
2. **A LaunchAgent runs only in the member's session.** `LimitLoadToSessionType Aqua` ties it to
   the GUI login, which is also when the login keychain is unlocked. "Runs when logged out" is a
   Linux property (lingering) in this design; on macOS it would need a LaunchDaemon, which has no
   access to the member's keychain. The planset's owner answer 4 is met on Linux only.
3. **AppImage binaries live in a FUSE mount that exists only while the AppImage runs.** A unit
   cannot point `ExecStart` into `squashfs-root/usr/bin`. For an AppImage install the unit has to
   run the AppImage file itself with a service argument, so the AppImage's entry point must be
   able to dispatch to the service; for the `.deb` the binary has a fixed path. SVC-1 ships the
   unit for the fixed-path case and records the AppImage dispatch as an open item.
4. **Lingering may need an admin.** `loginctl enable-linger` for the member's own account is
   polkit action `org.freedesktop.login1.set-self-linger`; on the build host (Ubuntu 22.04,
   systemd 249) it is `yes` for any, inactive and active sessions. Distributions and managed
   machines can tighten it to `auth_admin`, and the planset calls it an admin step. Without
   lingering the service still runs while the member is logged in and stops at logout. Setup must say which one the member got.
5. **Stop timing.** The stand-in's drain took 0.21 s, far inside `TimeoutStopSec=40` and the
   LaunchAgent's `ExitTimeOut 40`. Both are set above the 0.5.3 drain's 30 s exit wait
   (`update_drain::EXIT_WAIT`), so the unit manager never SIGKILLs a drain that is still inside
   its own budget. `KillMode=mixed` sends SIGTERM to the main process only, so the service, not
   systemd, decides the order (node first, then kubo).
6. **Restart policy.** `Restart=on-failure` (Linux) and `KeepAlive { SuccessfulExit = false }`
   (macOS) restart a crash but not a clean exit, so turning the service off in Settings can stop
   it without the manager bringing it back. The planset's plain `KeepAlive` would restart it.
7. **Ports.** The service owns the node's ports (RPC 8545, WS 8546, P2P 30303) and kubo's
   (5001, 4001, gateway). An app that starts its in-process supervisor while the service runs
   would collide on all of them; the existing holder check (`node_holder::find_blocker`) refuses
   that start, which is the right failure, but the app must attach first and only fall back when
   the service is absent (SVC-1).

## What SVC-1 takes from this

- Unit shape: `Type=simple`, `Restart=on-failure`, `RestartSec=10`, `TimeoutStopSec=40`,
  `KillMode=mixed`, `UMask=0077`, `WantedBy=default.target`; LaunchAgent with `RunAtLoad`,
  `KeepAlive { SuccessfulExit = false }`, `ExitTimeOut 40`, `ProcessType Background`,
  `LimitLoadToSessionType Aqua`, `Umask 63`.
- The service exits non-zero when a supervised child is gone for good, and zero after a
  requested stop.
- The node start waits for the keyring instead of failing the service.
- Validation in CI: `plutil -lint` (macOS) and `systemd-analyze --user verify` (Linux) on the
  rendered units.

## Cleanup

The throwaway user `citsvcspk`, its unit, data and the spike key pair on the build host are
removed when the SVC lane closes (it is reused for the SVC-1 runtime check).
