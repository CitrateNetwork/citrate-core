#!/usr/bin/env bash
# S0-SVC-SPIKE (V60): a stand-in for citrate-svc. It runs the bundled node and kubo headless
# under the per-user unit, so the unit's lifecycle (start at boot with lingering, restart on a
# crash, stop with a drain) can be measured before the real binary exists. Spike only: the
# shipped unit runs `citrate-svc run` (SVC-1).
#
# Local network only: `--network local --bootstrap` with loopback listen addresses, so the spike
# node never dials a real host.
set -euo pipefail
BIN="${SPIKE_BIN:?SPIKE_BIN unset}"
DATA="${SPIKE_DATA:?SPIKE_DATA unset}"
mkdir -p "$DATA/node" "$DATA/ipfs"
export IPFS_PATH="$DATA/ipfs"
if [ ! -f "$IPFS_PATH/config" ]; then
  "$BIN/ipfs" init --profile lowpower >/dev/null
  "$BIN/ipfs" config Addresses.API /ip4/127.0.0.1/tcp/15001
  "$BIN/ipfs" config Addresses.Gateway /ip4/127.0.0.1/tcp/18080
  "$BIN/ipfs" config --json Addresses.Swarm '["/ip4/127.0.0.1/tcp/14001"]'
  "$BIN/ipfs" config --json Bootstrap '[]'
  "$BIN/ipfs" config --json Routing.Type '"none"'
fi
"$BIN/ipfs" daemon --offline &
IPFS_PID=$!
"$BIN/citrate" --network local --bootstrap --mine \
  --coinbase "${SPIKE_COINBASE:-0x1111111111111111111111111111111111111111}" \
  --data-dir "$DATA/node" --rpc-addr 127.0.0.1:18545 --p2p-addr 127.0.0.1:31303 &
NODE_PID=$!
drain() {
  # The drain order the service will use: the node first (SIGINT, its shutdown path), then kubo.
  kill -INT "$NODE_PID" 2>/dev/null || true
  wait "$NODE_PID" 2>/dev/null || true
  kill -TERM "$IPFS_PID" 2>/dev/null || true
  wait "$IPFS_PID" 2>/dev/null || true
  exit 0
}
trap drain TERM INT
# Either child dying is a service failure: exit non-zero so Restart=on-failure brings it back.
wait -n "$IPFS_PID" "$NODE_PID"
kill -TERM "$IPFS_PID" "$NODE_PID" 2>/dev/null || true
exit 1
