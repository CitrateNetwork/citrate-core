#!/usr/bin/env bash
# HUP-S7.1 (federation F-4): rehearse runbook step 5 (scripts/sync-addresses.py) against a real
# DeployHupRegistries deploy on a local anvil fork of 40204. Nothing here sends a transaction to
# 40204: citrate-chain's scripts/ops/hup-redeploy-dryrun.sh forks the chain into its own anvil,
# deploys there with an impersonated sender, and writes the book and provenance tools' output to
# temp copies. While that fork is up it calls this script back (HUP_DRYRUN_POST_HOOK), which runs
# the sync against the temp book and the fork:
#
#   1. the redeployed book passes --check with code at every pin, CapsuleRegistry and
#      InferenceRouter included;
#   2. InferenceRouter moved to an address with no code fails closed;
#   3. CapsuleRegistry written as something that is not an address fails closed;
#   4. CapsuleRegistry absent is left out (optional), and the run still passes.
#
# The app's embedded book (src-tauri/addresses/40204.json) is never written: every run is --check.
#
# Usage: scripts/anvil-sync-addresses.sh [citrate-chain checkout]
#   default: ../citrate-chain next to this repo, or CITRATE_CHAIN. Needs forge, anvil, cast,
#   python3 and network access to the fork source (HUP_FORK_RPC, default https://rpc.citrate.ai).
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
sync="$repo/scripts/sync-addresses.py"

if [[ -n "${HUP_DRYRUN_RPC:-}" ]]; then
  # Hook mode: called by hup-redeploy-dryrun.sh while its fork is running.
  rpc="$HUP_DRYRUN_RPC"; book="$HUP_DRYRUN_BOOK"; gen="$HUP_DRYRUN_GENESIS"
  client="$(cast rpc --rpc-url "$rpc" web3_clientVersion 2>/dev/null | tr -d '"')"
  [[ "$client" == anvil/* ]] || { echo "sync rehearsal: $rpc is not an anvil ($client); refusing" >&2; exit 3; }
  work="$(mktemp -d "${TMPDIR:-/tmp}/sync-rehearsal.XXXXXX")"
  trap 'rm -rf "$work"' EXIT
  edit() { # <out> <python expression over b["contracts"] as c>
    python3 -c 'import json,sys; b=json.load(open(sys.argv[1])); c=b["contracts"]; exec(sys.argv[3]); json.dump(b, open(sys.argv[2], "w"), indent=2)' "$book" "$1" "$2"
  }
  ok()   { echo "  ok  $1"; }
  fail() { echo "sync rehearsal: FAIL $1" >&2; exit 6; }

  out="$(python3 "$sync" --book "$book" --genesis "$gen" --rpc "$rpc" --check --with-inference-router)" || fail "the redeployed book did not pass"
  echo "$out"
  grep -q "CapsuleRegistry" <<<"$out" && grep -q "InferenceRouter" <<<"$out" \
    || fail "CapsuleRegistry or InferenceRouter missing from the optional pins"
  for n in AgentSBT OrganizationSBT CapsuleRegistry AnchorRegistry BenchmarkRegistry SkillRegistry; do
    a="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["contracts"][sys.argv[2]])' "$book" "$n")"
    [[ "$(cast code --rpc-url "$rpc" "$a")" != "0x" ]] || fail "$n $a has no code on the fork"
  done
  ok "redeployed book passes with code at every pin (CapsuleRegistry, InferenceRouter included)"

  edit "$work/nocode.json" 'c["InferenceRouter"] = "0x000000000000000000000000000000000000dEaD"'
  if python3 "$sync" --book "$work/nocode.json" --genesis "$gen" --rpc "$rpc" --check 2>"$work/err"; then
    fail "an InferenceRouter pin with no code was accepted"
  fi
  grep -q "no code on the live chain at: InferenceRouter" "$work/err" || fail "wrong refusal: $(cat "$work/err")"
  ok "InferenceRouter with no code fails closed"

  edit "$work/malformed.json" 'c["CapsuleRegistry"] = "0x1234"'
  if python3 "$sync" --book "$work/malformed.json" --genesis "$gen" --rpc "$rpc" --check 2>"$work/err"; then
    fail "a malformed CapsuleRegistry entry was accepted"
  fi
  grep -q "contracts.CapsuleRegistry" "$work/err" || fail "wrong refusal: $(cat "$work/err")"
  ok "CapsuleRegistry that is not an address fails closed"

  edit "$work/absent.json" 'c.pop("CapsuleRegistry")'
  out="$(python3 "$sync" --book "$work/absent.json" --genesis "$gen" --rpc "$rpc" --check)" || fail "an absent optional pin failed the sync"
  grep -q "CapsuleRegistry" <<<"$out" && fail "an absent CapsuleRegistry was emitted"
  ok "absent CapsuleRegistry is left out"
  echo "sync rehearsal: PASS"
  exit 0
fi

chain="${1:-${CITRATE_CHAIN:-$repo/../citrate-chain}}"
dryrun="$chain/scripts/ops/hup-redeploy-dryrun.sh"
[[ -x "$dryrun" ]] || { echo "hup-redeploy-dryrun.sh not found under $chain" >&2; exit 2; }
for bin in forge anvil cast python3; do
  command -v "$bin" >/dev/null || { echo "$bin not installed" >&2; exit 2; }
done
HUP_DRYRUN_POST_HOOK="$repo/scripts/anvil-sync-addresses.sh" "$dryrun"
