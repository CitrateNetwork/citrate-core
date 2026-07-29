---
created: 2026-07-28
updated: 2026-07-28
branch: docs/lock-design-decisions
author: Claude (Opus 4.8), directed by @SaulBuilds
status: decisions locked — contract spec for G1 (owner-reviewed)
relates:
  - citrate-core/docs/DGX_HANDOFF_CONSENSUS_AND_ALF_2026-07-28.md  (packages M-1/M-2/M-3 for DGX + deconflicts vs ALF-ND)
  - citrate-alf-web/docs/ALF_ND_NODE_BRIDGE_PLAN_2026-07-28.md      (independent track; shares only the alf_member claim)
---

# Membership stake: 1‑year locked validator bond — design + spec

Owner decisions (2026‑07‑28) are LOCKED (§A). The good news: the existing
`citrate-chain/contracts/src/core_membership/MembershipStakeVault.sol` **already**
implements most of this — staked + validator‑eligibility‑attributed + a
`Attributed → Released → Claimed` lock lifecycle whose ONLY SALT‑out path is
`claimReleased`. The program is mostly ADDITIONS to that contract, not a new one.

## A. Locked decisions (owner)
1. **The locked 32k IS the validator bond** (model V). No separate membership stake.
2. **Extend `MembershipStakeVault`** — do NOT add new erroneous contracts. Compose
   into multiple contracts ONLY if stack depth forces it.
3. **Upgradeable — required.** (The contract is currently NOT upgradeable — see §C.1.)
4. **Automatic time‑based unlock**, expressed as a **block height a fixed count ahead
   of the grant's start height**, targeting **~1 year minus up to ~1 day** ("a little
   less than a year, within a day, is better than a little more") — so a member can
   exit slightly early rather than be held over.
5. **No automatic withdraw.** Unlock makes exit ELIGIBLE; funds stay staked +
   producing until the member acts. Auto‑returning principal would silently drop node
   operators. Optional **re‑lock incentive** is a later/mainnet feature.
6. **KYC supersedes every withdrawal check.** No release/claim if the member is not
   KYC‑verified — even after the time lock elapses. This gate is checked FIRST.
7. **Entitlement (companion):** a paid‑but‑unverified member GETS ACCESS (tier
   `commercial`), but **cannot withdraw the stake OR download from the commissary**
   until KYC‑verified.

## B. What the vault already gives us (verified in source)
`MembershipStakeVault.sol`:
- `grant(member, amount) payable onlyOwner` — stakes into `LiquidStakingPool`, mints
  stSALT to the vault, records `grantedAt`, sets `attributedShares[member]`, state
  `Attributed`. Comment: "attribute validator eligibility to the member."
- `attributedStake(member) = pool.previewWithdraw(attributedShares[member])`, and
  `meetsRequirement := attributedStake(member) >= VALIDATOR_STAKE_REQUIREMENT` — the
  vault stake IS the validator‑eligibility measure.
- Lifecycle `Attributed → Released → Claimed` (+ `Lapsed`/`renew`). The ONLY path that
  moves SALT out is `claimReleased`, reachable only from `Released`. So principal is
  **non‑withdrawable until an explicit release** — exactly the lock we want.
- Release today is orchestrator‑driven (`releaseGrant` / `enableMainnetRelease`,
  `onlyOwner`) — NOT time‑automatic. The source even carries the open TODO: "should the
  vault additionally hard‑enforce `block.timestamp >= grantedAt + 365 days`". We are
  answering that TODO: **yes.**
- NO clawback, no owner‑withdrawal, no sweep — "stake coverage, not a treasury pocket."

## C. What to ADD (the G1 contract work, on the extended vault)
1. **Upgradeability.** Convert to a UUPS (or transparent) proxy: `Initializable`,
   `initialize(owner, pool)` replacing the constructor, `_authorizeUpgrade onlyOwner`,
   storage‑gap. The `immutable pool` becomes an initialized storage var. This is the
   riskiest change (storage layout) — do it first, re‑verify the existing suite green.
2. **Automatic time‑release eligibility.** Add `LOCK_BLOCKS` (≈ 1 year − ~1 day, in
   40204 block time) and record `unlockBlock = block.number + LOCK_BLOCKS` at grant.
   Make release ELIGIBLE (not automatic transfer) once `block.number >= unlockBlock`:
   a member‑callable `requestRelease(grantId)` that moves `Attributed → Released`
   iff `block.number >= unlockBlock` AND the KYC gate (§C.3) passes. Keep the existing
   owner `releaseGrant` for admin/lapse paths. (Prefer block height over timestamp:
   the owner specified blocks‑ahead‑of‑tip, and it's miner‑manipulation‑resistant.)
3. **KYC gate that supersedes all.** Both `requestRelease` and `claimReleased` must
   FIRST require the member is KYC‑verified. KYC is off‑chain, so pick ONE:
   - **(i) Attestation on `CitrateMemberSBT`** — the SBT carries a `kycVerified` flag
     the vault reads (`ICitrateMemberSBT(sbt).isKycVerified(member)`). Hard on‑chain
     enforcement; needs the SBT to expose/maintain the flag.
   - **(ii) Orchestrator‑gated** — release stays `onlyOwner` and the treasury signer
     only calls it for KYC‑verified members (off‑chain check). Simpler; enforcement
     lives in the operator, not the contract.
   RECOMMEND (i) if the SBT can carry the flag (matches "supersedes every check,
   on‑chain"); else (ii) as an honest interim. Either way the gate is checked BEFORE
   the time check.
4. **Node/consensus reconciliation (the one real open question).** The node's
   stake‑gated membership reads `CITRATE_VALIDATOR_REGISTRY` (`ValidatorRegistry`),
   while the vault attributes eligibility via `attributedShares`. Decide how a
   vault‑granted member becomes a block‑producing validator:
   - (a) consensus ALSO honors `MembershipStakeVault.meetsRequirement(member)`, or
   - (b) the vault registers the member in `ValidatorRegistry` on grant.
   This is a citrate‑chain consensus decision (DGX) — flag for G0.5. Until resolved, a
   vault‑granted member is staked + eligible‑by‑vault but may not yet PRODUCE blocks.

## D. Cross‑repo wiring (after G1 + audit + deploy)
1. **core‑membership** `grant/execute.ts`: switch leg 1 from `fundMember` (liquid) →
   `vault.grant(member, 32k)` (the existing staked+attributed path). Idempotent.
2. **citrate‑core app**:
   - `grant_status.rs`: read `attributedStake` + the new `unlockBlock`; add to
     `GrantStatus`. Settle S3 on `attributedStake >= 32k` (the original gate) and
     **REMOVE the temporary #105 native‑balance bridge** (settling on liquid was only
     to unstick the interim build).
   - Dashboard/Wallet: show "Staked · unlocks at block N (~<date>)"; the honesty fix
     already reads real `attributedStake`, so it lights up automatically.
   - Withdrawal UI: only offer release when `block.number >= unlockBlock` AND KYC‑
     verified (chain enforces both regardless).
3. **Entitlement companion** (§A.7): core‑membership leg 3 writes `commercial` (not
   `commercial.kyc`) for a paid‑unverified member; `citrate-identity` /userinfo gate
   serves `commercial` without KYC (only `commercial.kyc` needs verified); withdrawal
   (above) + commissary download gate on KYC‑verified.

## E. Sequencing + gates
- **G0.5 — consensus reconciliation** (§C.4) with DGX: how vault attribution → block
  production. Blocks the "membership = validator produces blocks" promise.
- **G1 — contract red/green** (@rule8, T1 money): upgradeability first (storage‑safe),
  then time‑release, then KYC gate. Failing tests first: only‑after‑unlock, KYC‑
  supersedes‑time, no‑early‑release, no‑double‑claim, reentrancy, upgrade‑storage‑safe.
- **G2 — independent audit** before deploy.
- **G3 — deploy/upgrade + reroll**; re‑pin/getCode.
- **G4 — backend + app wiring** (§D) + E2E: pay → staked+locked grant → app settles on
  attributedStake → dashboard shows lock + unlock block → release blocked until unlock
  AND KYC → claim pays out.

## F. Interim (until G4 ships)
The current build settles S3 on the **liquid** grant (#105) and honestly shows it as
liquid (0 staked). Deliberate bridge so the app is testable now; G4's app changes
remove it. No funds at risk — the 32k is liquid + spendable, honestly labeled.
