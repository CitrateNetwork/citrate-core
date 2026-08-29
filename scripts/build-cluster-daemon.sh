#!/usr/bin/env bash
# build-cluster-daemon.sh — build the bundled `cluster-daemon` sidecar from citrate-cluster.
#
# WHY THIS EXISTS
#
# citrate-core spawns the citrate-cluster daemon (a group's private P2P mesh — Noise + gossipsub,
# admission gated by cluster-core) as a supervised sidecar and speaks its loopback UDS JSON IPC
# (src-tauri/src/cluster.rs). The binary is platform-specific + .gitignore'd, so a fresh checkout has
# no cluster daemon and the Cluster surface surfaces an honest "binary not bundled" until this runs.
# It is declared as a Tauri `externalBin` in the packaging overlays as `binaries/cluster-daemon`.
#
# Usage:
#   scripts/build-cluster-daemon.sh                            # host triple, ../citrate-cluster
#   scripts/build-cluster-daemon.sh --cluster ~/src/citrate-cluster
#   scripts/build-cluster-daemon.sh --target aarch64-apple-darwin
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLUSTER_DIR="${CLUSTER_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-cluster}"
TARGET=""

while [ $# -gt 0 ]; do
  case "$1" in
    --cluster) CLUSTER_DIR="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

[ -d "$CLUSTER_DIR" ] || die "citrate-cluster not found at $CLUSTER_DIR (use --cluster <path>)"
command -v cargo >/dev/null || die "cargo not found"
[ -z "$TARGET" ] && TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"

OUT="$REPO_ROOT/src-tauri/binaries/cluster-daemon-${TARGET}"
REV="$(git -C "$CLUSTER_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"

echo "citrate-cluster : $CLUSTER_DIR (@ $REV)"
echo "target          : $TARGET"
echo "install to      : $OUT"
echo

echo "▶ building cluster-daemon (release)"
( cd "$CLUSTER_DIR" && cargo build --release --locked -p cluster-daemon --target "$TARGET" 2>/dev/null \
  || cargo build --release -p cluster-daemon --target "$TARGET" )

SRC="$CLUSTER_DIR/target/${TARGET}/release/cluster-daemon"
[ -f "$SRC" ] || die "build produced no binary at $SRC"

mkdir -p "$REPO_ROOT/src-tauri/binaries"
cp "$SRC" "$OUT"
chmod +x "$OUT"
printf '%s  cluster-daemon  citrate-cluster@%s  %s\n' "$TARGET" "$REV" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  > "$REPO_ROOT/src-tauri/binaries/cluster-daemon-${TARGET}.provenance"

echo "✓ installed $OUT"
echo "  provenance: citrate-cluster@$REV"
echo
echo "Dev/tests do not need this — set CITRATE_CLUSTER_BIN to override, or run the packaged build:"
echo "  npx tauri build --config src-tauri/tauri.local-run.conf.json"
