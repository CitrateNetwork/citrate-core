#!/usr/bin/env bash
# HUP-S6.3 → S6.4 proof run (retro A27): a freshly rendered template through the D-4 deploy gate
# with the real toolchain.
#
#   1. render a template (default erc20) at tier T0 with the citrate-templates renderer;
#   2. copy the pinned libraries in from --deps-cache (checked against templates/deps.lock.json);
#   3. optionally inject a bug (--inject-selfdestruct: anyone can selfdestruct the token);
#   4. run forge, slither, aderyn and medusa with exactly the argv the Hermes toolchain tools use
#      (citrate-agent-runtime agent-sidecar/src/toolchain.rs) and keep each raw stdout;
#   5. optionally (--anvil) deploy the built bytecode on a throwaway local anvil (chain id 40204,
#      auto-impersonated sender, no key) and keep the receipt and the creation tx input: a local
#      dry run, not a fork of the live chain (the Citrate-aware fork is lane CH-fork's);
#   6. evaluate everything with the production bridge and gate
#      (`deploy_gate_toolchain::tests::recorded_proof_run_when_present`), which writes
#      gate-record.json into the run folder and checks the expected verdict.
#
# Nothing is signed or sent to chain 40204. Tools that are missing are reported, and their gate
# items fail (NOT READY).
#
# usage: forge-gate-proof.sh --work DIR --deps-cache DIR [--template erc20] [--inject-selfdestruct]
#                            [--anvil] [--expect READY|NOT_READY]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK=""
CACHE=""
TEMPLATE="erc20"
INJECT=0
ANVIL=0
EXPECT="NOT_READY"
while [ $# -gt 0 ]; do
  case "$1" in
    --work) WORK="$2"; shift 2 ;;
    --deps-cache) CACHE="$2"; shift 2 ;;
    --template) TEMPLATE="$2"; shift 2 ;;
    --inject-selfdestruct) INJECT=1; shift ;;
    --anvil) ANVIL=1; shift ;;
    --expect) EXPECT="$2"; shift 2 ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "forge-gate-proof: unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$WORK" ] && [ -n "$CACHE" ] || { echo "forge-gate-proof: --work and --deps-cache are required" >&2; exit 2; }
command -v jq >/dev/null || { echo "forge-gate-proof: jq is not installed" >&2; exit 2; }

TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
(cd "$ROOT" && cargo build -q -p citrate-templates)
BIN="$TARGET/debug/citrate-templates"

mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd -P)"
PROJ="$WORK/project"
RUN="$WORK/run"
rm -rf "$PROJ" "$RUN"
mkdir -p "$RUN"

OWNER="0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
"$BIN" render --root "$ROOT/templates" --template "$TEMPLATE" --tier T0 --out "$PROJ" \
  --param name="Lemon Drops" --param symbol=LEMON --param owner="$OWNER" > "$RUN/render.json"
CONTRACT="$(jq -r '.params.contract' "$RUN/render.json")"

for dep in $(jq -r '.deps | keys[]' "$ROOT/templates/deps.lock.json"); do
  commit=$(jq -r ".deps[\"$dep\"].commit" "$ROOT/templates/deps.lock.json")
  got=$(git -C "$CACHE/$dep" rev-parse HEAD)
  [ "$got" = "$commit" ] || { echo "forge-gate-proof: $dep is at $got, the lock pins $commit" >&2; exit 1; }
  mkdir -p "$PROJ/lib"
  cp -R "$CACHE/$dep" "$PROJ/lib/$dep"
  rm -rf "$PROJ/lib/$dep/.git"
done

if [ "$INJECT" = 1 ]; then
  # Injected bug for the US-6.2 scenario: an unprotected selfdestruct.
  perl -0pi -e 's/(_mint\(INITIAL_HOLDER, INITIAL_SUPPLY\);\n    \})/$1\n\n    function shutdown() external {\n        selfdestruct(payable(msg.sender));\n    }/' "$PROJ/src/Token.sol"
  grep -q "function shutdown" "$PROJ/src/Token.sol" || { echo "forge-gate-proof: could not inject the bug" >&2; exit 1; }
fi

export FOUNDRY_OFFLINE=true
cd "$PROJ"
now_ms() { python3 -c 'import time; print(int(time.time() * 1000))'; }
run_tool() {
  local name="$1"; shift
  local prog="$1"
  if ! command -v "$prog" >/dev/null; then
    echo "$name: $prog is not installed" | tee -a "$RUN/timings.txt"
    : > "$RUN/$name.out"
    return
  fi
  local s e rc
  s=$(now_ms)
  set +e
  "$@" > "$RUN/$name.out" 2> "$RUN/$name.err"
  rc=$?
  set -e
  e=$(now_ms)
  echo "$name rc=$rc ms=$((e - s))" | tee -a "$RUN/timings.txt"
}
run_tool forge forge test --json
run_tool slither slither . --sarif - --exclude-dependencies --disable-color --compile-force-framework foundry
run_tool aderyn aderyn . --output aderyn-report.sarif --stdout --skip-update-check
rm -rf medusa-corpus
run_tool medusa medusa fuzz --no-color --test-limit 10000 --timeout 600
cp medusa-corpus/coverage/lcov.info "$RUN/medusa.lcov" 2>/dev/null || : > "$RUN/medusa.lcov"
{
  forge --version 2>/dev/null | head -1 || true
  slither --version 2>/dev/null || true
  aderyn --version 2>/dev/null || true
  medusa --version 2>/dev/null || true
} > "$RUN/versions.txt"

# The proof's own binding of the four runs to one state of the sources (in the app the sidecar
# computes it before and after each run).
find src test foundry.toml remappings.txt medusa.json -type f 2>/dev/null | LC_ALL=C sort | xargs shasum -a 256 | shasum -a 256 | cut -d' ' -f1 > "$RUN/sources.txt"
echo "$PROJ" > "$RUN/project.txt"
echo "Token.sol/$CONTRACT.json" > "$RUN/artifact.txt"
echo "$EXPECT" > "$RUN/expect.txt"

if [ "$ANVIL" = 1 ]; then
  PORT=18645
  anvil --port "$PORT" --chain-id 40204 --auto-impersonate --silent > "$RUN/anvil.log" 2>&1 &
  APID=$!
  trap 'kill $APID 2>/dev/null || true' EXIT
  for _ in $(seq 1 50); do cast chain-id --rpc-url "http://127.0.0.1:$PORT" >/dev/null 2>&1 && break; sleep 0.2; done
  CODE="$(jq -r '.bytecode.object' "out/Token.sol/$CONTRACT.json")"
  SENDER="0x00000000000000000000000000000000000c17a7"
  cast rpc --rpc-url "http://127.0.0.1:$PORT" anvil_setBalance "$SENDER" 0x56BC75E2D63100000 >/dev/null
  cast send --rpc-url "http://127.0.0.1:$PORT" --unlocked --from "$SENDER" --create "$CODE" --json > "$RUN/fork-receipt.json"
  TX="$(jq -r '.transactionHash' "$RUN/fork-receipt.json")"
  cast tx --rpc-url "http://127.0.0.1:$PORT" "$TX" --json | jq -r '.input' > "$RUN/fork-tx-input.txt"
  kill "$APID" 2>/dev/null || true
fi

cd "$ROOT"
CITRATE_FORGE_PROOF_DIR="$RUN" cargo test -q -p citrate-core --lib deploy_gate_toolchain::tests::recorded_proof_run_when_present
jq '{verdict: .record.verdict, initcodeHash: .record.initcodeHash, failing: [.record.items[] | select(.pass == false) | {label, reason}], medusa: .medusa}' "$RUN/gate-record.json"
