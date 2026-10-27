#!/usr/bin/env bash
# created: 2026-10-22 | branch: ci/k1l-linux-native-acceptance | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active (K1-L). SELF-TEST FIXTURE ONLY: never shipped, never part of the app.
#
# A stand-in "app" for `linux-native-acceptance.sh --self-test`. The self-test packages it as
# usr/bin/citrate-core in a throwaway .deb, next to sidecars that are copies of /bin/sleep named
# like the real ones, and runs the acceptance scenarios against it inside the namespace. Its job
# is to prove that the harness tells a good supervisor from a bad one, so its behaviour is chosen
# by usr/lib/citrate-core-fake/mode inside the package:
#   good          sweep this install's orphaned sidecars at start, restart a sidecar that dies,
#                 stop every sidecar on SIGTERM
#   leak-on-quit  like good, but leaves mem-mcp running on SIGTERM
#   no-restart    like good, but does not restart a sidecar that dies
#   no-sweep      like good, but does not stop an earlier run's orphaned sidecars at start
#   dup-no-restart  starts a second citrate sidecar and restarts none: killing one citrate
#                 leaves the other running, which must not count as a restart
set -u
BIN="$(cd "$(dirname "$0")" && pwd)"
MODE="$(cat "$BIN/../lib/citrate-core-fake/mode" 2>/dev/null || echo good)"
SIDECARS=(citrate ipfs mem-mcp)
declare -A PIDS=()

exe_of() { readlink "/proc/$1/exe" 2>/dev/null; }

sweep() {
  local d pid exe ppid stat s
  for d in /proc/[0-9]*; do
    pid="${d#/proc/}"
    [ "$pid" = "$$" ] && continue
    exe="$(exe_of "$pid")" || continue
    for s in "${SIDECARS[@]}"; do
      [ "$exe" = "$BIN/$s" ] || continue
      { IFS= read -r stat <"$d/stat"; } 2>/dev/null || continue
      stat="${stat##*) }"; ppid="$(echo "$stat" | cut -d' ' -f2)"
      [ "$ppid" = "$$" ] && continue
      kill -TERM "$pid" 2>/dev/null
    done
  done
}

spawn() { "$BIN/$1" 100000 </dev/null >/dev/null 2>&1 & PIDS[$1]=$!; }

stop_all() {
  local s
  for s in "${SIDECARS[@]}"; do
    [ "$MODE" = leak-on-quit ] && [ "$s" = mem-mcp ] && continue
    kill -TERM "${PIDS[$s]}" 2>/dev/null
  done
  for s in "${SIDECARS[@]}"; do
    [ "$MODE" = leak-on-quit ] && [ "$s" = mem-mcp ] && continue
    wait "${PIDS[$s]}" 2>/dev/null
  done
  exit 0
}

[ "$MODE" = no-sweep ] || sweep
trap stop_all TERM INT
for s in "${SIDECARS[@]}"; do spawn "$s"; done
if [ "$MODE" = dup-no-restart ]; then
  "$BIN/citrate" 100000 </dev/null >/dev/null 2>&1 &
  EXTRA=$!
  trap 'kill -TERM "$EXTRA" 2>/dev/null; wait "$EXTRA" 2>/dev/null; stop_all' TERM INT
fi
while :; do
  wait -n 2>/dev/null
  for s in "${SIDECARS[@]}"; do
    if ! kill -0 "${PIDS[$s]}" 2>/dev/null; then
      case "$MODE" in no-restart|dup-no-restart) ;; *) spawn "$s" ;; esac
    fi
  done
  sleep 0.2
done
