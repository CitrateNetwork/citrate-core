#!/usr/bin/env bash
# HUP-S1.5 — registry escalation dry run on a LOCAL anvil chain (never 40204, never a real key).
#
# Builds InferenceRouter and WrappedSALT from citrate-chain source, starts anvil with chain id 40204
# (so the EIP-712 domain matches production), and runs the ignored integration test
# `inference_router::tests::anvil_dry_run_inference_router_and_wsalt_authorization`, which:
#   - deploys both contracts from anvil's unlocked dev accounts,
#   - registers a compute provider, quotes from the live route, estimates gas, and sends the exact
#     requestInference tx JSON the SignatureCeremony would carry,
#   - completes the request as the provider and reads the answer, price paid and refund back
#     through core's decoders, then claims the refund,
#   - checks the kit's EIP-712 domain separator and type hash against WrappedSALT and has the
#     contract accept a TransferWithAuthorization signed over the kit's digest (and refuse a replay).
#
# Usage: scripts/anvil-registry-dryrun.sh [path/to/citrate-chain]
# Env:   CITRATE_CHAIN_DIR (default: ../citrate-chain next to this repo), ANVIL_PORT (default 8645),
#        CARGO_TARGET_DIR (honoured as usual).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
chain="${1:-${CITRATE_CHAIN_DIR:-$here/../citrate-chain}}"
port="${ANVIL_PORT:-8645}"

for tool in anvil forge cargo; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing tool: $tool" >&2; exit 2; }
done
[ -f "$chain/contracts/src/InferenceRouter.sol" ] || { echo "citrate-chain not found at $chain" >&2; exit 2; }

work="$(mktemp -d "${TMPDIR:-/tmp}/citrate-anvil-dryrun.XXXXXX")"
anvil_pid=""
cleanup() {
  if [ -n "$anvil_pid" ]; then kill "$anvil_pid" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

echo "==> building InferenceRouter + WrappedSALT from $chain (artifacts in $work/out)"
(cd "$chain/contracts" && forge build --skip test --skip script \
  --out "$work/out" --cache-path "$work/cache" \
  src/InferenceRouter.sol src/WrappedSALT.sol >/dev/null)

echo "==> starting anvil (chain id 40204) on 127.0.0.1:$port"
anvil --chain-id 40204 --port "$port" --silent >"$work/anvil.log" 2>&1 &
anvil_pid=$!
for _ in $(seq 1 50); do
  if curl -s -X POST -H 'content-type: application/json' \
      --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' \
      "http://127.0.0.1:$port" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

echo "==> running the dry run"
cd "$here"
CITRATE_ANVIL_RPC="http://127.0.0.1:$port" CITRATE_ANVIL_ARTIFACTS="$work/out" \
  cargo test -p citrate-core --lib \
  inference_router::tests::anvil_dry_run_inference_router_and_wsalt_authorization \
  -- --ignored --exact --nocapture
