#!/usr/bin/env bash
# build-sidecar.sh — build the bundled `citrate` node sidecar from citrate-chain.
#
# WHY THIS EXISTS
#
# src-tauri/binaries/citrate-<target-triple> is ~33 MB and .gitignore'd, so it is
# NOT in this repo. `git clone && npm run tauri build` therefore produces an app
# with no node. Worse, the obvious workaround — copying whatever `citrate` binary
# happens to be lying around — can produce an app that FORKS THE CHAIN, silently.
#
# Chain 40204 was re-rolled 2026-08-04 and both consensus activation heights were
# set to 0 (citrate-chain PR #157):
#
#     VALUE_TRANSFER_ACTIVATION_HEIGHT  300_000 -> 0
#     MERGE_DEPTH_ACTIVATION_HEIGHT     100_000 -> 0
#
# A sidecar built from an older citrate-chain still has the old values. It will
# connect, sync, and look healthy — while computing DIFFERENT state roots below
# height 300k and accepting merge blocks the fleet rejects. This script refuses
# to install such a binary (see the lineage gate below).
#
# Usage:
#   scripts/build-sidecar.sh                       # host target, ../citrate-chain
#   scripts/build-sidecar.sh --chain ~/src/citrate-chain
#   scripts/build-sidecar.sh --target aarch64-apple-darwin
#   scripts/build-sidecar.sh --skip-build          # re-verify + install an existing build
#
# On a Mac the target is aarch64-apple-darwin (Apple Silicon) or
# x86_64-apple-darwin (Intel); the default is whatever `rustc -vV` reports.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHAIN_DIR="${CHAIN_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-chain}"
TARGET=""
SKIP_BUILD=0

while [ $# -gt 0 ]; do
  case "$1" in
    --chain) CHAIN_DIR="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

[ -d "$CHAIN_DIR" ] || die "citrate-chain not found at $CHAIN_DIR (use --chain <path>)"
command -v cargo >/dev/null || die "cargo not found"

if [ -z "$TARGET" ]; then
  TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
fi
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"

OUT="$REPO_ROOT/src-tauri/binaries/citrate-${TARGET}"

echo "citrate-chain : $CHAIN_DIR"
echo "target        : $TARGET"
echo "install to    : $OUT"
echo

# ── source-level lineage gate ───────────────────────────────────────────────
# Check the SOURCE we are about to build, so the failure is "your checkout is
# old" (actionable) rather than a fork discovered days later on-chain.
echo "▶ verifying citrate-chain source is post-re-roll"
vt="$(grep -oE 'pub const VALUE_TRANSFER_ACTIVATION_HEIGHT: u64 = [0-9_]+' \
      "$CHAIN_DIR/core/execution/src/executor.rs" 2>/dev/null | grep -oE '[0-9_]+$' | tr -d _)"
md="$(grep -oE 'pub const MERGE_DEPTH_ACTIVATION_HEIGHT: u64 = [0-9_]+' \
      "$CHAIN_DIR/core/consensus/src/ghostdag.rs" 2>/dev/null | grep -oE '[0-9_]+$' | tr -d _)"
[ -n "$vt" ] || die "could not read VALUE_TRANSFER_ACTIVATION_HEIGHT from $CHAIN_DIR"
[ -n "$md" ] || die "could not read MERGE_DEPTH_ACTIVATION_HEIGHT from $CHAIN_DIR"
echo "    VALUE_TRANSFER_ACTIVATION_HEIGHT = $vt"
echo "    MERGE_DEPTH_ACTIVATION_HEIGHT    = $md"
if [ "$vt" != "0" ] || [ "$md" != "0" ]; then
  cat >&2 <<EOF

ERROR: this citrate-chain checkout predates the 2026-08-04 re-roll.

  VALUE_TRANSFER_ACTIVATION_HEIGHT = $vt   (must be 0)
  MERGE_DEPTH_ACTIVATION_HEIGHT    = $md   (must be 0)

A sidecar built from it WILL FORK chain 40204: it computes different state roots
below height 300,000 and accepts merge blocks the fleet rejects. It will look
healthy while doing so.

Fix:  cd "$CHAIN_DIR" && git fetch origin && git checkout main && git pull
EOF
  exit 1
fi
echo "    OK — post-re-roll source"
echo

# ── build ───────────────────────────────────────────────────────────────────
BUILT="$CHAIN_DIR/target/release/citrate"
if [ "$SKIP_BUILD" -eq 0 ]; then
  echo "▶ building citrate (release) — first build takes several minutes"
  # LZMA_API_STATIC: lzma-sys (via xz2 -> zip -> citrate-storage) probes pkg-config
  # FIRST, so on any machine with Homebrew `xz` installed it links
  # /opt/homebrew/opt/xz/lib/liblzma.5.dylib dynamically. That path does not exist
  # on a member's Mac, and under hardened runtime the loader refuses it anyway
  # ("different Team IDs"), so the app dies on launch everywhere except the build
  # host. Setting this makes lzma-sys compile and statically link its vendored
  # liblzma instead. Verified 2026-09-09: the resulting binary has zero non-system
  # dylib deps. See the portability gate below, which enforces it.
  ( cd "$CHAIN_DIR" && LZMA_API_STATIC=1 cargo build --release --bin citrate )
  echo
else
  echo "▶ --skip-build: using the existing $BUILT"
fi
[ -f "$BUILT" ] || die "expected binary not found: $BUILT"

# Cross-compiling to a different triple needs an explicit toolchain; cargo puts
# the artefact under target/<triple>/release. Prefer that when it exists.
HOST="$(rustc -vV | awk '/^host:/{print $2}')"
if [ "$TARGET" != "$HOST" ] && [ -f "$CHAIN_DIR/target/$TARGET/release/citrate" ]; then
  BUILT="$CHAIN_DIR/target/$TARGET/release/citrate"
elif [ "$TARGET" != "$HOST" ]; then
  die "target $TARGET != host $HOST but no $CHAIN_DIR/target/$TARGET/release/citrate.
Build it on a $TARGET machine (binaries do NOT cross), or set up a cross toolchain
and re-run with --skip-build."
fi

# ── portability gate ────────────────────────────────────────────────────────
# A sidecar that links anything outside /System and /usr/lib runs ONLY on the
# build host. This is the liblzma class of bug (see LZMA_API_STATIC above): the
# beta.1 DMG shipped with a Homebrew liblzma reference and crashed on every Mac
# that was not this one. Catch it here, before it reaches a member, rather than
# in a crash report.
if [ "$(uname -s)" = "Darwin" ] && command -v otool >/dev/null; then
  echo "▶ verifying the binary is portable (no non-system dylibs)"
  LEAKS="$(otool -L "$BUILT" | tail -n +2 | grep -vE '^\s+(/System/|/usr/lib/)' || true)"
  if [ -n "$LEAKS" ]; then
    cat >&2 <<EOF

ERROR: this binary links dylibs that do not exist on a member's Mac:

$LEAKS

It will run here and crash everywhere else (under hardened runtime the loader
rejects a Homebrew dylib outright — "different Team IDs").

Fix: rebuild without --skip-build so LZMA_API_STATIC=1 is applied. If the leak
is NOT liblzma, find the crate that probed pkg-config and give it the same
static treatment — do NOT paper over it with install_name_tool.
EOF
    exit 1
  fi
  echo "    OK — system frameworks only"
  echo
fi

# ── install + verify what actually landed ───────────────────────────────────
mkdir -p "$(dirname "$OUT")"
install -m 755 "$BUILT" "$OUT"

echo "▶ installed"
echo "    $(ls -la "$OUT" | awk '{print $5" bytes"}')"
if command -v md5sum >/dev/null; then
  echo "    md5 $(md5sum "$OUT" | cut -d' ' -f1)"
elif command -v md5 >/dev/null; then   # macOS
  echo "    md5 $(md5 -q "$OUT")"
fi
"$OUT" --version 2>/dev/null | head -1 | sed 's/^/    /' || true
echo
echo "Done. Package with:"
echo "  npx tauri build --config src-tauri/tauri.bundle-node.conf.json"
