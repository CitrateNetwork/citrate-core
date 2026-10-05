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
#   scripts/build-hermes.sh --no-strip                        # keep local symbols (debugging)
#   scripts/build-hermes.sh --out-dir <dir>                   # install somewhere other than src-tauri/binaries
#
# HUP-S11.0 (gate g5-size): the installed binary has its local symbols removed with `strip -x`
# (about 29.4 MB to 25.4 MB on aarch64-apple-darwin, 2026-10-04). Exported symbols stay, so the
# binary links and runs the same; panic backtraces lose the names of local functions. Tauri signs
# the externalBin after this, at bundle time. Stripping runs when the target OS is the host OS
# (the host `strip` cannot be trusted with another OS's object format); --no-strip skips it.
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RT_DIR="${RT_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-agent-runtime}"
TARGET=""
STRIP=1
OUT_DIR=""

while [ $# -gt 0 ]; do
  case "$1" in
    --agent-runtime) RT_DIR="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    --no-strip) STRIP=0; shift ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

[ -d "$RT_DIR" ] || die "citrate-agent-runtime not found at $RT_DIR (use --agent-runtime <path>)"
command -v cargo >/dev/null || die "cargo not found"
[ -z "$TARGET" ] && TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"

# The Tauri externalBin is `binaries/hermes`; Tauri appends `-<triple>` at bundle time.
[ -n "$OUT_DIR" ] || OUT_DIR="$REPO_ROOT/src-tauri/binaries"
OUT="$OUT_DIR/hermes-${TARGET}"
REV="$(git -C "$RT_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"

echo "citrate-agent-runtime : $RT_DIR (@ $REV)"
echo "target                : $TARGET"
echo "install to            : $OUT"
echo

echo "▶ building agent-sidecar → citrate-agent-sidecar (release)"
( cd "$RT_DIR" && cargo build --release --locked -p agent-sidecar --target "$TARGET" 2>/dev/null \
  || cargo build --release -p agent-sidecar --target "$TARGET" )

# cargo honours CARGO_TARGET_DIR (a shared target dir); otherwise the runtime's own target/.
SRC="${CARGO_TARGET_DIR:-$RT_DIR/target}/${TARGET}/release/citrate-agent-sidecar"
[ -f "$SRC" ] || die "build produced no binary at $SRC"

mkdir -p "$OUT_DIR"
cp "$SRC" "$OUT"
chmod +x "$OUT"

# HUP-S11.0: strip local symbols from the installed copy (the cargo output is left as built).
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
    BEFORE="$(wc -c < "$OUT" | tr -d ' ')"
    strip -x "$OUT"
    AFTER="$(wc -c < "$OUT" | tr -d ' ')"
    STRIPPED="yes"
    echo "✓ strip -x: $BEFORE -> $AFTER bytes"
  else
    echo "note: target OS ($TARGET_OS) is not the host OS ($HOST_OS); installed unstripped"
  fi
fi

# Provenance: record the source rev next to the (gitignored) binary so a mystery binary is traceable.
printf '%s  hermes(citrate-agent-sidecar)  citrate-agent-runtime@%s  %s  stripped=%s\n' "$TARGET" "$REV" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$STRIPPED" \
  > "$OUT_DIR/hermes-${TARGET}.provenance"

echo "✓ installed $OUT"
echo "  provenance: citrate-agent-runtime@$REV"
echo
echo "Dev/tests do not need this — set CITRATE_HERMES_BIN to override, or run the packaged build:"
echo "  npx tauri build --config src-tauri/tauri.local-run.conf.json"
