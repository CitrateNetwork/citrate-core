#!/usr/bin/env bash
# build-component.sh — build (or fetch) one bundled component at its pin and install it only if
# the bytes match the pinned sha256.
#
# The pins live in release/components.json (see scripts/component-pins.mjs): source repo, exact
# rev, Rust toolchain, cargo package/bin/features/env, and the sha256 a release may ship for each
# target. This script is the only place a bundled component is built or downloaded; the
# per-component scripts (build-sidecar.sh, build-comms-daemon.sh, build-cluster-daemon.sh,
# build-hermes.sh) delegate here, and `node scripts/component-pins.mjs check` fails CI if one of
# them builds or downloads on its own.
#
# Usage:
#   scripts/build-component.sh <id> [--checkout <dir>] [--target <triple>] [--out-dir <dir>] [--print-digest]
#
#   <id>            a component id from release/components.json (citrate-node, node-agent, mem-mcp,
#                   comms-member-daemon, cluster-daemon, hermes-sidecar, kubo)
#   --checkout      the local git checkout to take the pinned commit from (default: the sibling
#                   ../<source.checkout>). Only its object store is used: the build runs in a fresh
#                   tree at the pinned rev, never in the checkout's own working tree.
#   --target        target triple (default: the host's)
#   --out-dir       install directory (default: src-tauri/binaries); the file is <name>-<triple>
#   --print-digest  build, print "<id> <triple> <sha256> <path>" and install nothing. Use it to
#                   propose a new pin (a reviewed PR to release/components.json or
#                   src-tauri/runtime-deps.sha256).
#
# Fails closed: a component with no pin for the target, a rev the checkout does not have, a
# modified build tree, a non-portable macOS binary, a download whose archive digest differs, or
# built bytes whose sha256 differs from the pin all exit 1 and install nothing.
#
# Reproducibility: a cargo build runs in $CITRATE_COMPONENT_BUILD_ROOT/<id>-<rev12>/{src,target}
# (default ~/.cache/citrate-components) with the pinned toolchain and --locked. On macOS the
# linker derives the Mach-O UUID from the output path, so the same bytes come back only from the
# same build root on the same machine and user: macOS pins are the release Mac's builds. Builders
# for other targets (the DGX for Linux) record their own pins with --print-digest.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PINS=(node "$REPO_ROOT/scripts/component-pins.mjs")

die() { echo "ERROR: $*" >&2; exit 1; }

ID="${1:-}"
[ -n "$ID" ] && [ "${ID#-}" = "$ID" ] || die "usage: build-component.sh <id> [--checkout <dir>] [--target <triple>] [--out-dir <dir>] [--print-digest]"
shift
CHECKOUT=""; TARGET=""; OUT_DIR=""; PRINT=0
while [ $# -gt 0 ]; do
  case "$1" in
    --checkout) CHECKOUT="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --print-digest) PRINT=1; shift ;;
    -h|--help) sed -n '2,36p' "$0"; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

field() { "${PINS[@]}" field "$ID" "$1"; }
field_or_empty() { "${PINS[@]}" field "$ID" "$1" 2>/dev/null || true; }

command -v node >/dev/null || die "node not found (needed to read release/components.json)"
KIND="$(field build.kind)"
BIN_NAME="$(basename "$(field external_bin)")"
if [ -z "$TARGET" ]; then
  command -v rustc >/dev/null && TARGET="$(rustc -vV | awk '/^host:/{print $2}')"
fi
[ -n "$TARGET" ] || die "could not determine the target triple; pass --target"
EXT=""; case "$TARGET" in *-windows-*) EXT=".exe" ;; esac
[ -n "$OUT_DIR" ] || OUT_DIR="$REPO_ROOT/src-tauri/binaries"
OUT="$OUT_DIR/${BIN_NAME}-${TARGET}${EXT}"
BUILD_ROOT="${CITRATE_COMPONENT_BUILD_ROOT:-$HOME/.cache/citrate-components}"

# Resolve the pin BEFORE building, so a target with no pin fails in seconds, not after a build.
PIN=""
if [ "$PRINT" -eq 0 ]; then
  PIN="$("${PINS[@]}" digest "$ID" "$TARGET")" || die "no pin for $ID on $TARGET in release/components.json; build with --print-digest and pin the result in a reviewed PR"
fi

sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }

case "$KIND" in
  cargo)
    REPO="$(field source.repo)"
    REV="$(field source.rev)"
    [[ "$REV" =~ ^[0-9a-f]{40}$ ]] || die "$ID: pinned rev is not a full commit: $REV"
    TOOLCHAIN="$("${PINS[@]}" field - toolchain.rust)"
    PKG="$(field build.package)"; CARGO_BIN="$(field build.bin)"
    FEATURES="$(field_or_empty build.features)"
    STRIP="$(field_or_empty build.strip)"
    [ -n "$CHECKOUT" ] || CHECKOUT="$(cd "$REPO_ROOT/.." && pwd)/$(field source.checkout)"
    command -v cargo >/dev/null || die "cargo not found"
    git -C "$CHECKOUT" rev-parse --git-dir >/dev/null 2>&1 || die "$CHECKOUT is not a git checkout of $REPO (use --checkout <dir>)"
    if ! git -C "$CHECKOUT" cat-file -e "${REV}^{commit}" 2>/dev/null; then
      git -C "$CHECKOUT" fetch --quiet origin 2>/dev/null || true
      git -C "$CHECKOUT" cat-file -e "${REV}^{commit}" 2>/dev/null \
        || die "$CHECKOUT does not have the pinned commit $REV of $REPO (git -C \"$CHECKOUT\" fetch origin)"
    fi
    # The pinned rev must contain every rev the app requires of this source (`requires`).
    "${PINS[@]}" requires "$ID" --checkout "$CHECKOUT" || die "$ID: the pinned rev lacks code this app requires; nothing built"
    W="$BUILD_ROOT/${ID}-${REV:0:12}"
    SRC="$W/src"
    if [ -d "$SRC" ]; then
      [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null)" = "$REV" ] || die "build tree $SRC is not at $REV; remove $W and re-run"
      [ -z "$(git -C "$SRC" status --porcelain 2>/dev/null)" ] || die "build tree $SRC has local changes; remove $W and re-run"
    else
      mkdir -p "$W"
      git clone --quiet --shared --no-checkout "$CHECKOUT" "$SRC"
      git -C "$SRC" checkout --quiet --detach "$REV"
    fi
    echo "component : $ID ($REPO@$REV)"
    echo "toolchain : $TOOLCHAIN"
    echo "target    : $TARGET"
    echo "build tree: $W"
    ARGS=(build --release --locked -p "$PKG" --bin "$CARGO_BIN" --target "$TARGET")
    [ -n "$FEATURES" ] && ARGS+=(--features "${FEATURES// /,}")
    ENV_ARGS=()
    while IFS= read -r kv; do [ -n "$kv" ] && ENV_ARGS+=("$kv"); done < <(field_or_empty build.env)
    echo "▶ cargo ${ARGS[*]}${ENV_ARGS[*]:+  (env ${ENV_ARGS[*]})}"
    ( cd "$SRC" && env ${ENV_ARGS[@]+"${ENV_ARGS[@]}"} RUSTUP_TOOLCHAIN="$TOOLCHAIN" CARGO_TARGET_DIR="$W/target" CARGO_INCREMENTAL=0 cargo "${ARGS[@]}" )
    BUILT="$W/target/$TARGET/release/${CARGO_BIN}${EXT}"
    [ -f "$BUILT" ] || die "the build produced no binary at $BUILT"
    mkdir -p "$W/out"
    STAGE="$W/out/${BIN_NAME}-${TARGET}${EXT}"
    rm -f "$STAGE"; cp "$BUILT" "$STAGE"; chmod 755 "$STAGE"
    if [ "$STRIP" = "true" ]; then
      case "$TARGET" in *-apple-darwin) TOS=Darwin ;; *-linux-*) TOS=Linux ;; *) TOS=other ;; esac
      [ "$TOS" = "$(uname -s)" ] || die "$ID is pinned stripped, and $TARGET binaries can only be stripped on a $TOS host"
      command -v strip >/dev/null || die "strip not found"
      strip -x "$STAGE"
      echo "✓ strip -x"
    fi
    PROV_SRC="$REPO@$REV toolchain=$TOOLCHAIN${FEATURES:+ features=${FEATURES// /,}}"
    ;;
  download)
    VERSION="$(field source.version)"
    URL="$(field "artifacts.$TARGET.url")" || die "no download pinned for $ID on $TARGET"
    ARCHIVE_SHA="$(field "artifacts.$TARGET.archive_sha256")"
    MEMBER="$(field "artifacts.$TARGET.member")"
    W="$BUILD_ROOT/${ID}-${VERSION}-${TARGET}"
    rm -rf "$W"; mkdir -p "$W/x"
    echo "component : $ID $VERSION ($URL)"
    command -v curl >/dev/null || die "curl not found"
    curl -fsSL --retry 3 -o "$W/archive" "$URL" || die "download failed: $URL"
    got="$(sha256_of "$W/archive")"
    [ "$got" = "$ARCHIVE_SHA" ] || die "archive digest mismatch for $URL: got $got, pinned $ARCHIVE_SHA. Nothing installed."
    case "$URL" in
      *.zip) unzip -q "$W/archive" "$MEMBER" -d "$W/x" ;;
      *) tar -xzf "$W/archive" -C "$W/x" "$MEMBER" ;;
    esac
    STAGE="$W/x/$MEMBER"
    [ -f "$STAGE" ] || die "archive has no $MEMBER"
    chmod 755 "$STAGE"
    PROV_SRC="$(field source.repo)@$VERSION archive_sha256=$ARCHIVE_SHA"
    ;;
  manual)
    die "$ID is built by hand ($(field build.doc)); stage it from the pinned runtime-deps asset"
    ;;
  *) die "$ID: unknown build kind '$KIND'" ;;
esac

# Portability (macOS): a sidecar that links anything outside /System and /usr/lib runs only on the
# build host (the liblzma class: a Homebrew dylib, refused under hardened runtime).
if [ "$KIND" = "cargo" ]; then
  case "$TARGET" in *-apple-darwin)
    if command -v otool >/dev/null; then
      LEAKS="$(otool -L "$STAGE" | tail -n +2 | grep -vE '^\s+(/System/|/usr/lib/)' || true)"
      [ -z "$LEAKS" ] || die "$ID links dylibs that do not exist on a member's Mac:
$LEAKS"
    fi ;;
  esac
fi

# The bytes must carry every `requires` marker (fails closed on a build of an older source).
"${PINS[@]}" verify-bytes "$ID" "$STAGE" || die "$ID: the built bytes lack an interface this app requires. Nothing installed."

DIGEST="$(sha256_of "$STAGE")"
if [ "$PRINT" -eq 1 ]; then
  echo "$ID $TARGET $DIGEST $STAGE"
  exit 0
fi
if [ "$DIGEST" != "$PIN" ]; then
  die "digest mismatch for $ID on $TARGET: built $DIGEST, pinned $PIN. Nothing installed.
If the change is intended, re-pin it in a reviewed PR (--print-digest)."
fi

mkdir -p "$OUT_DIR"
tmp="$OUT.tmp.$$"
cp "$STAGE" "$tmp" && chmod 755 "$tmp" && mv -f "$tmp" "$OUT"
printf '%s  %s  %s  sha256=%s  verified=pin  %s\n' "$TARGET" "$ID" "$PROV_SRC" "$DIGEST" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$OUT.provenance"
echo "✓ installed $OUT"
echo "  sha256 $DIGEST (matches the pin)"
