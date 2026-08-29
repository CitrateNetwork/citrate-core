#!/usr/bin/env bash
# build-comms-daemon.sh — build the bundled `comms-member-daemon` sidecar from citrate-comms.
#
# WHY THIS EXISTS
#
# citrate-core spawns the citrate-comms member-daemon (the wallet-owned OpenMLS member + an
# in-process server-blind relay) as a supervised sidecar and speaks its loopback UDS JSON IPC
# (src-tauri/src/comms.rs). The daemon binary is ~large + platform-specific + .gitignore'd, so it is
# NOT in this repo — a fresh checkout has no comms daemon and Groups surface an honest
# "binary not bundled" until this runs. It is declared as a Tauri `externalBin` in the packaging
# overlays (tauri.bundle-node.conf.json / tauri.local-run.conf.json) as `binaries/comms-member-daemon`.
#
# It REPLACES the retired `comms-relay` sidecar (the S3.1 bare relay the member-daemon subsumes).
#
# Usage:
#   scripts/build-comms-daemon.sh                          # host triple, ../citrate-comms
#   scripts/build-comms-daemon.sh --comms ~/src/citrate-comms
#   scripts/build-comms-daemon.sh --target aarch64-apple-darwin
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMS_DIR="${COMMS_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-comms}"
TARGET=""

while [ $# -gt 0 ]; do
  case "$1" in
    --comms) COMMS_DIR="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

[ -d "$COMMS_DIR" ] || die "citrate-comms not found at $COMMS_DIR (use --comms <path>)"
command -v cargo >/dev/null || die "cargo not found"
[ -z "$TARGET" ] && TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"

OUT="$REPO_ROOT/src-tauri/binaries/comms-member-daemon-${TARGET}"
REV="$(git -C "$COMMS_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"

echo "citrate-comms : $COMMS_DIR (@ $REV)"
echo "target        : $TARGET"
echo "install to    : $OUT"
echo

echo "▶ building comms-member-daemon (release)"
( cd "$COMMS_DIR" && cargo build --release --locked -p comms-member-daemon --target "$TARGET" 2>/dev/null \
  || cargo build --release -p comms-member-daemon --target "$TARGET" )

SRC="$COMMS_DIR/target/${TARGET}/release/comms-member-daemon"
[ -f "$SRC" ] || die "build produced no binary at $SRC"

mkdir -p "$REPO_ROOT/src-tauri/binaries"
cp "$SRC" "$OUT"
chmod +x "$OUT"

# Provenance: record the source rev next to the (gitignored) binary so a mystery binary is traceable.
printf '%s  comms-member-daemon  citrate-comms@%s  %s\n' "$TARGET" "$REV" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  > "$REPO_ROOT/src-tauri/binaries/comms-member-daemon-${TARGET}.provenance"

echo "✓ installed $OUT"
echo "  provenance: citrate-comms@$REV"
echo
echo "Dev/tests do not need this — set CITRATE_MEMBER_BIN to override, or run the packaged build:"
echo "  npx tauri build --config src-tauri/tauri.local-run.conf.json"
