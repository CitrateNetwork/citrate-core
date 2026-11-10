#!/usr/bin/env bash
# The network profile check (K6). In CI on every push; with --release it is the RC build gate
# (G0): the active profile must have every value set, so no release ships a network it refuses
# to sign on.
#
#   scripts/check-network-profile.sh [--release] [--require-active <profile>] [--canonical-book <file>]
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 -I "$here/network-profile.py" check "$@"
