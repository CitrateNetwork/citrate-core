#!/usr/bin/env bash
# build-sidecar.sh — build the bundled `citrate` node sidecar from citrate-chain at its pin.
#
# WHY THIS EXISTS
#
# src-tauri/binaries/citrate-<target-triple> is ~50 MB and .gitignore'd, so it is
# NOT in this repo. `git clone && npm run tauri build` therefore produces an app
# with no node. Worse, the obvious workaround — copying whatever `citrate` binary
# happens to be lying around — can produce an app that FORKS THE CHAIN, silently.
#
# The node is built from the rev pinned for `citrate-node` in release/components.json (never the
# checkout's HEAD) by scripts/build-component.sh, which installs it only if its sha256 matches the
# pin. Before that, this script checks the PINNED SOURCE for the consensus lineage below, so a pin
# that moved to an old commit fails here, not days later on-chain.
#
# Chain 40204 was re-rolled 2026-08-04 and both consensus activation heights were
# set to 0 (citrate-chain PR #157):
#
#     VALUE_TRANSFER_ACTIVATION_HEIGHT  300_000 -> 0
#     MERGE_DEPTH_ACTIVATION_HEIGHT     100_000 -> 0
#
# A sidecar built from an older citrate-chain still has the old values. It will
# connect, sync, and look healthy — while computing DIFFERENT state roots below
# height 300k and accepting merge blocks the fleet rejects.
#
# Usage:
#   scripts/build-sidecar.sh                       # host target, ../citrate-chain
#   scripts/build-sidecar.sh --chain ~/src/citrate-chain
#   scripts/build-sidecar.sh --target aarch64-apple-darwin
#   scripts/build-sidecar.sh --print-digest        # build, print the digest, install nothing
#
# The node is built with LZMA_API_STATIC=1 (release/components.json build.env): lzma-sys probes
# pkg-config first, so on a machine with Homebrew `xz` it would link
# /opt/homebrew/opt/xz/lib/liblzma.5.dylib dynamically, which does not exist on a member's Mac and
# which hardened runtime refuses ("different Team IDs"). build-component.sh's portability gate
# refuses any macOS sidecar that links outside /System and /usr/lib.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHAIN_DIR="${CHAIN_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-chain}"
PASS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --chain) CHAIN_DIR="$2"; shift 2 ;;
    --target|--out-dir) PASS+=("$1" "$2"); shift 2 ;;
    --print-digest) PASS+=("$1"); shift ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

die() { echo "ERROR: $*" >&2; exit 1; }

# ── activation-hardening floor ─────────────────────────────────────────────────
# The node must carry the activation hardening (citrate-chain f9f6257b, #240; re-pinned to the
# r1005 reroll G + book, 2979a157; v0.5.1 fork safety, c384fc98: bounded merge-block scoring, epoch
# producer set, canonical checkpoints, block-timing fix, storage key from a file). A node without it
# rejects nothing at the activation height and forks from the fleet there. `component-pins.mjs
# check` (CI) fails unless the citrate-node pin, this floor and the app's citrate-chain crates in
# src-tauri/Cargo.toml are the same rev.
MIN_CHAIN_REV="c384fc982882f830306142c70dd643b59fcda1f6"

[ -d "$CHAIN_DIR" ] || die "citrate-chain not found at $CHAIN_DIR (use --chain <path>)"
REV="$(node "$REPO_ROOT/scripts/component-pins.mjs" field citrate-node source.rev)"
git -C "$CHAIN_DIR" cat-file -e "${REV}^{commit}" 2>/dev/null || git -C "$CHAIN_DIR" fetch --quiet origin 2>/dev/null || true
git -C "$CHAIN_DIR" cat-file -e "${REV}^{commit}" 2>/dev/null || die "citrate-chain checkout does not have the pinned rev $REV (git -C \"$CHAIN_DIR\" fetch origin)"

echo "▶ verifying the pinned citrate-chain source ($REV)"
src() { git -C "$CHAIN_DIR" show "$REV:$1" 2>/dev/null; }
vt="$(src core/execution/src/executor.rs | grep -oE 'pub const VALUE_TRANSFER_ACTIVATION_HEIGHT: u64 = [0-9_]+' | grep -oE '[0-9_]+$' | tr -d _ || true)"
md="$(src core/consensus/src/ghostdag.rs | grep -oE 'pub const MERGE_DEPTH_ACTIVATION_HEIGHT: u64 = [0-9_]+' | grep -oE '[0-9_]+$' | tr -d _ || true)"
[ -n "$vt" ] || die "could not read VALUE_TRANSFER_ACTIVATION_HEIGHT at $REV"
[ -n "$md" ] || die "could not read MERGE_DEPTH_ACTIVATION_HEIGHT at $REV"
echo "    VALUE_TRANSFER_ACTIVATION_HEIGHT = $vt"
echo "    MERGE_DEPTH_ACTIVATION_HEIGHT    = $md"
if [ "$vt" != "0" ] || [ "$md" != "0" ]; then
  die "the pinned citrate-chain rev $REV predates the 2026-08-04 re-roll (both heights must be 0); a sidecar built from it WILL FORK chain 40204"
fi
git -C "$CHAIN_DIR" merge-base --is-ancestor "$MIN_CHAIN_REV" "$REV" 2>/dev/null \
  || die "the pinned citrate-chain rev $REV does not contain $MIN_CHAIN_REV (activation hardening)"
src core/consensus/src/crypto.rs | grep -q 'pub fn sign_transaction_v2' \
  || die "the pinned citrate-chain rev $REV has no V2 native signing (core/consensus/src/crypto.rs)"
echo "    OK — post-re-roll, contains $MIN_CHAIN_REV"
echo

exec "$REPO_ROOT/scripts/build-component.sh" citrate-node --checkout "$CHAIN_DIR" ${PASS[@]+"${PASS[@]}"}
