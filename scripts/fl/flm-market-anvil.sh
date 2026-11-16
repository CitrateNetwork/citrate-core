#!/usr/bin/env bash
# created: 2026-11-11 | branch: feat/flm-core-1-offers-read-model | author: Larry Klosowski + Claude Opus 5.5 (V60-FLM lane) | status: active
#
# FLM-CORE-1 conformance: the offers read model (src-tauri/src/fl_market.rs) against the REAL
# merged WorkMarket on a throwaway local anvil. It deploys FederatedRoundLedger, FLRoundVerifier and
# WorkMarket from a citrate-chain checkout and opens four offers:
#   1. an open FL offer whose requirements file hashes to its requirementsHash;
#   2. a group FL offer (groupHash = keccak256 of a 32-byte MLS group id);
#   3. an open FL offer whose requirementsHash does not match its file;
#   4. an open FL offer with a short deadline, refunded after it (OfferClosed, Expired).
# Then it mines past the read model's confirmation depth and either
#   - runs the ignored live test `fl_market_reads_the_merged_work_market_on_anvil`
#     (cargo test -p citrate-core --lib fl_market -- --ignored), or
#   - with FLM_FIXTURE_OUT=<dir>, writes the node's eth_getLogs answer and each getOffer return
#     there (the recorded fixtures the unit tests use).
#
# Network safety: anvil binds 127.0.0.1 only and every call targets that URL; the script refuses
# any other RPC. Nothing here reaches a fleet host or a public chain.
#
# Usage: CHAIN_CONTRACTS=/path/to/citrate-chain/contracts scripts/fl/flm-market-anvil.sh
set -euo pipefail

: "${CHAIN_CONTRACTS:?set CHAIN_CONTRACTS to a citrate-chain/contracts checkout (forge-built)}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PORT="${ANVIL_PORT:-18546}"
RPC="http://127.0.0.1:${PORT}"
FOUNDRY="${FOUNDRY_BIN:-$HOME/.foundry/bin}"
ANVIL="$FOUNDRY/anvil"; CAST="$FOUNDRY/cast"; FORGE="$FOUNDRY/forge"
case "$RPC" in http://127.0.0.1:*) ;; *) echo "refusing non-loopback RPC $RPC" >&2; exit 1 ;; esac

WORK="$(mktemp -d)"
"$ANVIL" --host 127.0.0.1 --port "$PORT" --chain-id 40204 --silent &
ANVIL_PID=$!
trap 'kill $ANVIL_PID 2>/dev/null || true; rm -rf "$WORK"' EXIT
for _ in $(seq 1 50); do "$CAST" chain-id --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.2; done
[ "$("$CAST" chain-id --rpc-url "$RPC")" = "40204" ] || { echo "anvil did not come up on chain 40204" >&2; exit 1; }

# anvil's public dev keys: 0 deployer/governance, 1 poster, 2 coordinator.
K0=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
K1=0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
GOV=$("$CAST" wallet address --private-key $K0)
COORD=0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC
GUARDIAN=0x976EA74026E726554dB657fA54763abd0C3a0aa9
SINK=0x14dC79964da2C08b23698B3D3cc7Ca32193d9955
create() { (cd "$CHAIN_CONTRACTS" && "$FORGE" create --rpc-url "$RPC" --private-key $K0 --broadcast "$@") \
  | awk '/Deployed to:/ {print $3}'; }
FRL=$(create src/FederatedRoundLedger.sol:FederatedRoundLedger)
FLV=$(create src/work/FLRoundVerifier.sol:FLRoundVerifier --constructor-args "$FRL")
WM=$(create src/work/WorkMarket.sol:WorkMarket --constructor-args "$GOV" "$GUARDIAN" "$SINK" \
  1000000000000000000 100000000000000000 "[$FLV]")
echo "deployed WorkMarket=$WM"

# The requirements files (their keccak256 is what an offer commits).
REQ_DIR="$WORK/req"; mkdir -p "$REQ_DIR"
printf '{"v":1,"dataClasses":["text.public"],"architectures":["llama"],"minVramGb":16,"estHours":2}' > "$REQ_DIR/bafkreiflmopen"
printf '{"v":1,"dataClasses":["text.group"],"architectures":["llama"],"minVramGb":8,"estHours":1}' > "$REQ_DIR/bafkreiflmgroup"
printf '{"v":1,"dataClasses":["text.public"],"architectures":["llama"],"minVramGb":4,"estHours":1}' > "$REQ_DIR/bafkreiflmswapped"
printf '{"v":1,"dataClasses":["text.public"],"architectures":["llama"],"minVramGb":4,"estHours":1}' > "$REQ_DIR/bafkreiflmshort"
hash_of() { "$CAST" keccak "$(cat "$REQ_DIR/$1")"; }  # the file's UTF-8 bytes (no 0x prefix)
GROUP_ID=0x$(printf '11%.0s' {1..32})
GROUP_HASH=$("$CAST" keccak "$GROUP_ID")
ZERO=0x$(printf '0%.0s' {1..64})
NOW=$("$CAST" block latest --field timestamp --rpc-url "$RPC")
LONG=$((NOW + 30 * 86400)); SHORT=$((NOW + 86400 + 3600))
OPEN_SIG="open((uint8,address,bytes32,bytes32,bytes32,string,bytes32,bytes32,uint128,uint64,uint32,uint32,uint128,address,uint128,address))"
open_offer() { # bindLabel reqHash cid groupHash deadline
  "$CAST" send --rpc-url "$RPC" --private-key $K1 "$WM" "$OPEN_SIG" \
    "(0,$FLV,$("$CAST" keccak "$1"),$("$CAST" keccak job),$2,$3,$4,$ZERO,1000000000000000000,$5,86400,8,0,$COORD,0,0x0000000000000000000000000000000000000000)" \
    --value 5ether >/dev/null; }
open_offer b1 "$(hash_of bafkreiflmopen)" bafkreiflmopen "$ZERO" $LONG
open_offer b2 "$(hash_of bafkreiflmgroup)" bafkreiflmgroup "$GROUP_HASH" $LONG
open_offer b3 "$(hash_of bafkreiflmopen)" bafkreiflmswapped "$ZERO" $LONG
open_offer b4 "$(hash_of bafkreiflmshort)" bafkreiflmshort "$ZERO" $SHORT
"$CAST" rpc --rpc-url "$RPC" evm_increaseTime $((2 * 86400)) >/dev/null
"$CAST" rpc --rpc-url "$RPC" anvil_mine 0x1 >/dev/null
"$CAST" send --rpc-url "$RPC" --private-key $K1 "$WM" "refund(uint256)" 4 >/dev/null
"$CAST" rpc --rpc-url "$RPC" anvil_mine 0x10 >/dev/null
echo "opened 4 offers, refunded offer 4; head $("$CAST" block-number --rpc-url "$RPC")"

if [ -n "${FLM_FIXTURE_OUT:-}" ]; then
  mkdir -p "$FLM_FIXTURE_OUT"
  "$CAST" rpc --rpc-url "$RPC" eth_getLogs \
    "{\"address\":\"$WM\",\"fromBlock\":\"0x0\",\"toBlock\":\"latest\",\"topics\":[[\"$("$CAST" keccak 'OfferOpened(uint256,uint8,address,bytes32,uint128,uint128,uint64,string)')\",\"$("$CAST" keccak 'OfferClosed(uint256,uint8)')\"]]}" > "$FLM_FIXTURE_OUT/logs.json"
  for id in 1 2 3 4; do
    "$CAST" call --rpc-url "$RPC" "$WM" "getOffer(uint256)" $id > "$FLM_FIXTURE_OUT/get_offer_$id.hex"
  done
  cp "$REQ_DIR"/* "$FLM_FIXTURE_OUT/"
  echo "market=$WM block0=$("$CAST" block 0 --field hash --rpc-url "$RPC")" > "$FLM_FIXTURE_OUT/README.txt"
  echo "fixtures written to $FLM_FIXTURE_OUT"
  exit 0
fi

cd "$ROOT/src-tauri"
FLM_ANVIL_RPC="$RPC" FLM_ANVIL_MARKET="$WM" FLM_ANVIL_REQ_DIR="$REQ_DIR" FLM_ANVIL_GROUP_ID="$GROUP_ID" \
  cargo test --locked --lib fl_market::tests::fl_market_reads_the_merged_work_market_on_anvil -- --ignored --exact
