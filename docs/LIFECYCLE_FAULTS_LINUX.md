---
created: 2026-10-19
branch: ci/k1f-linux-fault-suite
author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
status: active
wp: K1-F
---

# Running the supervisor fault suite safely on Linux (K1-F)

The services supervisor (`kit/src/supervisor.rs`) and its fault-injection suite
(`kit/tests/lifecycle_faults.rs`, harness in `kit/tests/lifecycle/`) start child processes and
then send them SIGTERM and SIGKILL. That is the point of the suite: kill paths, quarantine, the
deadline publisher. On a Linux desktop a wrong pid can reach a process in your login session.
On the DGX, losing a session process kills GNOME and every user unit with it.

So on Linux the suite always runs inside a fresh user + PID namespace:

```sh
CARGO_TARGET_DIR="$PWD/target-k1f" scripts/ci/lifecycle-faults-linux.sh --log-dir "$PWD/target-k1f/lcf-logs"
```

The script:

1. builds `citrate-core-kit`'s lib tests and, when the file exists, the `lifecycle_faults` test
   target, outside the namespace (the git dependency needs the network);
2. re-runs itself as PID 1 under `unshare --user --map-root-user --pid --fork --mount-proc setsid`
   and refuses to continue unless it is PID 1 and its own process-group leader. If the
   namespace cannot be created it exits 2. It never falls back to running the suite outside one;
3. inside the namespace, runs `cargo test --offline -p citrate-core-kit --lib supervisor` and
   `--test lifecycle_faults`, each under `timeout` (`LCF_TIMEOUT_SECS`, default 600);
4. checks for leftover processes **before** PID 1 exits. The namespace belongs to this run, so
   after the suites return any live process other than PID 1 is a leak, marked (`citrate-lcf-*`)
   or not (the supervisor lib tests spawn plain `/bin/sh` and `/bin/sleep`, the fault suite also
   `tail`). The scan reads `/proc` with shell builtins only, so it never sees itself; zombies are
   not counted. Before the suites run, a negative control leaks an unmarked `sleep`, requires the
   scan to report it, then kills it, so a scan that can't see leaks fails the lane. When the
   namespace's init exits the kernel kills everything left in it, so a check run after the
   namespace closes would hide a leak;
5. tolerates, **temporarily**, the four known leaks listed under "Pre-#352 allowlist" below;
6. writes `summary.txt`, the two test logs, `build.log` and `ps-at-exit.txt` into the log
   directory.

Inside the namespace, `ps` and `pgrep` see only the suite's processes (the remounted `/proc`),
and `kill(pid, …)` and `kill(-1, …)` can only reach processes in the namespace.

`setsid` matters. Without it, the namespace's init stays in the caller's process group. That
group is invisible inside the namespace (`ps -o pgid= -p 1` prints `0`), but `kill(0, sig)`
still signals it, and the user namespace does not stop that: the mapped root is your own uid
outside. With `setsid`, PID 1 leads its own session and group (`pgid` = `sid` = 1), so
`kill(0, …)` stays inside too. The `pgid`/`sid` values were checked on the DGX on 2026-10-19;
the `kill(0)` reach follows from `kill(2)` signalling the caller's group as the kernel tracks it,
not as the namespace numbers it.

## Pre-#352 allowlist (temporary)

Three supervisor lib-test fixtures in `kit/src/supervisor_tests.rs` run
`sh -c "...; sleep 60"`. The shell forks the sleep, so stopping the supervised shell leaves the
`sleep 60` behind: four per run, because `ring_survives_restart` spawns two. citrate-core#352
changes the fixtures to `exec sleep 60`. On a commit without that change the strict check fails
even when every test passes.

Until #352 is on `main`, the script tolerates exactly those leaks:

- only while `kit/src/supervisor_tests.rs` still has a non-exec `; sleep 60"` fixture line, so
  on any commit that carries the #352 fixtures the allowlist is off and the check is strict;
- only processes whose argv is exactly `sleep 60`;
- at most four of them. A fifth, or any other leftover, still fails the lane.

Tolerated leaks are listed in `summary.txt` with a `::warning::` and then killed. Once #352 is
on `main`, delete the block marked `TEMPORARY pre-#352 allowlist` in
`scripts/ci/lifecycle-faults-linux.sh`, the matching header note, and this section.

## Doing it by hand

If you run `cargo test` directly, wrap it yourself:

```sh
unshare --user --map-root-user --pid --fork --mount-proc setsid \
  cargo test --locked --offline -p citrate-core-kit --test lifecycle_faults -- --nocapture
```

Rules on the DGX:

- Never run the supervisor, `lifecycle_faults` or any kill-path test outside `unshare`.
- Never launch the Core app itself to test the supervisor.
- Use a `CARGO_TARGET_DIR` inside your own worktree, not a shared `target/`.

## Ubuntu 24.04 and AppArmor

Ubuntu 24.04 sets `kernel.apparmor_restrict_unprivileged_userns=1`. On the DGX the
`unshare` command above still works. On the GitHub `ubuntu-24.04` runner the workflow tries the
probe first and, only if it fails, runs
`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` on the throwaway VM. Do not
change that sysctl on a desktop; if `unshare` fails there, stop and ask.

## CI

The build fetches `citrate-wallet-core` from `citrate-chain` over git. As in `ci.yml`, the job
uses the read-only secret `CITRATE_CHAIN_READ_TOKEN` for that fetch when it is available, and
fetches anonymously when it is not (pull requests from forks get no secrets; `citrate-chain` is
public, so the anonymous fetch works).

`.github/workflows/lifecycle-faults-linux.yml` runs the same script on `ubuntu-24.04` (x86_64)
for pushes to `main` / `release/**` and for pull requests that touch `kit/**`, `Cargo.toml`,
`Cargo.lock`, the script or the workflow. The job has a 30-minute timeout and uploads the log
directory as the `lifecycle-faults-linux-logs` artifact.

Until #342/#351 land on `main`, the fault-suite file is not on `main`. The lane then runs the
supervisor lib tests only and says so in a `::notice::` and in `summary.txt`. It does not
count that as a pass of the fault suite. The supervisor lib test floor is
`LCF_MIN_SUPERVISOR=24`, the count on `main` at b521d98 (Rule 2).

The MAC lane's `lifecycle-faults.yml` (#342) also has an `ubuntu-latest` row. That row runs the
suite without a namespace. This lane is the namespaced Linux run.
