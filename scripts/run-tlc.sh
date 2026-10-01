#!/usr/bin/env bash
# Run TLC on a spec in src-tauri/formal.
# Usage: scripts/run-tlc.sh <Spec> [cfg-name | all] [extra TLC args...]
#   scripts/run-tlc.sh AgentLoop                 # AgentLoop.tla with AgentLoop.cfg
#   scripts/run-tlc.sh WebSigningBudget all      # every WebSigningBudget*.cfg in turn
#   scripts/run-tlc.sh WebSigningBudget WebSigningBudget_X402
# Finds a JDK (JAVA, then Homebrew openjdk, then PATH) and tla2tools.jar (TLA_JAR, then ~/.tla).
# TLC's state directory goes under $TMPDIR, never into the source tree.
set -euo pipefail
spec="${1:?usage: run-tlc.sh <Spec> [cfg|all] [extra TLC args...]}"
cfg="${2:-$spec}"
shift $(( $# >= 2 ? 2 : 1 ))
here="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/formal"
java="${JAVA:-}"
if [ -z "$java" ]; then
  for c in /opt/homebrew/opt/openjdk/bin/java "$(command -v java || true)"; do
    if [ -n "$c" ] && "$c" -version >/dev/null 2>&1; then java="$c"; break; fi
  done
fi
[ -n "$java" ] || { echo "no working Java runtime (brew install openjdk)" >&2; exit 2; }
jar="${TLA_JAR:-$HOME/.tla/tla2tools.jar}"
[ -f "$jar" ] || { echo "tla2tools.jar not found (set TLA_JAR)" >&2; exit 2; }
cd "$here"
meta="$(mktemp -d "${TMPDIR:-/tmp}/tlc-${spec}.XXXXXX")"
trap 'rm -rf "$meta"' EXIT
run() {
  local c="$1"; shift
  echo "== TLC $spec.tla -config $c.cfg ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
  "$java" -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto -metadir "$meta/$c" \
    "$spec.tla" -config "$c.cfg" "$@"
}
if [ "$cfg" = "all" ]; then
  status=0
  for f in "$spec".cfg "$spec"_*.cfg; do
    [ -f "$f" ] || continue
    run "${f%.cfg}" "$@" || status=$?
  done
  exit "$status"
fi
run "$cfg" "$@"
