#!/usr/bin/env bash
# HUP-S6 US-6.1 (local half), gates g3-gate + g3-e2e (local), HUP-S11.1 (macOS part).
#
# Runs src-tauri/e2e/hello_mint_local.feature on this machine:
#   interview answers -> hello-mint template render -> forge test -> slither -> aderyn -> medusa
#   -> anvil fork dry run -> D-4 deploy gate -> contract_deploy -> SignatureCeremony approve
#   (a fresh test vault signs a real EIP-155 tx) -> local chain deploy -> code check + verifier
#   input -> site switch -> test mint through a ceremony -> Vercel export [-> page build + IPFS].
# Plus: no aderyn/medusa = NOT READY, and an injected unbounded mint = NOT READY.
#
# Nothing touches the live chain 40204. The "chain" is a throwaway anvil with chain id 40204 and
# the dry run uses an anvil fork of it. No fixed key is used: the member wallet is created fresh.
#
# Not covered here: the Hermes interview turn (runtime interview tests), the Browser pop-out,
# the faucet (HUP-S6.5, not built; the local chain funds the wallet), CitrateScan verification
# (live explorer), and a member clicking Approve in the packaged app.
#
# usage: scripts/e2e-hello-mint.sh --work DIR --deps-cache DIR
#          [--with-bundle-tools DIR]   fetch aderyn + medusa from the URLs in
#                                      components/toolchain-bundle.json into DIR, check each
#                                      archive's SHA-256 against the measured value, and use them
#                                      (test-only verifier config; only platforms whose archives
#                                      are measured, today macOS arm64). Without it the READY and
#                                      injected-bug scenarios fail at their first step.
#          [--with-page-build]         npm install + build the page and the Vercel export, and pin
#                                      the page to a throwaway offline IPFS node when ipfs exists.
set -euo pipefail

CORE="$(cd "$(dirname "$0")/.." && pwd)"
WORK=""
CACHE=""
TOOLS=""
PAGE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --work) WORK="$2"; shift 2 ;;
    --deps-cache) CACHE="$2"; shift 2 ;;
    --with-bundle-tools) TOOLS="$2"; shift 2 ;;
    --with-page-build) PAGE=1; shift ;;
    -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
    *) echo "e2e: unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$WORK" ] && [ -n "$CACHE" ] || { echo "e2e: --work and --deps-cache are required" >&2; exit 2; }
for t in forge anvil cast jq git slither cargo shasum tar; do
  command -v "$t" >/dev/null || { echo "e2e: $t is not installed" >&2; exit 2; }
done
[ "$PAGE" = 0 ] || command -v npm >/dev/null || { echo "e2e: --with-page-build needs npm" >&2; exit 2; }

CHAIN_PORT="${E2E_CHAIN_PORT:-18645}"
FORK_PORT="${E2E_FORK_PORT:-18646}"
PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$WORK" || true
}
trap cleanup EXIT
rm -rf "$WORK"; mkdir -p "$WORK" "$CACHE"

# 1. The pinned Solidity dependencies, each checked against its locked commit.
LOCK="$CORE/templates/deps.lock.json"
for dep in $(jq -r '.deps | keys[]' "$LOCK"); do
  url=$(jq -r ".deps[\"$dep\"].url" "$LOCK")
  tag=$(jq -r ".deps[\"$dep\"].tag" "$LOCK")
  commit=$(jq -r ".deps[\"$dep\"].commit" "$LOCK")
  [ -d "$CACHE/$dep/.git" ] || git -c advice.detachedHead=false clone -q --depth 1 --branch "$tag" "$url" "$CACHE/$dep"
  [ "$(git -C "$CACHE/$dep" rev-parse HEAD)" = "$commit" ] || { echo "e2e: $dep is not at $commit" >&2; exit 1; }
done

# 2. The test-only verifier config: aderyn + medusa from the measured bundle archives.
ADERYN_BIN=""
MEDUSA_BIN=""
if [ -n "$TOOLS" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) PLATFORM=macos-arm64 ;;
    Darwin-x86_64) PLATFORM=macos-x64 ;;
    Linux-x86_64) PLATFORM=linux-x64 ;;
    *) echo "e2e: --with-bundle-tools: no bundle platform for $(uname -s)-$(uname -m)" >&2; exit 2 ;;
  esac
  BUNDLE="$CORE/components/toolchain-bundle.json"
  mkdir -p "$TOOLS"
  fetch_tool() { # name -> prints the entrypoint path
    local name="$1" art url sha file dir entry
    art=$(jq -c ".tools[] | select(.name == \"$name\") | .artifacts[\"$PLATFORM\"]" "$BUNDLE")
    [ "$(echo "$art" | jq -r .status)" = "measured" ] || {
      echo "e2e: $name has no measured $PLATFORM archive in components/toolchain-bundle.json (measure and record its SHA-256 first)" >&2; return 1; }
    url=$(echo "$art" | jq -r .url)
    sha=$(echo "$art" | jq -r .sha256)
    file="$TOOLS/$(basename "$url")"
    [ -f "$file" ] || curl -fsSL -o "$file" "$url"
    [ "$(shasum -a 256 "$file" | cut -d' ' -f1)" = "$sha" ] || { echo "e2e: $name archive SHA-256 does not match the bundle" >&2; return 1; }
    dir="$TOOLS/$name"
    rm -rf "$dir"; mkdir -p "$dir"
    tar -xf "$file" -C "$dir"
    entry=$(echo "$art" | jq -r '.entrypoints[0] // empty')
    [ -n "$entry" ] || entry=$(jq -r ".tools[] | select(.name == \"$name\") | .entrypoints[0]" "$BUNDLE")
    [ -x "$dir/$entry" ] || { echo "e2e: $name entrypoint $entry missing" >&2; return 1; }
    echo "$dir/$entry"
  }
  ADERYN_BIN=$(fetch_tool aderyn)
  MEDUSA_BIN=$(fetch_tool medusa)
fi

# 3. A PATH with the tools this machine provides and nothing else, so "aderyn and medusa are
#    not installed" is a property of the run, not of whatever is on the developer's PATH.
SAFE_PATH="/usr/bin:/bin:/usr/sbin:/sbin"
for t in forge anvil cast slither git jq cargo npm node ipfs; do
  p=$(command -v "$t" 2>/dev/null || true)
  [ -n "$p" ] && SAFE_PATH="$(dirname "$p"):$SAFE_PATH"
done
for t in aderyn medusa; do
  if PATH="$SAFE_PATH" command -v "$t" >/dev/null 2>&1; then
    echo "e2e: $t is installed beside the other tools ($(PATH="$SAFE_PATH" command -v "$t")); the missing-tool scenario needs it absent" >&2
    exit 2
  fi
done

# 4. The local chain (chain id 40204) and an anvil fork of it for the dry run.
anvil --chain-id 40204 --port "$CHAIN_PORT" --silent &
PIDS+=($!)
for _ in $(seq 1 50); do cast block-number --rpc-url "http://127.0.0.1:$CHAIN_PORT" >/dev/null 2>&1 && break; sleep 0.2; done
anvil --fork-url "http://127.0.0.1:$CHAIN_PORT" --chain-id 40204 --port "$FORK_PORT" --silent &
PIDS+=($!)
for _ in $(seq 1 50); do cast block-number --rpc-url "http://127.0.0.1:$FORK_PORT" >/dev/null 2>&1 && break; sleep 0.2; done

# 5. Optional: a throwaway offline IPFS node for the site pin.
KUBO_API=""
if [ "$PAGE" = 1 ] && command -v ipfs >/dev/null; then
  export IPFS_PATH="$WORK/ipfs"
  ipfs init --profile test >/dev/null
  ipfs config Addresses.API /ip4/127.0.0.1/tcp/15911
  ipfs config Addresses.Gateway /ip4/127.0.0.1/tcp/15912
  ipfs daemon --offline >/dev/null 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 100); do curl -s -X POST http://127.0.0.1:15911/api/v0/version >/dev/null 2>&1 && break; sleep 0.2; done
  KUBO_API=http://127.0.0.1:15911
fi

export CITRATE_E2E_HM_CHAIN_RPC="http://127.0.0.1:$CHAIN_PORT"
export CITRATE_E2E_HM_FORK_RPC="http://127.0.0.1:$FORK_PORT"
export CITRATE_E2E_HM_WORK="$WORK/run"
export CITRATE_E2E_HM_DEPS="$CACHE"
export CITRATE_E2E_HM_PAGE_BUILD="$PAGE"
[ -n "$ADERYN_BIN" ] && export CITRATE_E2E_HM_TEST_ADERYN_BIN="$ADERYN_BIN"
[ -n "$MEDUSA_BIN" ] && export CITRATE_E2E_HM_TEST_MEDUSA_BIN="$MEDUSA_BIN"
[ -n "$KUBO_API" ] && export CITRATE_E2E_HM_KUBO_API="$KUBO_API"
mkdir -p "$CITRATE_E2E_HM_WORK"

# E2E_TEST_BIN: an already built `cargo test --lib --no-run` binary of this checkout (skips the
# build, for a shared target directory that another build holds locked).
if [ -n "${E2E_TEST_BIN:-}" ]; then
  (cd "$CORE/src-tauri" && PATH="$SAFE_PATH" "$E2E_TEST_BIN" hello_mint_e2e --nocapture --test-threads=1)
else
  CARGO_BIN="$(command -v cargo)"
  (cd "$CORE/src-tauri" && PATH="$SAFE_PATH" "$CARGO_BIN" test --lib hello_mint_e2e -- --nocapture --test-threads=1)
fi
echo "e2e: hello mint OK"
