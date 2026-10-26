---
created: 2026-10-26
branch: beta/rc52-moneypath-e2e
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: active
wp: RC52 G7 (harness inventory §2 G7, §3 item 8)
---

# Money-path e2e on the genesis contract set (G7)

`scripts/e2e/moneypath-anvil.sh` runs citrate-core's money paths end to end against the contracts
that ship at genesis. It deploys them from a citrate-chain commit onto a throwaway anvil with chain
id 40204, then runs the ignored Rust test `moneypath_e2e_tests::anvil_moneypath_genesis_set`. That
test drives core's own code and compares every step with a golden. Nothing touches the live chain.

## Run it

```sh
# needs foundry (anvil, forge, cast), git, cargo; gh unless --ref is given
CARGO_TARGET_DIR=<your own target dir> scripts/e2e/moneypath-anvil.sh --chain ../citrate-chain
```

| Option | Meaning |
|---|---|
| `--chain DIR` | A citrate-chain git checkout. Default: `$CITRATE_CHAIN_DIR`, else `../citrate-chain`. The script adds a temporary worktree of it and removes it on exit. |
| `--ref SHA` | The citrate-chain commit to deploy. Default: `$CITRATE_E2E_MP_REF`, else the head of citrate-chain#331, read with `gh pr view 331 -R CitrateNetwork/citrate-chain --json headRefOid`. |
| `--update` | Rewrite the goldens from this run instead of comparing. Review the diff before committing it. |
| `--keep` | Keep the temporary worktree, forge logs and anvil log. |
| `ANVIL_PORT` | Default 18547. The script refuses a port that is in use. |

Exit 0 means all three flows matched their goldens. The script also checks that the one e2e test
ran: a cargo filter that matches nothing would otherwise exit 0.

A run took 5.3 minutes on an M-series Mac (2026-10-26). Most of that is forge compiling and
deploying the contracts (about 2.5 minutes) and cargo building the test binary. The flows take
10 to 90 seconds, mostly mining. It is not in CI, because CI does not run forge
or anvil for core. The goldens' truth labels are checked in CI (see "What CI checks").

## Which chain it pins

The goldens were recorded at citrate-chain **`dec0996465c8e7173cc70a48264600f479a629df`**. That is
the head of #331, the top of the genesis stack #320 → #324 → #331, on 2026-10-26. None of the three
is merged yet. The default ref follows #331's head, so a new push to the stack is compared with these
goldens on the next run, and any change in behaviour shows up as drift. #374 (`DeployAll` passes the
canonical SALT supply to `TreasuryGovernor`) does not touch these flows: the run does not use
`DeployAll`.

After the stack merges, run with `--ref <merge commit>`, or set `CITRATE_E2E_MP_REF` to it.

## What is deployed

The deploy follows citrate-chain's own `scripts/ci/genesis-pipeline-anvil.sh`. Each step is a
`forge script --broadcast` of that ref's ceremony scripts, with the pipeline's genesis-profile
inputs (a 48-hour, 3-of-5 council and a 1-hour, 2-of-3 guardian, both placeholder seats).

| Contract | How | In the genesis set? |
|---|---|---|
| Governance timelock, council, GuardianCanceller | `DeployGenesisGovernance` | yes (#320, #324) |
| FederatedRoundLedger, PatronageLedger (dormant) | `DeployDormantLedgers` | yes (#320) |
| CitrateMemberSBT, MemberBond, MembershipStakeVault (proxy) | `DeployGenesisMembership`, grantor = anvil account 2 | yes (#331, Track M) |
| ValidatorRegistry | `forge create` from the same ref, governed by the timelock, minStake 32,000 SALT (the pipeline's values) | deployed at genesis, not in the ceremony books |
| LiquidStakingPool | `forge create` from the same ref, governed by the timelock | **no**: the C2 genesis spec retires it. It is deployed only because core still ships the C1-W withdrawal read |

The genesis ceremony refuses to run on chain 40204 unless its floors hold. So a run that passes has
also run the ceremony scripts against a node that reports 40204.

### Test stand-ins (and why each is honest)

- **ed25519 precompile (0x0120).** Anvil has no ed25519 verifier, so the script installs a small
  contract there. It answers 1 only for an exact `pubkey || sig || message` input that the Rust test
  has whitelisted, and it whitelists one only after verifying the signature with ed25519-dalek. The
  message is the digest core signed, and the registry computes its own digest on chain. So
  activation succeeds only if the two digests are byte-identical. This is stricter than the chain's
  own forge tests, which mock the precompile to always answer 1.
- **The treasury's side.** Core has no code here, so the test does it as the grantor (anvil
  account 2): `mintMember`, `grant` (32,000 SALT), `setKycVerified` and `recordRelease`.
- **Time.** Blocks are mined with `anvil_mine`, and the 364-day lock elapses through
  `evm_increaseTime`. Anvil runs with `--prune-history`, because mining about 60,000 blocks is
  superlinear without it.
- **Keys.** Anvil's public test mnemonic only. The member wallet is account 0, imported into a
  throwaway in-memory vault. The node's proposer key is a fixed test seed in a temp data dir.

## The flows

Each step records three things:

- the events its transactions emitted;
- the chain state, read with direct `eth_call`s (vault grant, bond flags, registry `isActive`,
  `stakeOf`, `validatorInfo`, pool shares, balances);
- core's read model.

| Flow | Steps | Core code driven |
|---|---|---|
| `membership-activation` | before-grant (core refuses activation) → grant (treasury) → activate (gas = the node's estimate + `money_gas` margin, recorded as `gasLimit`) → activate-again (core refuses) → epoch-reached | `grant_status::read_grant_status`, `validator_activation::read_activation_status`, `node::bond_activation_intent` (the body of `node_register_validator`) with `validator::sign_registration_from_data_dir`, the SignatureCeremony (`request` + `approve_and_broadcast`, EIP-155 from the vault) |
| `bond-exit` | kyc-attested → release-inside-lock (reverts) → lock-elapsed → release (registry unbond, vault records it; "Exiting") → withdraw-too-early (reverts) → exit-matured (32,000 SALT back to the member; "Bond returned") | the same reads; the ceremony for the member's sends; `staking::read_pending_withdrawals_with` (C1-W) after the release |
| `lsp-withdrawal-c1w` | stake → request (core's gas: reverts, #501) → request-estimated-gas → claim-too-early (reverts) → delay-elapsed → claim (40 SALT back) | `staking::stake_intent`, `request_withdrawal_intent`, `claim_withdrawal_intent`, `read_self_stake`, the C1-W paged scan (plain `eth_getLogs` windows of 1,000 blocks on anvil; anvil has no `citrate_getLogs`, so `confirmed` is false) |

Pin bonds are not covered. `IPFSIncentivesV3` is not deployed by #320/#324/#331, and core has no
pin-bond transaction yet: pins are local only (storage S2.2).

## Goldens

The goldens live in `src-tauri/tests/fixtures/moneypath/<flow>.golden.json`, one per flow. Before
they are compared, values that move between identical runs are normalized:

- Addresses become role names (`member`, `bond`, `MembershipStakeVault`, …), also inside messages.
- Tx hashes become `<tx:label>`, and the node key becomes `<nodePubkey>`.
- Gas used and gas price become `<gas>`, and the measured block time becomes `<not compared>`.
- Unlock timestamps become offsets from their block (`blockTs+31449600`, `grantTs+31449600`).

Block numbers, amounts, epochs, states, revert status and core's messages are compared exactly.
The first difference fails the run, with its JSON path.

To change a golden on purpose, run with `--update` and review the diff in the PR. The labels in
`labels.json` are hand-written and are never rewritten by the script.

## What CI checks (no anvil)

- `src/onboarding/moneypathGoldens.test.ts` (vitest) feeds each recorded `activationStatus` through
  `validatorState` and `activationPhase`, the functions the Activate-validator step renders with. It
  checks the word and phase in `labels.json`, for example "Registered · active from epoch 1", then
  "Validator active", then "Exiting", then "Bond returned". A golden rewrite that changes what the member is told fails
  there.
- `moneypath_goldens_are_complete`, `moneypath_event_table_is_well_formed` and
  `moneypath_first_diff_names_the_path` (cargo, in `ci`) check the goldens' shape and the comparison
  itself.

## Gaps the run shows

- **#501 (defect).** `requestWithdrawal` goes out with a fixed 150,000 gas, and the pool needs about
  160,500, so the core-built request reverts. The golden records the revert. The next step resends
  core's same intent with the estimated gas, so the C1-W read is still exercised. The pool is
  retired at r1006 (D-9), and the LSP removal deletes this send, the C1-W read and this flow. Every
  other money send core builds now carries the node's estimate plus a margin (`money_gas`), and the
  `tripwire_no_transaction_core_builds_has_a_fixed_gas_limit` test keeps it that way.
- **Track M exit has no core builder.** Core has no `MemberBond.requestRelease` or `withdrawToMember`
  intent. The ceremony decodes both as "Unrecognized" and needs the raw-mode ack, and the test
  supplies their calldata. The lock is 364 days, so this is not on the 0.5.2 path, but it is the only
  exit at genesis.
- **C1-W does not see the Track M exit.** The pending-withdrawals scan reads only
  `LiquidStakingPool.WithdrawalRequested`, so after a bond release it shows nothing. The genesis
  spec retires the pool.
- **"Exiting" after the exit has paid out (fixed).** `validatorInfo.status` stays Exiting after
  `withdrawToMember` returns the principal. The activation read now carries `validatorInfo`'s bonded
  stake and escrow, and with both at zero the step says "Bond returned" (phase `exited`).
- **C1-W scan start.** `staking::LIQUID_STAKING_POOL_DEPLOY_BLOCK` is the r1005 pool's block (427).
  The script mines past it before deploying, so the scan covers the pool here. On a new genesis
  this pin has to move with the book, or go with the pool.
