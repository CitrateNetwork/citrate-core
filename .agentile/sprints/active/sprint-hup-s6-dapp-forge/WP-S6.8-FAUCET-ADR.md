---
created: 2026-10-01T09:00:00Z
branch: hup/n3-faucet-adr-literacy
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S6
wp: HUP-S6.8
---

# HUP-S6.8: faucet core ADR and the measured 40204 cost table

Planset `2026-09-30-hermes-upskill`: 05_SPRINTS_AND_WPS (S6.8), 00_OVERVIEW D-3 and D-31, red-team
correction 15 ("faucet core ADR + measured cost table"). Sprint issue:
CitrateNetwork/citrate-federation#283. The decision itself is
[`docs/adr/ADR-2026-10-01-faucet-for-deploy-gas.md`](../../../../docs/adr/ADR-2026-10-01-faucet-for-deploy-gas.md)
(status **proposed**); this file is the evidence and the not-done list.

## What landed

| Item | State |
|---|---|
| ADR: faucet for deploy gas only, supersedes ADR-2026-07-27's "No faucet" for deploy gas, scope, who pays, routing through core with no sidecar keys, app-side abuse limits, default off | proposed, waiting for the owner |
| Cost table: deploy, verify, mint (three shapes), `setBaseURI`, member SBT mint, AgentSBT mint, `anchor()` (first and later root), transfer | measured 2026-10-01, blocks 61,570 to 61,978 |

## How the numbers were taken

- Read-only against `https://rpc.citrate.ai` (chain id 40204 confirmed): `eth_gasPrice`,
  `eth_feeHistory`, `eth_maxPriorityFeePerGas`, `eth_getCode`, `eth_call`, `eth_estimateGas`. No
  transaction was sent to 40204.
- hello-mint was the `erc721` template on `origin/hup/n3-templates` (not yet on the integration
  branch), rendered by hand with fixed parameters and compiled with forge 1.5.1 / solc 0.8.36 against
  the OpenZeppelin checkout at the pinned v5.7.0 commit `cab1993`.
- Standard-EVM gas came from a local anvil fork of 40204 at block 61,598 (Cancun). Transactions went
  to the fork process only; it was stopped afterwards.
- The node's `eth_estimateGas` is padded (read in `citrate-chain` `core/api/src/eth_rpc.rs`): +10 %
  for a deploy, `max(2 x used, used + 50,000)` for a state-changing call. Calibrated both ways on the
  same calls: node simulation = standard + 700 for a call, + 32,000 for a creation.
- The node ignores `eth_call`/`eth_estimateGas` state overrides, so calls into the not-yet-deployed
  hello-mint are "projected" from the fork figure with the padding rule; the table says which rows
  are live and which are projected.

## Findings worth carrying forward

- **AgentSBT cannot be minted on 40204 today.** `mintAgent` is `onlyOwner` (the CitAgentTimelock)
  and needs an active OrganizationSBT; `OrganizationSBT.nextTokenId()` is 0. HUP-S7.4 ("AgentSBT mint
  at onboarding") needs an organisation and a minting path first.
- **Member SBT mint is paid by the orchestrator**, not the member (`onlyOwner`), so it is outside the
  faucet's scope.
- **Deploy gas is tiny next to the drip.** The full hello-mint path needs about 0.002 SALT up front at
  1 gwei; one 10-SALT drip covers it about 5,000 times. This is why the ADR gates on need and on one
  drip per member per day, and asks (O-1) whether a smaller gas-only drip is worth a faucet change.
- **`anchor()` estimation works from a zero-balance address** (the node simulates with the sender
  funded), so the S7.3 anchor flow can estimate before the anchor key holds any gas.
- **Top-level `eth_call` to a precompile address returns `0x`** on the current node, including the
  standard `0x02`. Recorded in the S7.7 literacy skills; it matters to the planned `precompile_call`
  helpers (S7.2), which will need to call through contract code.

## Gates

Docs-only WP: no code. The ADR follows Rule 5 frontmatter, uses HIC vocabulary, and has no
em-dashes. The numbers were re-read from the saved command output before writing.

## Not done

- Not accepted: status stays **proposed** until the owner decides O-1 to O-4.
- No implementation: `faucet_request` (S6.5) is not built here; the ADR says it stays off by default.
- No faucet-side change (membership check, CAPTCHA path for the desktop app): those are citrate-chain
  changes plus a DGX deploy, listed as O-2 and O-3.
- The table is one sample on an idle chain at 1 gwei. The app must re-estimate before every request.
- Which gas figure the chain bills (standard or the node's +700 / +32,000) is not established by
  read-only measurement; the ADR budgets with the node's.
