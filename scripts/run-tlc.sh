#!/usr/bin/env bash
# Run TLC on a spec in src-tauri/formal. Usage: scripts/run-tlc.sh AgentLoop [cfg-name]
# Finds a JDK (JAVA, then Homebrew openjdk, then PATH) and tla2tools.jar (TLA_JAR, then ~/.tla).
set -euo pipefail
spec="${1:?usage: run-tlc.sh <Spec> [cfg]}"
cfg="${2:-$spec}"
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
exec "$java" -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto "$spec.tla" -config "$cfg.cfg"
