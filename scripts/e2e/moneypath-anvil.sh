#!/usr/bin/env bash
# G7 (0.5.2 RC): citrate-core's money paths, end to end, on the genesis contract set.
#
# On a throwaway anvil with chain id 40204 (nothing touches the live chain), this script:
#
#   1. checks out citrate-chain at REF in a temporary worktree (default: the head of
#      citrate-chain#331, the top of the genesis stack #320 -> #324 -> #331);
#   2. deploys the genesis contract set with that ref's own ceremony scripts, as
#      `forge script --broadcast`, exactly as citrate-chain's scripts/ci/genesis-pipeline-anvil.sh
#      does: DeployGenesisGovernance -> DeployDormantLedgers -> DeployGenesisMembership (Track M:
#      CitrateMemberSBT, MemberBond, MembershipStakeVault behind its proxy). The ValidatorRegistry
#      is not in the ceremony books: like the pipeline it is deployed from the same ref, governed
#      by the GOVERNANCE timelock, with minStake = the 32,000 SALT grant;
#   3. deploys LiquidStakingPool from the same ref (governed by the GOVERNANCE timelock). It is NOT
#      in the genesis set (C2 genesis spec: retired at genesis); it is here only because core still
#      ships the C1-W withdrawal read, which reads it;
#   4. installs a test stand-in for the node's 0x0120 ed25519-verify precompile (anvil has none):
#      it answers 1 only for inputs the Rust test whitelisted after verifying the signature with
#      ed25519-dalek, so the registry's own digest must equal the one core signed;
#   5. runs the ignored Rust test `moneypath_e2e_tests::anvil_moneypath_genesis_set`, which drives
#      citrate-core's real money code (grant-status read, activation read, bond-activation intent,
#      the SignatureCeremony signing EIP-155 transactions from a test vault, the C1-W paged
#      withdrawal scan) and compares every step with the goldens in
#      src-tauri/tests/fixtures/moneypath/.
#
# Keys: anvil's public test mnemonic only. The member wallet is that mnemonic imported into a
# throwaway in-memory vault; the deployer and grantor are its accounts 1 and 2. No real key is read.
#
# usage: scripts/e2e/moneypath-anvil.sh [--chain DIR] [--ref REF] [--update] [--keep]
#   --chain   a citrate-chain git checkout (default: $CITRATE_CHAIN_DIR or ../citrate-chain)
#   --ref     the citrate-chain commit to deploy (default: $CITRATE_E2E_MP_REF, else the head of
#             citrate-chain#331 read with `gh pr view 331`)
#   --update  rewrite the goldens from this run instead of comparing (review the diff!)
#   --keep    keep the temporary worktree, forge output and anvil log
# env: ANVIL_PORT (default 18547), CARGO_TARGET_DIR (honoured).
# Needs anvil, forge, cast, git, cargo (and gh unless --ref or CITRATE_E2E_MP_REF is given).
set -euo pipefail

CORE="$(cd "$(dirname "$0")/../.." && pwd)"
CHAIN="${CITRATE_CHAIN_DIR:-$CORE/../citrate-chain}"
REF="${CITRATE_E2E_MP_REF:-}"
UPDATE=0
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --chain) CHAIN="$2"; shift 2 ;;
    --ref) REF="$2"; shift 2 ;;
    --update) UPDATE=1; shift ;;
    --keep) KEEP=1; shift ;;
    -h|--help) sed -n '2,38p' "$0"; exit 0 ;;
    *) echo "moneypath-anvil: unknown argument: $1" >&2; exit 2 ;;
  esac
done

for t in anvil forge cast git cargo; do
  command -v "$t" >/dev/null || { echo "moneypath-anvil: $t is not installed" >&2; exit 2; }
done
git -C "$CHAIN" rev-parse --git-dir >/dev/null 2>&1 || { echo "moneypath-anvil: $CHAIN is not a citrate-chain git checkout (--chain)" >&2; exit 2; }
if [ -z "$REF" ]; then
  command -v gh >/dev/null || { echo "moneypath-anvil: gh is not installed; pass --ref" >&2; exit 2; }
  REF="$(gh pr view 331 -R CitrateNetwork/citrate-chain --json headRefOid -q .headRefOid)"
fi
case "$REF" in
  *[!0-9a-fA-F]* | "") echo "moneypath-anvil: --ref must be a commit sha, got '$REF'" >&2; exit 2 ;;
esac

PORT="${ANVIL_PORT:-18547}"
RPC="http://127.0.0.1:${PORT}"
if (exec 3<>"/dev/tcp/127.0.0.1/${PORT}") 2>/dev/null; then
  echo "moneypath-anvil: port ${PORT} is in use; set ANVIL_PORT" >&2
  exit 2
fi
MNEMONIC="test test test test test test test test test test test junk" # anvil's public default
ED25519_PRECOMPILE=0x0000000000000000000000000000000000000120
# CALLDATACOPY the input, return SLOAD(keccak256(input)) as one word: 1 only for whitelisted inputs.
ED25519_STANDIN=0x366000600037366000205460005260206000f3

WORK="$(mktemp -d "${TMPDIR:-/tmp}/citrate-moneypath.XXXXXX")"
WT="$WORK/chain"
ANVIL_PID=""
cleanup() {
  if [ -n "$ANVIL_PID" ]; then kill "$ANVIL_PID" 2>/dev/null || true; wait "$ANVIL_PID" 2>/dev/null || true; fi
  if [ "$KEEP" = 1 ]; then
    echo "moneypath-anvil: kept $WORK (remove the chain worktree with: git -C $CHAIN worktree remove --force $WT)"
  else
    git -C "$CHAIN" worktree remove --force "$WT" >/dev/null 2>&1 || true
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT

echo "==> citrate-chain @ $REF (worktree in $WT)"
git -C "$CHAIN" cat-file -e "$REF^{commit}" 2>/dev/null || git -C "$CHAIN" fetch -q origin "$REF"
git -C "$CHAIN" worktree add -q --detach "$WT" "$REF"
git -C "$WT" submodule update -q --init contracts/lib/openzeppelin-contracts contracts/lib/openzeppelin-contracts-upgradeable
for f in script/DeployGenesisGovernance.s.sol script/DeployDormantLedgers.s.sol script/DeployGenesisMembership.s.sol \
  src/core_membership/MemberBond.sol src/ValidatorRegistry.sol src/LiquidStakingPool.sol; do
  [ -f "$WT/contracts/$f" ] || { echo "moneypath-anvil: $f is missing at $REF: not the genesis contract set" >&2; exit 2; }
done

echo "==> anvil (chain id 40204) on $RPC"
# --prune-history: the flows mine ~60,000 blocks (epochs, the exit lock, the pool's withdrawal
# delay); keeping every block's state makes anvil_mine superlinear. Logs and `latest` reads stay.
anvil --chain-id 40204 --port "$PORT" --mnemonic "$MNEMONIC" --balance 1000000 --prune-history --silent >"$WORK/anvil.log" 2>&1 &
ANVIL_PID=$!
for _ in $(seq 1 50); do cast chain-id --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.2; done
[ "$(cast chain-id --rpc-url "$RPC")" = 40204 ] || { echo "moneypath-anvil: anvil not up on 40204" >&2; exit 1; }
# Past citrate-core's C1-W scan start (staking::LIQUID_STAKING_POOL_DEPLOY_BLOCK = 427, the r1005
# pool's deploy block), so the pool deployed below is inside the range the scan reads.
cast rpc --rpc-url "$RPC" anvil_mine 0x1f4 >/dev/null

KEY="$(cast wallet private-key "$MNEMONIC" 1)"
DEPLOYER="$(cast wallet address --private-key "$KEY")"
GRANTOR="$(cast wallet address --private-key "$(cast wallet private-key "$MNEMONIC" 2)")"
export DEPLOYER_ADDRESS="$DEPLOYER" FOUNDRY_DISABLE_NIGHTLY_WARNING=1
OUT_REL="genesis/anvil-moneypath" # under foundry.toml's read-write ./genesis
OUT="$WT/contracts/$OUT_REL"
mkdir -p "$OUT"
# Council seats: placeholder owner addresses (nothing here votes), the pipeline's genesis floors.
export GOV_OWNERS=0x000000000000000000000000000000000000A001,0x000000000000000000000000000000000000A002,0x000000000000000000000000000000000000A003,0x000000000000000000000000000000000000A004,0x000000000000000000000000000000000000A005
export GOV_THRESHOLD=3 GOV_MIN_DELAY=172800
export GRD_OWNERS=0x000000000000000000000000000000000000B001,0x000000000000000000000000000000000000B002,0x000000000000000000000000000000000000B003
export GRD_THRESHOLD=2 GRD_MIN_DELAY=3600
export GENESIS_GOVERNANCE_OUT="$OUT_REL/governance.json" DORMANT_LEDGERS_OUT="$OUT_REL/dormant.json"
export GENESIS_GOVERNANCE_BOOK="$OUT_REL/governance.json" GENESIS_MEMBERSHIP_OUT="$OUT_REL/membership.json"

cd "$WT/contracts"
run_script() {
  echo "==> forge script $1 --broadcast"
  forge script "$1" --rpc-url "$RPC" --broadcast --private-key "$KEY" --slow -q >"$WORK/$(basename "$1").log" 2>&1 ||
    { tail -40 "$WORK/$(basename "$1").log" >&2; echo "moneypath-anvil: $1 failed" >&2; exit 1; }
}
create() { # <contract> <constructor args...>
  local contract="$1"
  shift
  forge create "$contract" --rpc-url "$RPC" --private-key "$KEY" --broadcast --json --constructor-args "$@" |
    grep -o '"deployedTo": *"0x[0-9a-fA-F]*"' | grep -o '0x[0-9a-fA-F]*'
}
field() { sed -n "s/.*\"$2\":\"\(0x[0-9a-fA-F]*\)\".*/\1/p" "$1"; }

run_script script/DeployGenesisGovernance.s.sol
# PatronageLedger's KYC registry and membership: the pipeline's stand-ins (not in the gated books).
KYC="$(create src/KYCRegistry.sol:KYCRegistry 0x0000000000000000000000000000000000000000 "$DEPLOYER")"
PATRONAGE_MEMBERSHIP="$(create src/coop/MembershipSBT.sol:MembershipSBT "$KYC" "[]" "$DEPLOYER" "$DEPLOYER")"
[ -n "$KYC" ] && [ -n "$PATRONAGE_MEMBERSHIP" ] || { echo "moneypath-anvil: stand-in deploy failed" >&2; exit 1; }
export GOVERNANCE_TIMELOCK="$(field "$OUT/governance.json" GovernanceTimelock)"
export GOVERNANCE_MULTISIG="$(field "$OUT/governance.json" GovernanceMultisig)"
export PATRONAGE_KYC_REGISTRY="$KYC" PATRONAGE_MEMBERSHIP
run_script script/DeployDormantLedgers.s.sol

# ValidatorRegistry(governance, slasher, rewardMinter, minStake = 32,000 SALT, blockSubsidy 1 SALT,
# priorityFeeShareBps 5000, maxEpochEmission 10,000 SALT): the pipeline's values.
REGISTRY="$(create src/ValidatorRegistry.sol:ValidatorRegistry "$GOVERNANCE_TIMELOCK" \
  0x000000000000000000000000000000000000C001 0x000000000000000000000000000000000000C002 \
  32000000000000000000000 1000000000000000000 5000 10000000000000000000000)"
[ -n "$REGISTRY" ] || { echo "moneypath-anvil: ValidatorRegistry deploy failed" >&2; exit 1; }
export VALIDATOR_REGISTRY="$REGISTRY" MEMBERSHIP_GRANTOR="$GRANTOR"
run_script script/DeployGenesisMembership.s.sol

POOL="$(create src/LiquidStakingPool.sol:LiquidStakingPool "$GOVERNANCE_TIMELOCK")"
[ -n "$POOL" ] || { echo "moneypath-anvil: LiquidStakingPool deploy failed" >&2; exit 1; }

cast rpc --rpc-url "$RPC" anvil_setCode "$ED25519_PRECOMPILE" "$ED25519_STANDIN" >/dev/null

M="$OUT/membership.json"
BOOK="$WORK/moneypath-book.json"
cat >"$BOOK" <<EOF
{
  "chainRef": "$REF",
  "deployer": "$DEPLOYER",
  "grantor": "$GRANTOR",
  "contracts": {
    "GovernanceTimelock": "$GOVERNANCE_TIMELOCK",
    "GovernanceMultisig": "$GOVERNANCE_MULTISIG",
    "CitrateMemberSBT": "$(field "$M" CitrateMemberSBT)",
    "MemberBond": "$(field "$M" MemberBond)",
    "MembershipStakeVault": "$(field "$M" MembershipStakeVault)",
    "ValidatorRegistry": "$REGISTRY",
    "LiquidStakingPool": "$POOL"
  }
}
EOF
for c in CitrateMemberSBT MemberBond MembershipStakeVault; do
  [ -n "$(field "$M" "$c")" ] || { echo "moneypath-anvil: $c missing from $M" >&2; exit 1; }
done
echo "==> deployed: $(tr -d '\n ' <"$BOOK")"

echo "==> citrate-core money paths (cargo test, ignored e2e)"
cd "$CORE"
CITRATE_E2E_MP_RPC="$RPC" CITRATE_E2E_MP_BOOK="$BOOK" CITRATE_E2E_MP_UPDATE="$UPDATE" \
  cargo test -p citrate-core --lib moneypath_e2e_tests::anvil_moneypath_genesis_set \
  -- --ignored --exact --nocapture --test-threads=1 2>&1 | tee "$WORK/cargo-test.log"
# A filter that matches nothing also exits 0: require that exactly the one e2e test ran and passed.
grep -q "test result: ok. 1 passed" "$WORK/cargo-test.log" ||
  { echo "moneypath-anvil: the e2e test did not run and pass" >&2; exit 1; }
echo "moneypath-anvil: PASS (citrate-chain $REF)"
