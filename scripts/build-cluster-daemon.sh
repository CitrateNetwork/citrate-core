#!/usr/bin/env bash
# build-cluster-daemon.sh — build the bundled `cluster-daemon` sidecar from citrate-cluster at its pin.
#
# WHY THIS EXISTS
#
# citrate-core spawns the citrate-cluster daemon (a group's private P2P mesh — Noise + gossipsub,
# admission gated by cluster-core) as a supervised sidecar and speaks its loopback UDS JSON IPC
# (src-tauri/src/cluster.rs). The binary is platform-specific + .gitignore'd, so a fresh checkout has
# no cluster daemon and the Cluster surface surfaces an honest "binary not bundled" until this runs.
# It is declared as a Tauri `externalBin` in the packaging overlays as `binaries/cluster-daemon`.
#
# The daemon is built from the rev pinned for `cluster-daemon` in release/components.json (never the
# checkout's HEAD) by scripts/build-component.sh, which installs it only if its sha256 matches the pin.
#
# Usage:
#   scripts/build-cluster-daemon.sh                            # host triple, ../citrate-cluster
#   scripts/build-cluster-daemon.sh --cluster ~/src/citrate-cluster
#   scripts/build-cluster-daemon.sh --target aarch64-apple-darwin
#   scripts/build-cluster-daemon.sh --print-digest             # build, print the digest, install nothing
#
# Binaries do NOT cross architectures — build a Mac sidecar on a Mac.
# Dev/tests do not need this: set CITRATE_CLUSTER_BIN to point the app at any build.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLUSTER_DIR="${CLUSTER_DIR:-$(cd "$REPO_ROOT/.." && pwd)/citrate-cluster}"
PASS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --cluster) CLUSTER_DIR="$2"; shift 2 ;;
    --target|--out-dir) PASS+=("$1" "$2"); shift 2 ;;
    --print-digest) PASS+=("$1"); shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

exec "$REPO_ROOT/scripts/build-component.sh" cluster-daemon --checkout "$CLUSTER_DIR" ${PASS[@]+"${PASS[@]}"}
