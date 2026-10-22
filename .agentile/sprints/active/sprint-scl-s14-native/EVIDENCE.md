---
created: 2026-10-22
branch: ci/k1l-linux-native-acceptance
author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
status: active
gate: g4-linux-smoke-cut (SCL-S14.6); Linux slice only
---

# SCL-S14 native acceptance: evidence

This file is the evidence path `gates.yaml` names for the S14 gates. This first entry is the
Linux S14.6 slice (K1-L). macOS (S14.1, S14.4) and Windows entries belong to their lanes.

## 2026-10-22, DGX (aarch64), S14.6 packaged run: RESULT PASS

- Machine: DGX spark-2e01, aarch64, kernel 6.14.0-1015-nvidia, Ubuntu 24.04, WebKitGTK 2.50.4.
- Harness: `scripts/ci/linux-native-acceptance.sh` at b0c6356a (`summary.txt` prints it), `--fetch aarch64`, x11 quit.
- Packages (published, sha256 = CDN `SHA256SUMS-linux`, pinned in `pins.txt`):
  - to: 0.5.1 `.deb` `a01951553dc8877a…`, AppImage `c9205242157aff0d…`
  - from: 0.4.2 `.deb` `b7583630ea587f75…`, AppImage `0a56b2565dd6cd1c…`
- Isolation: all ten probes ok before the first launch. The host had 18 abstract unix sockets
  (including the systemd user manager's bus and the desktop X server); 0 were reachable inside.
- Desktop session unaffected: the `systemd --user` manager kept the same pid (started
  2026-10-19) through all runs.

Summary (verbatim `summary.txt`; the per-step install and session-service lines are left out):

```
probe ok: pid namespace: PID 1, own session and group, no other process
probe ok: host home hidden (/home/saul)
probe ok: host /run/user and system bus hidden
probe ok: no inherited DISPLAY / WAYLAND / DBUS / XDG_RUNTIME_DIR
probe ok: cgroup filesystem hidden
probe ok: GPU device nodes hidden
probe ok: network: loopback only
probe ok: host abstract sockets unreachable (with positive control)
probe ok: stray-process scan sees a leaked sleep (negative control)
probe ok: listening-socket scan sees a new listener (negative control)
private display :77 up (Xvfb pid 122, namespace-local)
== deb
     deb session: private bus pid 139 (subreaper), keyring pid 143 (default collection unlocked)
PASS deb launch [from] in 4s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
PASS deb quit [from] in 1s: no process and no listening socket left
PASS deb launch [update] in 4s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
PASS deb update: relaunch over the old install runs the new package's sidecars; HOME kept (17 dirs before)
PASS deb restart: citrate killed (pid 1691), started again as pid 2679 in 2s
PASS deb quit [after-restart] in 1s: no process and no listening socket left
PASS deb launch [crash-1] in 4s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
     deb reap: after SIGKILL of the app, 3 sidecar(s) still running: ipfs:3854 citrate:3863 mem-mcp:4737
PASS deb launch [crash-2] in 4s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
PASS deb reap: the relaunch stopped all 3 orphaned sidecar(s) of the killed run
PASS deb quit [crash-2] in 0s: no process and no listening socket left
== appimage
     appimage session: private bus pid 6755 (subreaper), keyring pid 6759 (default collection unlocked)
PASS appimage launch [from] in 8s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
PASS appimage quit [from] in 1s: no process and no listening socket left
PASS appimage launch [update] in 8s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
PASS appimage update: relaunch over the old install runs the new package's sidecars; HOME kept (17 dirs before)
PASS appimage restart: citrate killed (pid 8578), started again as pid 9603 in 2s
PASS appimage quit [after-restart] in 1s: no process and no listening socket left
PASS appimage launch [crash-1] in 5s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
     appimage reap: after SIGKILL of the app, 3 sidecar(s) still running: ipfs:10838 citrate:10847 mem-mcp:11726
PASS appimage launch [crash-2] in 5s: sidecars citrate ipfs mem-mcp  window: 0x200002 Citrate Core
PASS appimage reap: the relaunch stopped all 3 orphaned sidecar(s) of the killed run
PASS appimage quit [crash-2] in 1s: no process and no listening socket left
final: no process left in the namespace
RESULT: PASS
```

The indented `reap` lines record the setup: after SIGKILL of the app, 3 sidecars
(`citrate`, `ipfs`, `mem-mcp`) were still running, for both kinds; the relaunch stopped all of
them. Quit sent `WM_DELETE_WINDOW` to the app's windows (`Citrate Core`, `citrate-core`,
`WebKitWebProcess`). The session services the private bus started on the app's request
(`xdg-desktop-portal`, `xdg-desktop-portal-gtk`, `xdg-permission-store`, `gvfsd`, at-spi) were
not counted as app processes and were stopped before the final check, which found nothing left.

S14.6 criteria (plan: `S14.6_LINUX_PLAN.md`): A1 both kinds, A2 launch, A3 quit, A4 manual
update over 0.4.2, A5 relaunch, A6 no Citrate-started process or listening socket left: met on
aarch64. B1 restart, B2 orphan reaping: met. Not covered: x86_64 (CI leg), the AppImage FUSE
mount path, anything behind sign-in, peering/sync.

Harness self-test on the same machine: good mode passes; leak-on-quit, no-restart and no-sweep
each fail exactly their scenario (re-run at b0c6356a: PASS). Revert check: with the checks stubbed (commit 66a75116) the
three broken modes pass and the self-test fails.

Earlier runs the same day (harness at 333b2647, same packages) also passed on the DGX. The first
CI run of that revision passed the self-test and the aarch64 leg; on the x86_64 leg the relaunch
after the crash waited on a keyring unlock prompt (the private keyring had no login collection),
which is a harness environment gap, fixed in b0c6356a by creating and unlocking a throwaway login
keyring up front. The CI result for b0c6356a is recorded on the PR.
