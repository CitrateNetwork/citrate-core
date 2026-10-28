#!/usr/bin/env bash
# check-reproducible.sh — build one cargo component twice, each from clean in its own new build
# root (the two roots have different paths), and check that both builds give the same bytes.
#
# Usage:
#   scripts/check-reproducible.sh <id> [--checkout <dir>] [--target <triple>] [--keep]
#
#   <id>        a cargo component from release/components.json (e.g. citrate-node)
#   --checkout  passed to build-component.sh (the local git checkout holding the pinned rev)
#   --target    passed to build-component.sh (default: the host's triple)
#   --keep      keep both build roots (for diffoscope) instead of removing them
#
# Both builds run through scripts/build-component.sh --print-digest, one after the other, so they
# use exactly the release recipe, and nothing is installed. Prints
#   reproducible: <id> <triple> <sha256>
# and exits 0 when the digests match. Otherwise it prints both digests and exits 1. A pin proposed
# for a component with `build.reproducible: true` should come with a passing run on each target.
#
# The build roots are made with mktemp under $TMPDIR (default /tmp). A cargo build of the node
# needs several GB there; point TMPDIR at a roomy disk if /tmp is small.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
die() { echo "ERROR: $*" >&2; exit 1; }

ID="${1:-}"
[ -n "$ID" ] && [ "${ID#-}" = "$ID" ] || die "usage: check-reproducible.sh <id> [--checkout <dir>] [--target <triple>] [--keep]"
shift
PASS=(); KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --checkout|--target) PASS+=("$1" "$2"); shift 2 ;;
    --keep) KEEP=1; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

KIND="$(node "$REPO_ROOT/scripts/component-pins.mjs" field "$ID" build.kind)" || die "unknown component: $ID"
[ "$KIND" = "cargo" ] || die "$ID is a '$KIND' component; only cargo components are rebuilt (check-reproducible.sh checks only cargo components)"

BASE="${TMPDIR:-/tmp}"
ROOT_A="$(mktemp -d "$BASE/repro-a.XXXXXX")"
ROOT_B="$(mktemp -d "$BASE/repro-second-root.XXXXXX")"
cleanup() {
  if [ "$KEEP" -eq 1 ]; then
    echo "kept build roots: $ROOT_A $ROOT_B"
    return
  fi
  # Only the two directories this script made.
  for d in "$ROOT_A" "$ROOT_B"; do
    case "$d" in "$BASE"/repro-*) rm -rf -- "$d" ;; esac
  done
}
trap cleanup EXIT

build() {
  local root="$1" log="$1/build.log" line
  if ! CITRATE_COMPONENT_BUILD_ROOT="$root/cb" "$REPO_ROOT/scripts/build-component.sh" "$ID" ${PASS[@]+"${PASS[@]}"} \
      --out-dir "$root/out" --print-digest > "$log" 2>&1; then
    tail -n 30 "$log" >&2
    die "build in $root failed"
  fi
  line="$(tail -n 1 "$log")"
  # "<id> <triple> <sha256> <path>"
  set -- $line
  [ "$1" = "$ID" ] && [[ "$3" =~ ^[0-9a-f]{64}$ ]] || { tail -n 5 "$log" >&2; die "unexpected build output in $root"; }
  echo "$2 $3"
}

echo "▶ build 1 of 2 in $ROOT_A"
read -r TRIPLE DIGEST_A < <(build "$ROOT_A")
echo "  $DIGEST_A"
echo "▶ build 2 of 2 in $ROOT_B"
read -r _ DIGEST_B < <(build "$ROOT_B")
echo "  $DIGEST_B"

if [ "$DIGEST_A" = "$DIGEST_B" ]; then
  echo "reproducible: $ID $TRIPLE $DIGEST_A"
  exit 0
fi
echo "NOT reproducible: $ID $TRIPLE"
echo "  build root A: $DIGEST_A"
echo "  build root B: $DIGEST_B"
[ "$KEEP" -eq 1 ] || echo "  (re-run with --keep and compare the two binaries with diffoscope)"
exit 1
