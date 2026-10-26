#!/usr/bin/env bash
# build-hermes.sh — build the bundled `hermes` sidecar from citrate-agent-runtime at its pin.
#
# WHY THIS EXISTS
#
# citrate-core spawns the Hermes operator sidecar (the `agent-sidecar` crate → bin
# `citrate-agent-sidecar`) as a supervised child and drives its loopback control API
# (src-tauri/src/hermes.rs: hands it CITRATE_HERMES_ADDR + a 0600 CITRATE_HERMES_TOKEN_FILE). The
# binary is large + platform-specific + .gitignore'd, so it is NOT in this repo — a fresh checkout
# has no hermes sidecar and `hermes_start` returns BinaryNotFound honestly until this runs. It is
# declared as a Tauri `externalBin` in the packaging overlays as `binaries/hermes`, so the produced
# file is `hermes-<triple>`.
#
# The sidecar is built from the rev pinned for `hermes-sidecar` in release/components.json (never the
# checkout's HEAD) by scripts/build-component.sh, which installs it only if its sha256 matches the
# pin.
#
# HUP-S11.0 (gate g5-size): the pinned bytes have their local symbols removed with `strip -x`
# (about 4 MB on aarch64-apple-darwin; build.strip in the manifest). Exported symbols stay, so the
# binary links and runs the same; panic backtraces lose the names of local functions. Tauri signs
# the externalBin after this, at bundle time. Stripping needs a host of the target's OS. For a
# debuggable build, run cargo in citrate-agent-runtime yourself and point CITRATE_HERMES_BIN at it.
#
# Usage:
#   scripts/build-hermes.sh                                   # host triple, ../citrate-agent-runtime
#   scripts/build-hermes.sh --agent-runtime ~/src/citrate-agent-runtime
#   scripts/build-hermes.sh --target aarch64-apple-darwin
#   scripts/build-hermes.sh --out-dir <dir>                   # install somewhere other than src-tauri/binaries
#   scripts/build-hermes.sh --print-digest                    # build, print the digest, install nothing
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RT_DIR="${RT_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-agent-runtime}"
PASS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --agent-runtime) RT_DIR="$2"; shift 2 ;;
    --target|--out-dir) PASS+=("$1" "$2"); shift 2 ;;
    --print-digest) PASS+=("$1"); shift ;;
    -h|--help) sed -n '2,31p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

exec "$REPO_ROOT/scripts/build-component.sh" hermes-sidecar --checkout "$RT_DIR" ${PASS[@]+"${PASS[@]}"}
