#!/usr/bin/env bash
# HUP-S1.5 — registry escalation end to end on a LOCAL anvil chain (never 40204).
#
# Deploys citrate-chain's WrappedSALT and InferenceRouter on anvil, registers one provider, wraps
# SALT for the member account, then runs three ignored tests in order:
#   1. citrate-core  escalation_registry::tests::anvil_e2e_registry_quote_and_approve
#        read the router -> pick the provider -> quote -> build the x402 authorization ->
#        SignatureCeremony.request_x402 + approve -> verify -> write the sidecar request
#   2. citrate-agent-runtime  agent-escalation registry_tests anvil_e2e_registry_payment_settles_on_chain
#        send the paid request (X-PAYMENT) to a loopback provider on the router's endpoint, which
#        settles with wSALT.transferWithAuthorization on anvil and answers with X-PAYMENT-RESPONSE
#   3. citrate-core  escalation_registry::tests::anvil_e2e_registry_settlement_is_confirmed
#        core's own authorizationState check sees the authorization consumed
#
# Usage: scripts/escalation-registry-anvil-e2e.sh [CITRATE_CHAIN_DIR] [CITRATE_RUNTIME_DIR]
# Defaults: ../citrate-chain and ../citrate-agent-runtime next to this checkout.
# Optional: CORE_TARGET_DIR / RUNTIME_TARGET_DIR point each repo's cargo at a shared target dir.
# Keys: anvil's public test mnemonic, derived at run time. Nothing here touches chain 40204.
set -euo pipefail

CORE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CHAIN_DIR="${1:-${CITRATE_CHAIN_DIR:-$CORE_DIR/../citrate-chain}}"
RUNTIME_DIR="${2:-${CITRATE_RUNTIME_DIR:-$CORE_DIR/../citrate-agent-runtime}}"
PORT="${ANVIL_PORT:-18545}"
PROVIDER_PORT="${PROVIDER_PORT:-18999}"
RPC="http://127.0.0.1:${PORT}"

for t in anvil forge cast jq cargo; do
  command -v "$t" >/dev/null || { echo "missing tool: $t" >&2; exit 2; }
done
[ -f "$CHAIN_DIR/contracts/src/InferenceRouter.sol" ] || { echo "no citrate-chain at $CHAIN_DIR" >&2; exit 2; }
[ -d "$RUNTIME_DIR/agent-escalation" ] || { echo "no citrate-agent-runtime at $RUNTIME_DIR" >&2; exit 2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/citrate-x402-e2e.XXXXXX")"
anvil --port "$PORT" --silent >"$WORK/anvil.log" 2>&1 &
ANVIL_PID=$!
cleanup() { kill "$ANVIL_PID" 2>/dev/null || true; }
trap cleanup EXIT
for _ in $(seq 1 50); do cast chain-id --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.2; done
CHAIN_ID="$(cast chain-id --rpc-url "$RPC")"
echo "anvil up on $RPC (chain id $CHAIN_ID); work dir $WORK"

echo "== compile WrappedSALT + InferenceRouter from $CHAIN_DIR"
(cd "$CHAIN_DIR/contracts" && forge build --out "$WORK/out" --cache-path "$WORK/cache" \
  src/WrappedSALT.sol src/InferenceRouter.sol >/dev/null)

MNEMONIC="$(printf 'test %.0s' $(seq 1 11))junk"
MEMBER_KEY="$(cast wallet private-key --mnemonic "$MNEMONIC" --mnemonic-index 0)"
PROVIDER_KEY="$(cast wallet private-key --mnemonic "$MNEMONIC" --mnemonic-index 1)"
MEMBER="$(cast wallet address "$MEMBER_KEY")"
PROVIDER="$(cast wallet address "$PROVIDER_KEY")"

deploy() { cast send --rpc-url "$RPC" --private-key "$MEMBER_KEY" --create "$1" --json | jq -r .contractAddress; }
WSALT="$(deploy "$(jq -r .bytecode.object "$WORK/out/WrappedSALT.sol/WrappedSALT.json")")"
ROUTER_ARGS="$(cast abi-encode 'c(address,address)' "$MEMBER" "$MEMBER")"
ROUTER="$(deploy "$(jq -r .bytecode.object "$WORK/out/InferenceRouter.sol/InferenceRouter.json")${ROUTER_ARGS#0x}")"
MODEL_HASH="$(cast keccak hermes-planner-e2e-model)"
echo "wSALT $WSALT  InferenceRouter $ROUTER  model $MODEL_HASH"

echo "== provider $PROVIDER registers (0.01 wSALT per request, 100 SALT stake)"
cast send --rpc-url "$RPC" --private-key "$PROVIDER_KEY" "$ROUTER" \
  'registerProvider(string,uint256,bytes32[])' "http://127.0.0.1:${PROVIDER_PORT}/v1" 10000000000000000 "[$MODEL_HASH]" \
  --value 100ether >/dev/null
echo "== member $MEMBER wraps 1 SALT"
cast send --rpc-url "$RPC" --private-key "$MEMBER_KEY" "$WSALT" 'deposit()' --value 1ether >/dev/null

export CITRATE_X402_E2E_RPC="$RPC"
export CITRATE_X402_E2E_CHAIN_ID="$CHAIN_ID"
export CITRATE_X402_E2E_ASSET="$WSALT"
export CITRATE_X402_E2E_ROUTER="$ROUTER"
export CITRATE_X402_E2E_MODEL_HASH="$MODEL_HASH"
export CITRATE_X402_E2E_REQUEST="$WORK/request.json"
export CITRATE_X402_E2E_OUTCOME="$WORK/outcome.json"
export CARGO_INCREMENTAL=0

bal() { cast call --rpc-url "$RPC" "$WSALT" 'balanceOf(address)(uint256)' "$1"; }
echo "before: member $(bal "$MEMBER") provider $(bal "$PROVIDER")"

core_cargo() { (cd "$CORE_DIR" && if [ -n "${CORE_TARGET_DIR:-}" ]; then CARGO_TARGET_DIR="$CORE_TARGET_DIR" cargo "$@"; else cargo "$@"; fi); }
runtime_cargo() { (cd "$RUNTIME_DIR" && if [ -n "${RUNTIME_TARGET_DIR:-}" ]; then CARGO_TARGET_DIR="$RUNTIME_TARGET_DIR" cargo "$@"; else cargo "$@"; fi); }

# Build each test binary once and run copies, so a parallel build cannot swap them between legs.
test_exe() { jq -r 'select(.executable != null and .profile.test == true) | .executable' | tail -1; }
echo "== build the core and runtime test binaries"
CORE_EXE="$(core_cargo test -p citrate-core --lib --no-run --message-format=json | test_exe)"
RUNTIME_EXE="$(runtime_cargo test -p citrate-agent-escalation --test registry_tests --no-run --message-format=json | test_exe)"
cp "$CORE_EXE" "$WORK/core-tests"
cp "$RUNTIME_EXE" "$WORK/runtime-tests"

echo "== 1/3 core: quote, build, approve in the ceremony, verify"
(cd "$CORE_DIR/src-tauri" && "$WORK/core-tests" --ignored --exact --nocapture \
  escalation_registry::tests::anvil_e2e_registry_quote_and_approve)
echo "== 2/3 runtime: paid request, provider settles on chain, receipt"
(cd "$RUNTIME_DIR/agent-escalation" && "$WORK/runtime-tests" --ignored --exact \
  anvil_e2e_registry_payment_settles_on_chain)
echo "== 3/3 core: authorizationState confirms settlement"
(cd "$CORE_DIR/src-tauri" && "$WORK/core-tests" --ignored --exact \
  escalation_registry::tests::anvil_e2e_registry_settlement_is_confirmed)

echo "after:  member $(bal "$MEMBER") provider $(bal "$PROVIDER")"
echo "receipt: $(jq -c .receipt "$WORK/outcome.json")"
echo "PASS: registry escalation settled on anvil (chain id $CHAIN_ID)"
