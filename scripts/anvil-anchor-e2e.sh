#!/usr/bin/env bash
# HUP-S7.3 + S7.5: the anvil rehearsal of the nightly anchor, inclusion proofs and opt-in
# benchmark sharing (gate g4-anchor, local evidence). Nothing here touches 40204.
#
#   1. builds AnchorRegistry + BenchmarkRegistry from the citrate-chain source (scratch dir only);
#   2. builds the real Hermes sidecar from citrate-agent-runtime and copies the binary aside;
#   3. seeds a Hermes folder with real decision records + metering turns through the runtime's own
#      writers (agent-sidecar/tests/anchor_e2e_seed.rs);
#   4. runs the opt-in core test anchor_proof::e2e_tests (src-tauri/src/anchor_e2e_tests.rs): a
#      private anvil on chain id 40204, the sidecar started by HermesManager, the nightly tick, the
#      anchor ceremony, the in-flight record, the settle step, core's own proof check for every
#      record, and the benchmark payload rechecked by core and read back from the registry.
#
# With CITRATE_ANCHOR_E2E_NEXT_REF set (a ref in the chain repo, for example
# origin/hup/n5-chain-redeploy), step 3-4 run a second time against that ref's AnchorRegistry and
# BenchmarkRegistry (the versions the next redeploy ships).
#
# Usage: scripts/anvil-anchor-e2e.sh [citrate-chain/contracts] [citrate-agent-runtime]
#   defaults: ../citrate-chain/contracts and ../citrate-agent-runtime next to this repo, or
#   CITRATE_CHAIN_CONTRACTS / CITRATE_AGENT_RUNTIME. Needs forge + anvil (Foundry) and cargo.
#   Build dirs: CITRATE_RUNTIME_TARGET_DIR / CITRATE_CORE_TARGET_DIR (default: each repo's target/).
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
chain="${1:-${CITRATE_CHAIN_CONTRACTS:-$repo/../citrate-chain/contracts}}"
runtime="${2:-${CITRATE_AGENT_RUNTIME:-$repo/../citrate-agent-runtime}}"
[ -f "$chain/src/cit_agent/AnchorRegistry.sol" ] || { echo "AnchorRegistry.sol not found under $chain" >&2; exit 2; }
[ -f "$runtime/agent-sidecar/Cargo.toml" ] || { echo "agent-sidecar not found under $runtime" >&2; exit 2; }
command -v forge >/dev/null || { echo "forge not installed" >&2; exit 2; }
command -v anvil >/dev/null || { echo "anvil not installed" >&2; exit 2; }

rt_target="${CITRATE_RUNTIME_TARGET_DIR:-$runtime/target}"
core_target="${CITRATE_CORE_TARGET_DIR:-$repo/target}"
work="$(mktemp -d "${TMPDIR:-/tmp}/anchor-e2e.XXXXXX")"
trap 'rm -rf "$work"' EXIT

build_registries() { # <contracts project dir> <out dir>
  ( cd "$1" && forge build --out "$2" --cache-path "$2.cache" \
      src/cit_agent/AnchorRegistry.sol src/cit_agent/BenchmarkRegistry.sol >"$2.log" 2>&1 ) \
    || { cat "$2.log" >&2; exit 1; }
}

build_registries "$chain" "$work/out-current"
echo "built AnchorRegistry + BenchmarkRegistry from $chain ($(git -C "$chain" rev-parse --short HEAD 2>/dev/null || echo 'no git'))"

runs=("current:$work/out-current")
if [ -n "${CITRATE_ANCHOR_E2E_NEXT_REF:-}" ]; then
  next="$work/next"
  mkdir -p "$next/src/cit_agent"
  cp "$chain/foundry.toml" "$next/"
  for f in AnchorRegistry BenchmarkRegistry; do
    git -C "$chain" show "$CITRATE_ANCHOR_E2E_NEXT_REF:contracts/src/cit_agent/$f.sol" >"$next/src/cit_agent/$f.sol"
  done
  build_registries "$next" "$work/out-next"
  echo "built the registries of $CITRATE_ANCHOR_E2E_NEXT_REF ($(git -C "$chain" rev-parse --short "$CITRATE_ANCHOR_E2E_NEXT_REF"))"
  runs+=("next:$work/out-next")
fi

# The sidecar binary: built, then copied aside so a later build in a shared target dir cannot
# swap it underneath the run.
( cd "$runtime" && CARGO_TARGET_DIR="$rt_target" cargo build -p agent-sidecar --bin citrate-agent-sidecar >"$work/sidecar-build.log" 2>&1 ) \
  || { tail -40 "$work/sidecar-build.log" >&2; exit 1; }
cp "$rt_target/debug/citrate-agent-sidecar" "$work/citrate-agent-sidecar"
echo "built the sidecar from $runtime ($(git -C "$runtime" rev-parse --short HEAD 2>/dev/null || echo 'no git'))"

# Two free loopback ports per run (anvil, sidecar control), so nothing else listening is reused.
free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'; }

for run in "${runs[@]}"; do
  label="${run%%:*}"; out="${run#*:}"
  hermes="$work/hermes-$label"
  ( cd "$runtime" && CARGO_TARGET_DIR="$rt_target" CITRATE_ANCHOR_E2E_HERMES_DIR="$hermes" \
      cargo test -p agent-sidecar --test anchor_e2e_seed -- --ignored --nocapture ) >"$work/seed-$label.log" 2>&1 \
    || { tail -40 "$work/seed-$label.log" >&2; exit 1; }
  grep '^E2E_SEED' "$work/seed-$label.log"
  ( cd "$repo/src-tauri" && \
    CARGO_TARGET_DIR="$core_target" \
    CITRATE_ANCHOR_E2E_ARTIFACTS="$out" \
    CITRATE_ANCHOR_E2E_SIDECAR="$work/citrate-agent-sidecar" \
    CITRATE_ANCHOR_E2E_HERMES_DIR="$hermes" \
    CITRATE_ANCHOR_E2E_WORK="$work/core-$label" \
    CITRATE_ANCHOR_E2E_LABEL="$label" \
    CITRATE_ANCHOR_E2E_ANVIL_PORT="$(free_port)" \
    CITRATE_ANCHOR_E2E_CONTROL_PORT="$(free_port)" \
    cargo test --lib anchor_proof::e2e_tests -- --ignored --nocapture ) >"$work/core-$label.log" 2>&1 \
    || { tail -60 "$work/core-$label.log" >&2; exit 1; }
  grep -E '^E2E_RESULT|^test |test result' "$work/core-$label.log"
done
echo "anchor e2e: PASS (${#runs[@]} registry version(s))"
