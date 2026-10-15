#!/usr/bin/env bash
# build-citrate-templates.sh — build the bundled `citrate-templates` renderer (HGW-W3, core #289).
#
# WHY THIS EXISTS
#
# The Hermes sidecar's `project_create {template}` (citrate-agent-runtime #81) renders a bundled
# contract template by running core's own renderer program:
#
#   citrate-templates render --root <templates> --template <id> --tier <T> --out <new empty dir>
#
# inside the OS sandbox (no network, writes only the new project, reads the templates). Core hands
# the sidecar its path (CITRATE_HERMES_TEMPLATE_RENDERER, src-tauri/src/hermes_projects.rs). The
# program is this repo's `templates/renderer` crate (the same renderer the app links for
# Agent > Contracts), declared as a Tauri `externalBin` (`binaries/citrate-templates`) in the
# macOS and Linux packaging overlays, so the produced file must be `citrate-templates-<triple>`.
# The binary is .gitignore'd like the other sidecars; without it the app passes no renderer and
# `project_create` creates plain folders only (it says templates are not available).
#
# Usage:
#   scripts/build-citrate-templates.sh                        # host triple
#   scripts/build-citrate-templates.sh --target aarch64-apple-darwin
#   scripts/build-citrate-templates.sh --no-strip
#   scripts/build-citrate-templates.sh --out-dir <dir>        # instead of src-tauri/binaries
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET=""
STRIP=1
OUT_DIR=""

while [ $# -gt 0 ]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --no-strip) STRIP=0; shift ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

command -v cargo >/dev/null || die "cargo not found"
[ -z "$TARGET" ] && TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"
[ -n "$OUT_DIR" ] || OUT_DIR="$REPO_ROOT/src-tauri/binaries"
OUT="$OUT_DIR/citrate-templates-${TARGET}"
REV="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"

echo "citrate-core   : $REPO_ROOT (@ $REV)"
echo "target         : $TARGET"
echo "install to     : $OUT"
echo

echo "▶ building templates/renderer → citrate-templates (release)"
( cd "$REPO_ROOT" && cargo build --release --locked -p citrate-templates --bin citrate-templates --target "$TARGET" )

SRC="${CARGO_TARGET_DIR:-$REPO_ROOT/target}/${TARGET}/release/citrate-templates"
[ -f "$SRC" ] || die "build produced no binary at $SRC"

mkdir -p "$OUT_DIR"
cp "$SRC" "$OUT"
chmod +x "$OUT"

STRIPPED="no"
if [ "$STRIP" = 1 ]; then
  HOST_OS="$(uname -s)"
  case "$TARGET" in
    *-apple-darwin) TARGET_OS="Darwin" ;;
    *-linux-*) TARGET_OS="Linux" ;;
    *) TARGET_OS="other" ;;
  esac
  if [ "$TARGET_OS" = "$HOST_OS" ]; then
    command -v strip >/dev/null || die "strip not found (pass --no-strip to install the unstripped binary)"
    strip -x "$OUT"
    STRIPPED="yes"
  else
    echo "note: target OS ($TARGET_OS) is not the host OS ($HOST_OS); installed unstripped"
  fi
fi

printf '%s  citrate-templates  citrate-core@%s  %s  stripped=%s\n' "$TARGET" "$REV" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$STRIPPED" \
  > "$OUT_DIR/citrate-templates-${TARGET}.provenance"

echo "✓ installed $OUT"
echo "  provenance: citrate-core@$REV"
echo
echo "Dev/tests do not need this: without it Hermes creates plain projects (no templates)."
