#!/usr/bin/env bash
# PBA-L7b-005 — verify every runtime-deps asset against the committed sha256 manifest BEFORE it is
# staged into the bundle (and therefore before it is Developer-ID signed, notarized and
# updater-signed). The `runtime-deps` GitHub prerelease is mutable: anyone who can overwrite a
# release asset would otherwise ship code to every auto-updating member without code review.
#
# Usage: verify-runtime-deps.sh <manifest> <download-dir> <asset>...
#   <manifest>      src-tauri/runtime-deps.sha256 (`shasum -a 256` format: "<hex>  <asset>")
#   <download-dir>  where `gh release download` put the assets
#   <asset>...      every asset the workflow downloaded (each MUST be pinned)
# Fails closed: an asset with no pin, a malformed pin, or a digest mismatch is an error.
# The Hermes knowledge corpus fails closed too: every bundle overlay's `knowledge-corpus/**/*` glob
# matches the committed README.md, so a release that skipped the corpus would still bundle (README
# only). The manifest must pin knowledge-corpus.tar.gz, and a call that stages a mem-mcp sidecar
# must stage the corpus that mem-mcp imports (scripts/check-staged-corpus.mjs then checks the staged
# directory right before bundling).
set -euo pipefail
manifest="$1"; dir="$2"; shift 2
[ "$#" -gt 0 ] || { echo "::error::no assets given to verify"; exit 1; }
[ -f "$manifest" ] || { echo "::error::runtime-deps manifest missing: $manifest"; exit 1; }
CORPUS=knowledge-corpus.tar.gz
n="$(grep -cE "^[0-9a-f]{64}  ${CORPUS//./\\.}\$" "$manifest" || true)"
if [ "$n" -ne 1 ]; then
  echo "::error::$manifest does not pin $CORPUS exactly once (found $n): the knowledge corpus is required (checklist A9)"
  exit 1
fi
stages_mem_mcp=0; stages_corpus=0
for asset in "$@"; do
  case "$asset" in
    mem-mcp-*) stages_mem_mcp=1 ;;
    "$CORPUS") stages_corpus=1 ;;
  esac
done
if [ "$stages_mem_mcp" -eq 1 ] && [ "$stages_corpus" -eq 0 ]; then
  echo "::error::a mem-mcp is staged without $CORPUS: the app would ship a README-only knowledge corpus"
  exit 1
fi
tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
for asset in "$@"; do
  line="$(grep -E "^[0-9a-f]{64}  ${asset//./\\.}\$" "$manifest" || true)"
  n="$(printf '%s' "$line" | grep -c . || true)"
  if [ "$n" -ne 1 ]; then
    echo "::error::runtime-deps asset '$asset' is not pinned exactly once in $manifest (found $n) — refusing to stage an unpinned binary"
    exit 1
  fi
  [ -f "$dir/$asset" ] || { echo "::error::downloaded asset missing: $dir/$asset"; exit 1; }
  printf '%s\n' "$line" >> "$tmp"
done
( cd "$dir" && shasum -a 256 -c --strict "$tmp" ) || {
  echo "::error::runtime-deps digest mismatch — an asset differs from the reviewed pin"; exit 1; }
echo "runtime-deps: $# assets verified against $manifest"
