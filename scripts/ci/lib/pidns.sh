# shellcheck shell=bash
# created: 2026-10-22 | branch: ci/k1l-linux-native-acceptance | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active. Shared helpers for the Linux lanes that run inside a fresh PID namespace:
#   scripts/ci/lifecycle-faults-linux.sh     (K1-F, the supervisor fault suite)
#   scripts/ci/linux-native-acceptance.sh    (K1-L, SCL-S14.6 packaged-app acceptance)
# Source it; it defines functions only and runs nothing.

# pidns_assert_init: succeed only when this shell is PID 1 and the leader of its own process
# group in the current PID namespace (see `setsid` in docs/LIFECYCLE_FAULTS_LINUX.md: without it
# kill(0, sig) would still reach the caller's group outside the namespace).
pidns_assert_init() {
  [ "$$" -eq 1 ] || return 1
  local stat rest
  { IFS= read -r stat <"/proc/$$/stat"; } 2>/dev/null || return 1
  rest="${stat##*) }" # state ppid pgrp session ...
  # shellcheck disable=SC2086 # split the stat fields on purpose
  set -- $rest
  [ "${3:-}" = 1 ] && [ "${4:-}" = 1 ]
}

# scan_strays [exempt-pid ...]: set STRAYS to one "pid state argv" line per live process other
# than PID 1 and the exempt pids. Reads /proc with shell builtins only (no subshell, pipe or
# command), so the scan itself never shows up as a process. Zombies are not counted: they have
# already exited and only wait to be reaped by PID 1.
scan_strays() {
  STRAYS=()
  local d pid stat state argv e skip
  for d in /proc/[0-9]*; do
    pid="${d#/proc/}"
    [ "$pid" = 1 ] && continue
    skip=0
    for e in "$@"; do [ "$pid" = "$e" ] && skip=1; done
    [ "$skip" -eq 1 ] && continue
    stat=""
    { IFS= read -r stat <"$d/stat"; } 2>/dev/null || continue # gone since the glob
    [ -n "$stat" ] || continue
    stat="${stat##*) }" # comm may hold spaces or ')'; the state follows the last ") "
    state="${stat%% *}"
    [ "$state" = Z ] && continue
    argv=()
    { mapfile -d '' -t argv <"$d/cmdline"; } 2>/dev/null
    STRAYS+=("$pid $state ${argv[*]:-[${stat}]}")
  done
}
