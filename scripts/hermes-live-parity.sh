#!/usr/bin/env bash
# hermes-live-parity.sh — HUP-S1.9: run the parity-v1 suite LIVE against the Hermes sidecar inside a
# built Citrate Core app (src/agent/parity/live.test.ts).
#
# The chat view the app uses (sidecarProvider.ts) drives the packaged sidecar binary over its HTTP
# control API with the session body core sends; a scripted model server stands in for the model.
#
# Usage:
#   scripts/hermes-live-parity.sh "<path>/Citrate Core.app" [results.json]
#   scripts/hermes-live-parity.sh --bin <path-to-hermes-binary> [results.json]
#
# Build the app first, for example (no signing needed for this check):
#   scripts/build-hermes.sh && npx tauri build --config src-tauri/tauri.bundle-lite.conf.json \
#     --config '{"bundle":{"targets":["app"],"createUpdaterArtifacts":false,"macOS":{"signingIdentity":null}}}'
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
die() { echo "ERROR: $*" >&2; exit 1; }

[ $# -ge 1 ] || die "usage: $0 <Citrate Core.app> [results.json]  |  $0 --bin <hermes> [results.json]"
if [ "$1" = "--bin" ]; then
  [ $# -ge 2 ] || die "--bin needs a path"
  BIN="$2"; shift 2
else
  APP="$1"; shift
  [ -d "$APP" ] || die "not an app bundle: $APP"
  # Tauri strips the target triple from an externalBin and places it next to the main executable.
  BIN="$APP/Contents/MacOS/hermes"
fi
[ -x "$BIN" ] || die "no executable sidecar at $BIN"
OUT="${1:-}"

echo "sidecar binary : $BIN"
echo "sha256         : $(shasum -a 256 "$BIN" | awk '{print $1}')"
[ -n "$OUT" ] && echo "results        : $OUT"
echo

cd "$REPO_ROOT"
CITRATE_HERMES_LIVE_BIN="$BIN" CITRATE_PARITY_LIVE_OUT="$OUT" npx vitest run src/agent/parity/live.test.ts
