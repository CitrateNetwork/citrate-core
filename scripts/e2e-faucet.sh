#!/usr/bin/env bash
# HUP-S6.5 end to end, on this machine only (nothing touches chain 40204):
#
#   1. build the citrate-faucet binary from a citrate-chain checkout;
#   2. start a throwaway anvil (chain id 40204);
#   3. make a throwaway faucet key at run time (cast wallet new; never written to disk) and fund
#      its address on that anvil;
#   4. run the faucet against the anvil on a loopback port;
#   5. run core's faucet e2e test (production HTTP client, real faucet, real transfers).
#
# usage: scripts/e2e-faucet.sh --chain DIR [--work DIR]
#   --chain  a citrate-chain checkout whose faucet/ has /ready and /eligibility (HUP-S6.5)
#   --work   scratch folder for the faucet's cooldown file (removed on exit)
set -euo pipefail

CORE="$(cd "$(dirname "$0")/.." && pwd)"
CHAIN=""
WORK=""
while [ $# -gt 0 ]; do
  case "$1" in
    --chain) CHAIN="$2"; shift 2 ;;
    --work) WORK="$2"; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "e2e-faucet: unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$CHAIN" ] && [ -f "$CHAIN/faucet/Cargo.toml" ] || { echo "e2e-faucet: --chain must be a citrate-chain checkout" >&2; exit 2; }
[ -n "$WORK" ] || WORK="$(mktemp -d)"
for t in anvil cast jq curl cargo; do command -v "$t" >/dev/null || { echo "e2e-faucet: $t is not installed" >&2; exit 2; }; done

ANVIL_PORT="${E2E_ANVIL_PORT:-18546}"
FAUCET_PORT="${E2E_FAUCET_PORT:-13002}"
RPC="http://127.0.0.1:$ANVIL_PORT"
FAUCET="http://127.0.0.1:$FAUCET_PORT"
PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  rm -rf "$WORK"
}
trap cleanup EXIT
rm -rf "$WORK"; mkdir -p "$WORK"

CHAIN_TARGET="${CHAIN_TARGET_DIR:-$CHAIN/target}"
(cd "$CHAIN" && CARGO_TARGET_DIR="$CHAIN_TARGET" cargo build -q -p citrate-faucet)
BIN="$CHAIN_TARGET/debug/citrate-faucet"
[ -x "$BIN" ] || { echo "e2e-faucet: $BIN was not built" >&2; exit 1; }

anvil --chain-id 40204 --port "$ANVIL_PORT" --silent &
PIDS+=($!)
for _ in $(seq 1 50); do cast block-number --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.2; done

# A throwaway key, held only in this shell's memory for the life of the run.
KEYJSON="$(cast wallet new --json)"
FAUCET_ADDR="$(echo "$KEYJSON" | jq -r '.[0].address')"
FAUCET_KEY="$(echo "$KEYJSON" | jq -r '.[0].private_key')"
unset KEYJSON
cast rpc --rpc-url "$RPC" anvil_setBalance "$FAUCET_ADDR" 0x3635c9adc5dea00000 >/dev/null  # 1000 SALT

CITRATE_RPC_URL="$RPC" CITRATE_CHAIN_ID=40204 FAUCET_PRIVATE_KEY="$FAUCET_KEY" \
  FAUCET_PORT="$FAUCET_PORT" FAUCET_COOLDOWN_FILE="$WORK/cooldowns.json" RUST_LOG=warn \
  "$BIN" >"$WORK/faucet.log" 2>&1 &
PIDS+=($!)
unset FAUCET_KEY
for _ in $(seq 1 100); do curl -fs "$FAUCET/health" >/dev/null 2>&1 && break; sleep 0.2; done
curl -fs "$FAUCET/ready" >/dev/null || { echo "e2e-faucet: the faucet is not ready" >&2; cat "$WORK/faucet.log" >&2; exit 1; }
echo "e2e-faucet: faucet $FAUCET_ADDR serving on $FAUCET against anvil $RPC"

export CITRATE_E2E_RPC="$RPC" CITRATE_E2E_FAUCET_URL="$FAUCET"
(cd "$CORE/src-tauri" && cargo test --lib e2e_faucet_binary -- --nocapture)
echo "e2e-faucet: OK"
