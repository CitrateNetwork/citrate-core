#!/usr/bin/env bash
# CORE-B1.4 — the LIVE PROOF (Rule 11). Signs a REAL 40204 transaction through the
# SignatureCeremony from the vault key and broadcasts it to the live chain, then
# confirms it by block inclusion.
#
# NO mocks (Rule 1): real A2 vault, real EIP-155 signing via the ceremony, real
# eth_sendRawTransaction to https://rpc.citrate.ai, real receipt poll.
#
# HONESTY: the live OS keyring + the interactive approval UI are not headless. The
# live test uses the in-memory keyring fake (same custody vault crypto) and calls
# the approval function directly in place of a human click — the SIGNING +
# BROADCAST + on-chain confirmation are fully real.
#
# Usage (from repo root or anywhere):
#   src-tauri/scripts/b1_4_live_proof.sh
#
# It sources DEPLOY_KEY from citrate-defense_prime-shell/.env.demo.local (gitignored),
# funds a fresh app test wallet from 0x98a3…, and runs the ignored live test.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENV_FILE="${REPO_ROOT}/../citrate-defense_prime-shell/.env.demo.local"
RPC_URL="https://rpc.citrate.ai"
FUNDER="0x98a32D944e9138B14A35b5D4dcE53339570F371A"

echo "[b1.4-live] repo root: ${REPO_ROOT}"

if [[ ! -f "${ENV_FILE}" ]]; then
  echo "[b1.4-live] ERROR: ${ENV_FILE} not found (needs the gitignored DEPLOY_KEY)." >&2
  echo "[b1.4-live] The live proof requires a funded 40204 key. See the B1.4 scope." >&2
  exit 1
fi

# shellcheck disable=SC1090
source "${ENV_FILE}"
if [[ -z "${DEPLOY_KEY:-}" ]]; then
  echo "[b1.4-live] ERROR: DEPLOY_KEY not exported by ${ENV_FILE}." >&2
  exit 1
fi
export DEPLOY_KEY

CAST="${HOME}/.foundry/bin/cast"
echo "[b1.4-live] funder ${FUNDER} balance (wei): $(${CAST} balance ${FUNDER} --rpc-url ${RPC_URL})"
echo "[b1.4-live] chain id: $(${CAST} chain-id --rpc-url ${RPC_URL})"

echo "[b1.4-live] running the ignored live test (signs + broadcasts a real 40204 tx)…"
cd "${REPO_ROOT}/src-tauri"
cargo test --release b1_4_live_broadcast_real_40204_tx -- --ignored --nocapture

echo "[b1.4-live] done. The confirmed tx hash + block are printed above."
