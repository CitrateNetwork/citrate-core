#!/usr/bin/env bash
# build-comms-daemon.sh — build the bundled `comms-member-daemon` sidecar from citrate-comms at its pin.
#
# WHY THIS EXISTS
#
# citrate-core spawns the citrate-comms member-daemon (the wallet-owned OpenMLS member + an
# in-process server-blind relay) as a supervised sidecar and speaks its loopback UDS JSON IPC
# (src-tauri/src/comms.rs). The daemon binary is platform-specific + .gitignore'd, so it is NOT in
# this repo — a fresh checkout has no comms daemon and Groups surface an honest "binary not bundled"
# until this runs. It is declared as a Tauri `externalBin` in the packaging overlays as
# `binaries/comms-member-daemon`. It REPLACES the retired `comms-relay` sidecar.
#
# The daemon is built from the rev pinned for `comms-member-daemon` in release/components.json (never
# the checkout's HEAD) by scripts/build-component.sh, which installs it only if its sha256 matches
# the pin.
#
# Usage:
#   scripts/build-comms-daemon.sh                          # host triple, ../citrate-comms
#   scripts/build-comms-daemon.sh --comms ~/src/citrate-comms
#   scripts/build-comms-daemon.sh --target aarch64-apple-darwin
#   scripts/build-comms-daemon.sh --print-digest           # build, print the digest, install nothing
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
# Dev/tests do not need this: set CITRATE_MEMBER_BIN to point the app at any build.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMS_DIR="${COMMS_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-comms}"
PASS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --comms) COMMS_DIR="$2"; shift 2 ;;
    --target|--out-dir) PASS+=("$1" "$2"); shift 2 ;;
    --print-digest) PASS+=("$1"); shift ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

exec "$REPO_ROOT/scripts/build-component.sh" comms-member-daemon --checkout "$COMMS_DIR" ${PASS[@]+"${PASS[@]}"}
