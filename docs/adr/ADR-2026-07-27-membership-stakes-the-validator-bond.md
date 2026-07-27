---
created: 2026-07-27
branch: feat/w1-3-validator-registration
author: Claude (Opus 4.8), directed by @SaulBuilds
status: accepted
---

# ADR — Membership stakes the validator bond (the 32k earns as a block producer)

## Context
citrate-chain main @26f08c8 makes nodes **earn as block producers**: multi-producer
(#115) + the flat per-block `blockSubsidy()` (#103) credited to `validatorInfo.rewards`.
To earn, a node must be **registered** in `ValidatorRegistry.registerValidator`, which
is `payable` — `staker = msg.sender`, `bondedStake = msg.value`. The bond is native
SALT sent by the member's own wallet; it is a **separate lock** from the
MembershipStakeVault / LiquidStakingPool (that contract's balance is not readable by
ValidatorRegistry). So the vaulted grant cannot double as the bond.

## Decision
**Membership makes the member a block-producing validator; the 32k is their bond.**
1. **Reroute the grant** (treasury-signer + core-membership): the treasury funds the
   member's smart wallet with 32k SALT instead of `vault.grant`. Same SALT source, new
   destination. The SBT mint + entitlement raise are unchanged. LiquidStakingPool
   starts empty — members liquid-stake their **earnings** (block subsidy, pinning) later.
2. **Register in the app** (citrate-core), at the END of onboarding once the node is
   **synced**: read the node's `proposer.key` → sign the EIP-712 register digest with
   the ed25519 proposer key → the wallet sends
   `registerValidator{value:32k}(pubkey, sig)` as a **CitratePaymaster-sponsored UserOp**
   through the SignatureCeremony (Rule 3). Registering only once synced minimizes the
   window the wallet holds 32k liquid and only bonds members actually running a node.
3. No faucet, **no DGX**, no new contract: ValidatorRegistry already exists on 40204;
   the treasury-signer (`citrate-identity/services/treasury-signer`) + the grant flow
   (`core-membership`) + the app are all local.

## Why no faucet / no paymaster-for-stake
A faucet is unnecessary — the treasury already sources the grant's 32k; we only change
where it lands. A paymaster sponsors **gas**, not `msg.value`, so it cannot supply the
bond — but the CitratePaymaster DOES sponsor the register UserOp's gas, so the member
needs exactly 32k.

## Slashing / safety (why laptops are fine)
ValidatorRegistry's real slash is **equivocation** (double-signing → ~100%), which only
happens if the same `proposer.key` runs on two machines. A member on one laptop never
equivocates. **Going offline just pauses earnings — the bond is safe** (the 5%
missed-checkpoint tier lives in `NematocystSlashing`, the compute/model-provider layer,
not the block-producer registry). The one invariant the app enforces: never copy
`proposer.key` to a second machine (WP-11 already treats it as preserve-not-copy).
A non-equivocation `slasher` role exists but its beta enforcement is a DGX/ops fact
(low practical risk); flagged, not silently assumed.

## Build (all local)
- `validator.rs` (this branch): `registration_digest` + `sign_registration` (ed25519
  over the digest) + `register_validator_calldata` + `registration_nonce_calldata` —
  pinned byte-for-byte to `crypto.rs` + `ValidatorRegistry.sol` by golden vectors.
- **TODO next:** the `node_register_validator` command (read seed + nonce via eth_call
  → sign → build calldata → ceremony UserOp with value=32k → broadcast); the treasury
  grant reroute; the onboarding "activate validator once synced" step; the
  `validatorInfo(pubkey).rewards` earnings read.

## Consequences
- citrate-core members are validators; the 32k is bonded + productive, not idle in a
  vault. Rewards accrue to the member (staker) and are claimable.
- One node per member (equivocation guard). A reinstall/new-machine mints a new
  proposer key → the member re-registers (or migrates `proposer.key`).
