#!/usr/bin/env bash
# parity-live.sh: HUP-S1.9 live parity run against a PACKAGED build.
#
# Runs every parity-v1 scenario through core's sidecar provider (src/agent/sidecarProvider.ts), the
# Hermes sidecar binary shipped inside the .app (Contents/MacOS/hermes), and a scripted model over
# real HTTP; see src/agent/parity/live/. Writes a JSON report (per-scenario result, the sidecar's
# sha256, the session config and the live step cap) and exits non-zero on any mismatch.
#
# Usage:
#   scripts/parity-live.sh                                   # newest .app under the cargo target dir
#   scripts/parity-live.sh --app "/path/Citrate Core.app"    # a specific packaged build
#   scripts/parity-live.sh --bin /path/to/hermes             # a bare sidecar binary (not a packaged proof)
#   scripts/parity-live.sh --report /tmp/parity-live.json
#
# Build the app first (no signing needed for this local proof):
#   npx tauri build --config src-tauri/tauri.bundle-lite.conf.json --no-sign --bundles app
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP=""
BIN=""
REPORT="${TMPDIR:-/tmp}/parity-live-report.json"

while [ $# -gt 0 ]; do
  case "$1" in
    --app) APP="$2"; shift 2 ;;
    --bin) BIN="$2"; shift 2 ;;
    --report) REPORT="$2"; shift 2 ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

CAPSULES=""
if [ -z "$BIN" ]; then
  if [ -z "$APP" ]; then
    TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
    APP="$(ls -dt "$TARGET_DIR"/release/bundle/macos/*.app 2>/dev/null | head -n 1 || true)"
    [ -n "$APP" ] || die "no packaged .app under $TARGET_DIR/release/bundle/macos (build one, or pass --app)"
  fi
  [ -d "$APP" ] || die "not an app bundle: $APP"
  BIN="$APP/Contents/MacOS/hermes"
  CAPSULES="$APP/Contents/Resources/capsules"
fi
[ -x "$BIN" ] || die "no executable sidecar at $BIN"

echo "sidecar : $BIN"
echo "sha256  : $(shasum -a 256 "$BIN" | awk '{print $1}')"
[ -n "$APP" ] && echo "app     : $APP ($(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist" 2>/dev/null || echo '?'))"
echo "report  : $REPORT"
echo

cd "$REPO_ROOT"
CITRATE_PARITY_LIVE_SIDECAR="$BIN" \
CITRATE_PARITY_LIVE_CAPSULES="$CAPSULES" \
CITRATE_PARITY_LIVE_REPORT="$REPORT" \
  npx vitest run src/agent/parity/live/parity.live.test.ts
