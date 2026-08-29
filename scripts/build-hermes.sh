#!/usr/bin/env bash
# build-hermes.sh — build the bundled `hermes` sidecar from citrate-agent-runtime.
#
# WHY THIS EXISTS
#
# citrate-core spawns the Hermes operator sidecar (the `agent-sidecar` crate → bin
# `citrate-agent-sidecar`) as a supervised child and drives its loopback control API
# (src-tauri/src/hermes.rs: hands it CITRATE_HERMES_ADDR + a 0600 CITRATE_HERMES_TOKEN_FILE). The
# binary is large + platform-specific + .gitignore'd, so it is NOT in this repo — a fresh checkout
# has no hermes sidecar and `hermes_start` returns BinaryNotFound honestly until this runs. It is
# declared as a Tauri `externalBin` in the packaging overlays (tauri.local-run.conf.json /
# tauri.bundle-node.conf.json) as `binaries/hermes`, so the produced file must be `hermes-<triple>`.
#
# Mirrors build-comms-daemon.sh / build-cluster-daemon.sh. This is the last thing between "Hermes
# code is merged" and "Hermes runs on the Mac" (gD-hermes packaging).
#
# Usage:
#   scripts/build-hermes.sh                                   # host triple, ../citrate-agent-runtime
#   scripts/build-hermes.sh --agent-runtime ~/src/citrate-agent-runtime
#   scripts/build-hermes.sh --target aarch64-apple-darwin
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RT_DIR="${RT_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-agent-runtime}"
TARGET=""

while [ $# -gt 0 ]; do
  case "$1" in
    --agent-runtime) RT_DIR="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

[ -d "$RT_DIR" ] || die "citrate-agent-runtime not found at $RT_DIR (use --agent-runtime <path>)"
command -v cargo >/dev/null || die "cargo not found"
[ -z "$TARGET" ] && TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"

# The Tauri externalBin is `binaries/hermes`; Tauri appends `-<triple>` at bundle time.
OUT="$REPO_ROOT/src-tauri/binaries/hermes-${TARGET}"
REV="$(git -C "$RT_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"

echo "citrate-agent-runtime : $RT_DIR (@ $REV)"
echo "target                : $TARGET"
echo "install to            : $OUT"
echo

echo "▶ building agent-sidecar → citrate-agent-sidecar (release)"
( cd "$RT_DIR" && cargo build --release --locked -p agent-sidecar --target "$TARGET" 2>/dev/null \
  || cargo build --release -p agent-sidecar --target "$TARGET" )

SRC="$RT_DIR/target/${TARGET}/release/citrate-agent-sidecar"
[ -f "$SRC" ] || die "build produced no binary at $SRC"

mkdir -p "$REPO_ROOT/src-tauri/binaries"
cp "$SRC" "$OUT"
chmod +x "$OUT"

# Provenance: record the source rev next to the (gitignored) binary so a mystery binary is traceable.
printf '%s  hermes(citrate-agent-sidecar)  citrate-agent-runtime@%s  %s\n' "$TARGET" "$REV" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  > "$REPO_ROOT/src-tauri/binaries/hermes-${TARGET}.provenance"

echo "✓ installed $OUT"
echo "  provenance: citrate-agent-runtime@$REV"
echo
echo "Dev/tests do not need this — set CITRATE_HERMES_BIN to override, or run the packaged build:"
echo "  npx tauri build --config src-tauri/tauri.local-run.conf.json"
