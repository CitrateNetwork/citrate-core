#!/usr/bin/env bash
# HUP-S7.4 — the AgentSBT anvil test: build AgentSBT + OrganizationSBT from the citrate-chain
# source, then run the opt-in Rust test that deploys them on a local anvil and checks the member
# path (mintAgentAsMember, reroll 2026-10-05) through the app's readiness gather and calldata
# builder (src-tauri/src/agent_sbt_tests.rs). Against a chain checkout without member issuance it
# proves the app reports "after the network upgrade" and never falls back to an owner-only mint.
#
# Usage: scripts/anvil-agent-sbt.sh [path/to/citrate-chain/contracts]
#   default: ../citrate-chain/contracts next to this repo, or CITRATE_CHAIN_CONTRACTS.
# Needs forge + anvil (Foundry) on PATH. The artifacts go to a temp dir, never into either repo.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
chain="${1:-${CITRATE_CHAIN_CONTRACTS:-$repo/../citrate-chain/contracts}}"
[ -f "$chain/src/cit_agent/AgentSBT.sol" ] || { echo "AgentSBT.sol not found under $chain (pass the citrate-chain contracts dir)" >&2; exit 2; }
command -v forge >/dev/null || { echo "forge not installed" >&2; exit 2; }
command -v anvil >/dev/null || { echo "anvil not installed" >&2; exit 2; }
work="$(mktemp -d "${TMPDIR:-/tmp}/agent-sbt-anvil.XXXXXX")"
trap 'rm -rf "$work"' EXIT
( cd "$chain" && forge build --out "$work/out" --cache-path "$work/cache" src/cit_agent/AgentSBT.sol >"$work/forge.log" 2>&1 ) \
  || { cat "$work/forge.log" >&2; exit 1; }
echo "built AgentSBT + OrganizationSBT from $chain ($(git -C "$chain" rev-parse --short HEAD 2>/dev/null || echo 'no git'))"
cd "$repo/src-tauri"
CITRATE_AGENT_SBT_ARTIFACTS="$work/out" cargo test --lib agent_sbt::tests::anvil -- --ignored --nocapture
