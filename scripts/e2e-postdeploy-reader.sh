#!/usr/bin/env bash
# HUP-S6.6 + S6.7 end to end, on this machine only (nothing touches chain 40204):
#
#   1. render the hello-mint template (citrate-templates) into a work dir;
#   2. fetch the pinned Solidity dependencies and forge build the contract;
#   3. start a throwaway anvil (chain id 40204) and deploy the contract from anvil's unlocked
#      test account 0 (no key is used or stored; anvil signs for its own dev accounts);
#   4. optionally start a throwaway kubo (IPFS) node when `ipfs` is installed;
#   5. run the Rust post-deploy + reader test and the TypeScript reader test against them.
#
# The page is not npm-built here: a two-file stand-in for app/dist is written so the IPFS pin
# step has a site to add. Everything is removed on exit except the deps cache.
#
# usage: scripts/e2e-postdeploy-reader.sh --work DIR --deps-cache DIR
set -euo pipefail

CORE="$(cd "$(dirname "$0")/.." && pwd)"
WORK=""
CACHE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --work) WORK="$2"; shift 2 ;;
    --deps-cache) CACHE="$2"; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$WORK" ] && [ -n "$CACHE" ] || { echo "e2e: --work and --deps-cache are required" >&2; exit 2; }
for t in forge anvil cast jq; do command -v "$t" >/dev/null || { echo "e2e: $t is not installed" >&2; exit 2; }; done

# anvil's dev account 0 (unlocked on a local anvil).
OWNER=0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
PORT="${E2E_ANVIL_PORT:-18545}"
RPC="http://127.0.0.1:$PORT"
PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  rm -rf "$WORK"
}
trap cleanup EXIT
rm -rf "$WORK"; mkdir -p "$WORK" "$CACHE"

TARGET="${CARGO_TARGET_DIR:-$CORE/target}"
(cd "$CORE" && cargo build -q -p citrate-templates)
"$TARGET/debug/citrate-templates" render --root "$CORE/templates" --template hello-mint --tier T1 \
  --out "$WORK/project" --param "name=Lemon Drops" --param symbol=LEMON --param supply=500 \
  --param price=5000000000000000000 --param "owner=$OWNER" >/dev/null

for dep in $(jq -r '.deps | keys[]' "$CORE/templates/deps.lock.json"); do
  url=$(jq -r ".deps[\"$dep\"].url" "$CORE/templates/deps.lock.json")
  tag=$(jq -r ".deps[\"$dep\"].tag" "$CORE/templates/deps.lock.json")
  commit=$(jq -r ".deps[\"$dep\"].commit" "$CORE/templates/deps.lock.json")
  [ -d "$CACHE/$dep/.git" ] || git -c advice.detachedHead=false clone -q --depth 1 --branch "$tag" "$url" "$CACHE/$dep"
  [ "$(git -C "$CACHE/$dep" rev-parse HEAD)" = "$commit" ] || { echo "e2e: $dep is not at $commit" >&2; exit 1; }
  mkdir -p "$WORK/project/contracts/lib"
  cp -R "$CACHE/$dep" "$WORK/project/contracts/lib/$dep"
done
(cd "$WORK/project/contracts" && forge build --offline >/dev/null)
CONTRACT=$(jq -r '.params.contract' "$WORK/project/citrate-template.lock.json")

anvil --chain-id 40204 --port "$PORT" --silent &
PIDS+=($!)
for _ in $(seq 1 50); do cast block-number --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.2; done

OUT=$(cd "$WORK/project/contracts" && forge create "src/Token.sol:$CONTRACT" --rpc-url "$RPC" \
  --unlocked --from "$OWNER" --broadcast --json)
ADDRESS=$(echo "$OUT" | jq -r '.deployedTo')
TX=$(echo "$OUT" | jq -r '.transactionHash')
echo "e2e: deployed $CONTRACT at $ADDRESS (tx $TX) on anvil"

mkdir -p "$WORK/project/app/dist/assets"
printf '<!doctype html><title>Lemon Drops</title><script src="./assets/app.js"></script>\n' > "$WORK/project/app/dist/index.html"
printf 'console.log("hello mint");\n' > "$WORK/project/app/dist/assets/app.js"

KUBO_API=""
if command -v ipfs >/dev/null; then
  export IPFS_PATH="$WORK/ipfs"
  ipfs init --profile test >/dev/null
  ipfs config Addresses.API /ip4/127.0.0.1/tcp/15901
  ipfs config Addresses.Gateway /ip4/127.0.0.1/tcp/15902
  ipfs daemon --offline >/dev/null 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 100); do curl -s -X POST http://127.0.0.1:15901/api/v0/version >/dev/null 2>&1 && break; sleep 0.2; done
  KUBO_API=http://127.0.0.1:15901
else
  echo "e2e: ipfs is not installed; the pin step is not exercised"
fi

export CITRATE_E2E_RPC="$RPC" CITRATE_E2E_PROJECT="$WORK/project" CITRATE_E2E_ADDRESS="$ADDRESS" \
  CITRATE_E2E_DEPLOY_TX="$TX" CITRATE_E2E_CONTRACT="$CONTRACT"
[ -n "$KUBO_API" ] && export CITRATE_E2E_KUBO_API="$KUBO_API"
(cd "$CORE/src-tauri" && cargo test --lib e2e_anvil -- --nocapture)
(cd "$CORE" && npx vitest run src/contractReader/anvil.e2e.test.ts)
echo "e2e: OK"
