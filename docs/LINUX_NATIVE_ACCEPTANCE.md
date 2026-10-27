---
created: 2026-10-22
branch: ci/k1l-linux-native-acceptance
author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
status: active
wp: K1-L (SCL-S14.6)
---

# Packaged Linux acceptance (SCL-S14.6)

`scripts/ci/linux-native-acceptance.sh` runs the **packaged** Linux build of Citrate Core, the
published `.deb` and AppImage, through the S14.6 scenarios on a real Linux machine, and fails if
any Citrate-started process or listening socket is left behind. The plan, the criteria and what
is not covered are in
[`.agentile/sprints/active/sprint-scl-s14-native/S14.6_LINUX_PLAN.md`](../.agentile/sprints/active/sprint-scl-s14-native/S14.6_LINUX_PLAN.md);
the recorded runs are in `EVIDENCE.md` next to it.

The supervisor fault suite (unit level, no packaged app) is the separate K1-F lane:
[`LIFECYCLE_FAULTS_LINUX.md`](LIFECYCLE_FAULTS_LINUX.md). Both lanes share the stray-process
scan in `scripts/ci/lib/pidns.sh`.

## Why this does not endanger a desktop session

Starting Citrate Core in the DGX's desktop session has ended that session before (core#318: every
`systemd --user` unit was killed shortly after the app exited). The harness never starts the app
in the caller's session. It re-runs itself as PID 1 of new user, PID, mount, network, IPC and UTS
namespaces (`unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts setsid`,
the same entry as K1-F plus `--net --ipc --uts`), and then, inside them:

| What the app could otherwise reach | How it is cut off |
|---|---|
| Host processes (signals) | PID namespace; PID 1 is the harness and leads its own session and group, so `kill(-1)` and `kill(0)` stay inside |
| The systemd user manager and the session bus | `/run` is a private tmpfs (no `/run/user/<uid>`, no system bus socket); the network namespace makes the host's abstract sockets (`@…/bus/systemd/bus-api-user`, `@/tmp/.X11-unix/X1`, …) unreachable; the app gets its own `dbus-daemon` and `gnome-keyring-daemon` |
| cgroup kills | `/sys/fs/cgroup` is a private tmpfs |
| The desktop's X server and GPU | `/tmp` is private, the abstract X socket is unreachable, `/dev` is a minimal tmpfs (null, zero, full, random, urandom, tty, own devpts; no `/dev/dri`, no `/dev/nvidia*`); the app draws on a private `Xvfb :77` |
| Your files | Every mount inherited from the host is read-only inside (one recursive `mount_setattr`, other mount flags unchanged; only the namespace's own `/proc` stays writable), so files you own anywhere, such as data directories under `/opt` or `/var/lib`, cannot be changed; `/home`, `/root`, `/tmp`, `/var/tmp`, `/dev/shm` are private tmpfs; the run directory is the only writable host directory, and its staged packages (possibly hard links to your downloads) and the harness copy are read-only binds; HOME and every XDG dir point into the run directory; the harness moves into the run directory before anything runs, so no working directory still points at a mounted-over directory |
| Undoing any of the above | Everything started after the seal (the app, the package's install scripts, the AppImage extraction, Xvfb, the bus, the keyring and the harness's X and bus clients) runs in one nested user and mount namespace made from the finished seal, where every inherited mount is locked (its read-only flag cannot be cleared and the mounts over hidden directories cannot be removed), and `setpriv` drops every capability and sets `no_new_privs` before exec. Such a process has no capability in the seal's namespace, so it can neither change those mounts nor enter that namespace |
| The network (and production RPC) | Loopback only |

Before the first launch the harness checks each of these and records it in `summary.txt`
("probe ok: …"). The abstract-socket check has a positive control (a socket made inside must be
reachable), the stray-process scan and the listening-socket scan each have a negative control (a
leaked `sleep` and a new listener must be seen). Two probes cover paths that are not absolute:
no process may hold a working directory, root or open directory that the seal mounted over, and
from the working directory no `.`, `../` chain, `./src`-style child or symlink (to the checkout,
the caller's working directory, HOME, `/proc/1/cwd`, `/proc/self/cwd`, or any symlink in the run
directory) may resolve to the caller's checkout, working directory or HOME, compared by
device and inode recorded before entry. Their negative controls use handles taken before the
seal (on the host `/tmp` and on the checkout), which both scans must flag. A checkout outside
the hidden directories (for example under `/opt`) stays reachable, so that probe fails and the
harness refuses to run. Three probes cover writes: every inherited mount must be read-only; no
path the caller can write outside the namespace (their own and world-writable files and
directories, found per real filesystem to depth 3 before entry, plus the checkout, working
directory, HOME and package sources) may be writable inside, checked with `access(W_OK)` and, for
files, an open for appending that writes nothing; and the staged packages and the harness copy
must be read-only. Their negative control is a directory and a file in the run directory, which
the scan must flag as writable. One more probe starts a fixture
(`scripts/ci/linux-native-acceptance/unseal-attempt.sh`) through the same launcher as the app and
has it try to undo the seal with mount operations only: remount `/` and the harness copy
read-write (plain and bind), clear the read-only flag with `mount_setattr`, unmount `/home` (plain
and lazy) and the harness bind, and enter PID 1's mount namespace. Every attempt must be refused,
and the fixture must report no effective capability, `no_new_privs` set, and a user and mount
namespace other than PID 1's; the seal probes are then run again. **If any probe fails, the app is not started** and the run exits 2.

Mounts are made in the run's private mount namespace and never propagate to the host. When the
run's PID 1 exits, the kernel ends every process left in the namespace, so nothing outlives a run
even if the harness itself is killed.

## Running it on the DGX (aarch64)

```sh
# packages: the published 0.5.1 and 0.4.2 aarch64 builds, sha256-checked against pins.txt
scripts/ci/linux-native-acceptance.sh --fetch aarch64 --cache "$PWD/target/k1l-cache" --fetch-only
scripts/ci/linux-native-acceptance.sh --fetch aarch64 --cache "$PWD/target/k1l-cache" \
  --work "$PWD/target/k1l-work" --log-dir "$PWD/target/k1l-logs"
```

or with local builds: `--deb NEW.deb --deb-from OLD.deb --appimage NEW.AppImage --appimage-from OLD.AppImage`
(any subset; at least one of `--deb` / `--appimage`). First run the harness's own check:

```sh
scripts/ci/linux-native-acceptance.sh --self-test
```

It packages a stand-in app (`scripts/ci/linux-native-acceptance/fake-app.sh`, copies of `sleep`
as sidecars) as two real `.deb` versions in five modes and requires: the good mode passes;
"leaks a sidecar on quit" fails `quit` only; "does not restart a killed sidecar" fails `restart`
only; "does not stop an earlier run's orphans" fails `reap` only; "runs two instances of the
restart target and restarts neither" fails `restart` only (an instance that was already running
before the kill does not count as the restart).

A run takes about 10 minutes per package kind. `K1L_KEEP=1` keeps the run directory (the app's
HOME, logs, the installed tree) for inspection; otherwise only `logs/` is kept.

Requirements: util-linux `unshare`, `nsenter`, `setpriv`, `dpkg`, `Xvfb`, `dbus-daemon`, `python3`, `iproute2`
(`ss`, `ip`), and the package's runtime libraries (`libwebkit2gtk-4.1-0`, `libgtk-3-0`).
`gnome-keyring-daemon` is used when present.

## What it does per package kind

1. `update`: install the old package (`dpkg -i` into a private root for the `.deb`; the AppImage
   file is put in place), launch, quit, install the new package over it with the same HOME,
   relaunch, and check that every running sidecar is byte-identical to the new package's binary.
2. `launch`: the required sidecars (`K1L_REQUIRED`, default `citrate`) run under the app within
   `K1L_LAUNCH_TIMEOUT`; the sidecars the app starts by itself and its window are recorded.
3. `restart`: SIGKILL the node sidecar; a new process of the same binary must appear under the app.
4. `quit`: send `WM_DELETE_WINDOW` to the app's windows (what a window manager's close button
   sends; `scripts/ci/linux-native-acceptance/x11_windows.py`), then require the app to exit and,
   within `K1L_QUIT_TIMEOUT`, no process other than the session's own services and no listening
   TCP, UDP or unix socket of the app's to be left.
5. `reap`: launch, SIGKILL the app process only, record which sidecars survive it, relaunch, and
   require every survivor to be gone within `K1L_QUIT_TIMEOUT`; then quit cleanly again.
6. `final`: after the session services are stopped, nothing may be left in the namespace.

The session's own services are Xvfb, the private bus and keyring, and anything the bus starts on
the app's request (portal, gvfs, at-spi, dconf). The bus runs as a child subreaper so those stay
its descendants; they are listed in the summary as "session services … (not counted)". On a
desktop they belong to the login session, not to Citrate.

7. `update-e2e` (only with `--update-e2e DIR`, RC52 G8): the app's own updater on a test
   AppImage that pins a throwaway key: it installs a signed newer build from a loopback feed and
   restarts into it, and refuses an altered signature, another key, an altered artifact and a feed
   that is not newer. How the inputs are built, the cases and the checklist mapping:
   [UPDATER_E2E.md](UPDATER_E2E.md).

## Limits

- **AppImage runs extracted.** Inside an unprivileged user namespace the setuid `fusermount`
  cannot mount the AppImage, so the AppImage runs with `APPIMAGE_EXTRACT_AND_RUN=1`. The FUSE
  mount path a member gets by default is not exercised here.
- **No sign-in.** The app stops at "Sign in" (interactive OIDC). Only what starts without a
  signed-in member is covered: the node, IPFS and the memory daemon. Hermes, llama-server
  (needs a downloaded model) and the comms/cluster daemons are not started.
- **Offline.** Loopback only, so the node never peers or syncs.
- **Root inside the namespace, without capabilities.** The app runs as root of the nested namespace
  (your uid outside) with no capability and `no_new_privs` set.

## CI

`.github/workflows/linux-native-acceptance.yml` runs the self-test on `ubuntu-24.04`, then the
packaged scenarios on `ubuntu-24.04` (x86_64) and `ubuntu-24.04-arm` (aarch64), fetching the
packages pinned in `scripts/ci/linux-native-acceptance/pins.txt`. The `update-e2e` job builds its own two
test AppImages first ([UPDATER_E2E.md](UPDATER_E2E.md)). As with K1-F, the job lifts
`kernel.apparmor_restrict_unprivileged_userns` only on the throwaway runner and only if the
`unshare` probe fails. Never change that sysctl on a desktop.
