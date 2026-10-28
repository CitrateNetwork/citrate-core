#!/usr/bin/env bash
# Record a reroll's values in the network profile (K6), and nothing else:
#
#   scripts/fill-network-profile.sh r1006 --genesis-hash 0x<64 hex> \
#       --address-book <citrate-chain contracts/addresses/40204.json as exported at the reroll> \
#       --min-chain-rev <40-hex citrate-chain rev of the node> [--replace] [--activate]
#
# Every input is checked before anything is written (scripts/network-profile.py has the rules).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ $# -lt 1 ] || [ "${1#-}" != "$1" ]; then
  echo "usage: $0 <profile> --genesis-hash 0x… --address-book <file> --min-chain-rev <rev> [--replace] [--activate]" >&2
  exit 2
fi
exec python3 -I "$here/network-profile.py" fill "$@"
