#!/usr/bin/env bash
# lifecycle-faults-linux.sh — K1-F: run the services-supervisor tests and the lifecycle
# fault-injection suite on Linux, inside a fresh PID namespace, and fail if any process other
# than the namespace's PID 1 is left behind (marked or not). Used by
# .github/workflows/lifecycle-faults-linux.yml and by hand on a Linux desktop
# (see docs/LIFECYCLE_FAULTS_LINUX.md).
#
# Why a PID namespace: the suite sends SIGTERM/SIGKILL to the processes it spawns. A wrong pid
# outside a namespace can reach a process in the desktop session (on the DGX this kills GNOME).
# Inside `unshare --pid --fork --mount-proc` the suite can only see and signal its own
# processes, and when the namespace's init exits the kernel kills whatever is left.
#
# Usage: scripts/ci/lifecycle-faults-linux.sh [--log-dir DIR]
# Env:   CARGO_TARGET_DIR      honoured (set it to a directory inside your checkout)
#        RUSTUP_TOOLCHAIN      honoured (CI sets 1.98.1)
#        LCF_TIMEOUT_SECS      per cargo-test run inside the namespace (default 600)
#        LCF_MIN_SUPERVISOR    minimum passing supervisor lib tests (default 24, main b521d98)
#
# Temporary: until citrate-core#352 is on main, the leftover check tolerates the 4 known
# `sleep 60` leaks from the non-exec supervisor fixtures (see "pre-#352 allowlist" below).
#
# Never falls back to running outside a namespace: if `unshare` cannot create one, it exits 2.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SELF="$ROOT/scripts/ci/lifecycle-faults-linux.sh"
LOG_DIR="${LOG_DIR:-$ROOT/target/lifecycle-faults-linux}"
INNER=0
while [ $# -gt 0 ]; do
  case "$1" in
    --log-dir) LOG_DIR="$2"; shift 2 ;;
    --inner) INNER=1; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done
FAULTS_SRC="$ROOT/kit/tests/lifecycle_faults.rs"
TIMEOUT_SECS="${LCF_TIMEOUT_SECS:-600}"
MIN_SUPERVISOR="${LCF_MIN_SUPERVISOR:-24}"

# Pull "N passed; M failed" out of a libtest log. Prints "passed failed" or nothing.
counts() {
  grep -E '^test result: ' "$1" | tail -n 1 |
    sed -E 's/.* ([0-9]+) passed; ([0-9]+) failed.*/\1 \2/'
}

if [ "$INNER" -eq 0 ]; then
  [ "$(uname -s)" = Linux ] || { echo "::error::this lane is Linux-only"; exit 2; }
  command -v unshare >/dev/null || { echo "::error::unshare (util-linux) not found"; exit 2; }
  mkdir -p "$LOG_DIR"
  cd "$ROOT" || exit 2

  # Build outside the namespace (needs the network for the git dep); run offline inside it.
  build_args=(--lib)
  [ -f "$FAULTS_SRC" ] && build_args+=(--test lifecycle_faults)
  echo "▶ build: cargo test --locked -p citrate-core-kit ${build_args[*]} --no-run"
  if ! cargo test --locked -p citrate-core-kit "${build_args[@]}" --no-run \
      >"$LOG_DIR/build.log" 2>&1; then
    tail -n 40 "$LOG_DIR/build.log"
    echo "::error::build failed (log: $LOG_DIR/build.log)"
    exit 1
  fi

  if ! unshare --user --map-root-user --pid --fork --mount-proc true 2>"$LOG_DIR/unshare-probe.log"; then
    cat "$LOG_DIR/unshare-probe.log"
    echo "::error::cannot create an unprivileged user+PID namespace; refusing to run the suite"
    echo "  (Ubuntu 24.04: sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0)"
    exit 2
  fi

  # setsid: without it the namespace's init keeps the caller's process group, which is not
  # visible inside the namespace (getpgrp() == 0) but still receives kill(0, sig). With setsid,
  # PID 1 leads its own session and group, so kill(0) and kill(-1) both stay in the namespace.
  echo "▶ run inside: unshare --user --map-root-user --pid --fork --mount-proc setsid"
  unshare --user --map-root-user --pid --fork --mount-proc \
    setsid env LOG_DIR="$LOG_DIR" bash "$SELF" --inner --log-dir "$LOG_DIR"
  rc=$?
  [ -f "$LOG_DIR/summary.txt" ] && cat "$LOG_DIR/summary.txt"
  exit "$rc"
fi

# ---- inside the namespace ----------------------------------------------------------------
# Stray-process detection: scan_strays (scripts/ci/lib/pidns.sh). The namespace is dedicated to
# this run, so once the suites have returned the only process that may exist is PID 1 (this
# shell). Every other live process is a leftover, whatever its name: a marked `citrate-lcf-*`
# leaf from the fault suite, or an unmarked `/bin/sh`, `/bin/sleep` or `tail` from a supervisor
# lib-test fixture or one of their descendants.
# shellcheck source=scripts/ci/lib/pidns.sh
. "$ROOT/scripts/ci/lib/pidns.sh"

cd "$ROOT" || exit 2
if ! pidns_assert_init; then
  echo "::error::--inner must run as PID 1 and group leader of a fresh PID namespace (got pid $$)"
  exit 2
fi
SUMMARY="$LOG_DIR/summary.txt"
FAILED=0
{
  echo "lifecycle-faults-linux summary"
  echo "commit:    $(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "arch:      $(uname -m)  kernel: $(uname -r)"
  echo "rustc:     $(rustc --version 2>/dev/null || echo unknown)"
  echo "namespace: user+pid+mount, own session (pid $$, $(ps -e --no-headers | wc -l) processes visible)"
} >"$SUMMARY"

scan_strays
if [ "${#STRAYS[@]}" -ne 0 ]; then
  printf '%s\n' "${STRAYS[@]}"
  echo "::error::the namespace is not empty before the run" | tee -a "$SUMMARY"
  exit 1
fi

# Negative control: an unmarked, orphaned `sleep` (reparented to PID 1, like a leaked fixture
# descendant) must be reported by the scan. If it is not, the leftover check below cannot be
# trusted and the lane fails. The control process is killed again before the suites run.
# shellcheck disable=SC2217 # detach all three fds so the control holds nothing of ours open
( exec sleep 300 </dev/null >/dev/null 2>&1 & echo "$!" >"$LOG_DIR/negative-control.pid" )
nc_pid="$(<"$LOG_DIR/negative-control.pid")"
scan_strays
nc_seen=0
for l in "${STRAYS[@]}"; do [ "${l%% *}" = "$nc_pid" ] && nc_seen=1; done
kill -KILL "$nc_pid" 2>/dev/null
for _ in $(seq 1 100); do
  scan_strays
  [ "${#STRAYS[@]}" -eq 0 ] && break
  sleep 0.05
done
if [ "$nc_seen" -ne 1 ] || [ "${#STRAYS[@]}" -ne 0 ]; then
  echo "leftover check negative control: FAIL (leaked sleep pid $nc_pid seen=$nc_seen, still present: ${#STRAYS[@]})" | tee -a "$SUMMARY"
  exit 1
fi
echo "leftover check negative control: ok (an unmarked leaked sleep was detected)" | tee -a "$SUMMARY"

run_suite() { # name, log, cargo args...
  local name="$1" log="$2"; shift 2
  echo "▶ $name: cargo test --locked --offline -p citrate-core-kit $*"
  timeout --kill-after=10 "$TIMEOUT_SECS" \
    cargo test --locked --offline -p citrate-core-kit "$@" >"$log" 2>&1
  local st=$?
  local c; c="$(counts "$log")"
  local p="${c%% *}" f="${c##* }"
  [ -n "$c" ] || { p=0; f="?"; }
  if [ "$st" -eq 124 ] || [ "$st" -eq 137 ]; then
    echo "$name: TIMEOUT after ${TIMEOUT_SECS}s (exit $st)" | tee -a "$SUMMARY"
    FAILED=1
  elif [ "$st" -ne 0 ]; then
    echo "$name: FAIL (exit $st) passed=$p failed=$f" | tee -a "$SUMMARY"
    grep -E '^(---- |test .* FAILED|failures:)' "$log" | head -n 40 | tee -a "$SUMMARY"
    FAILED=1
  else
    echo "$name: ok passed=$p failed=$f" | tee -a "$SUMMARY"
  fi
  LAST_PASSED="$p"
}

run_suite "supervisor (kit lib)" "$LOG_DIR/supervisor-lib.log" --lib supervisor -- --nocapture
if [ "${LAST_PASSED:-0}" -lt "$MIN_SUPERVISOR" ]; then
  echo "supervisor (kit lib): ${LAST_PASSED:-0} passed < floor $MIN_SUPERVISOR (Rule 2)" | tee -a "$SUMMARY"
  FAILED=1
fi

if [ -f "$FAULTS_SRC" ]; then
  run_suite "lifecycle_faults" "$LOG_DIR/lifecycle-faults.log" --test lifecycle_faults -- --nocapture
  if [ "${LAST_PASSED:-0}" -eq 0 ]; then
    echo "lifecycle_faults: no test passed" | tee -a "$SUMMARY"
    FAILED=1
  fi
else
  echo "::notice::kit/tests/lifecycle_faults.rs is not on this commit; ran the supervisor lib tests only"
  echo "lifecycle_faults: not present at this commit (lands with #342/#351)" | tee -a "$SUMMARY"
fi

# Leftover check: must run here, before PID 1 exits, because the kernel kills every process in
# the namespace when its init exits and that would hide a leak.
# Every process other than PID 1 counts, marked or not (see scan_strays). A short grace lets a
# process that is already exiting finish; anything still there after it is a leak.
for _ in $(seq 1 20); do
  scan_strays
  [ "${#STRAYS[@]}" -eq 0 ] && break
  sleep 0.1
done
ps -e -o pid,ppid,stat,args >"$LOG_DIR/ps-at-exit.txt" 2>&1

# ---- TEMPORARY pre-#352 allowlist (remove this block once citrate-core#352 is on main) -------
# Three supervisor lib-test fixtures in kit/src/supervisor_tests.rs run `sh -c "...; sleep 60"`.
# The shell forks the sleep, and stopping the supervised shell leaves the sleep running: 4 per
# run (ring_survives_restart spawns two). #352 changes them to `exec sleep 60`. Until that is on
# the commit under test, exactly those leaks are tolerated, and nothing else:
#   - only while the fixture source still has a non-exec `; sleep 60"` line (so the allowlist
#     switches itself off on any commit that carries the #352 fixtures, and the check is strict);
#   - only processes whose argv is exactly `sleep 60`;
#   - at most LCF_PRE352_MAX (4) of them. A fifth, or any other leftover, still fails the lane.
# Tolerated leaks are listed in summary.txt and then killed.
PRE352_SRC="$ROOT/kit/src/supervisor_tests.rs"
PRE352_MAX=4
pre352_fixtures=0
[ -f "$PRE352_SRC" ] && pre352_fixtures="$(grep -cE '; sleep 60"' "$PRE352_SRC")"
if [ "${#STRAYS[@]}" -ne 0 ] && [ "$pre352_fixtures" -gt 0 ]; then
  kept=() tolerated=()
  for l in "${STRAYS[@]}"; do
    rest="${l#* }"; rest="${rest#* }" # drop "pid state "
    if [ "$rest" = "sleep 60" ] && [ "${#tolerated[@]}" -lt "$PRE352_MAX" ]; then
      tolerated+=("$l")
    else
      kept+=("$l")
    fi
  done
  if [ "${#tolerated[@]}" -ne 0 ]; then
    printf 'tolerated (pre-#352 fixture): %s\n' "${tolerated[@]}" | tee -a "$SUMMARY"
    echo "::warning::tolerated ${#tolerated[@]} leaked 'sleep 60' from the $pre352_fixtures non-exec supervisor fixture(s); fixed by citrate-core#352" | tee -a "$SUMMARY"
    for l in "${tolerated[@]}"; do kill -KILL "${l%% *}" 2>/dev/null; done
  fi
  STRAYS=("${kept[@]}")
fi
# ---- end TEMPORARY pre-#352 allowlist ---------------------------------------------------------
if [ "${#STRAYS[@]}" -ne 0 ]; then
  printf '%s\n' "${STRAYS[@]}" | tee -a "$SUMMARY"
  echo "::error::the suites left ${#STRAYS[@]} process(es) behind in the namespace" | tee -a "$SUMMARY"
  FAILED=1
else
  echo "leftover processes in the namespace: none" | tee -a "$SUMMARY"
fi

[ "$FAILED" -eq 0 ] && echo "RESULT: PASS" >>"$SUMMARY" || echo "RESULT: FAIL" >>"$SUMMARY"
exit "$FAILED"
