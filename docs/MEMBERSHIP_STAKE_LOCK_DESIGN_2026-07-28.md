---
created: 2026-07-28
branch: test/combined-remediation
author: Claude (Opus 4.8), directed by @SaulBuilds
status: draft — design for owner review (NO code yet)
---

# Membership stake: 1‑year locked grant — design draft

The owner's decision (2026‑07‑28): the $48 membership grant must land **staked,
non‑withdrawable, locked exactly 1 year, with no user action**. This does not
exist on‑chain today, so this is a **citrate‑chain contract program** followed by
backend + app wiring. This draft frames the decisions BEFORE any code.

## 1. Where we are (verified on live 40204)
- The grant currently funds the member EOA **liquid** (`core-membership grant/execute.ts`
  leg 1 `fundMember`, ADR 2026‑07‑27 "replacing vault.grant"). On‑chain a granted
  member reads: native `32000.05 SALT`, `MembershipStakeVault.attributedStake = 0`,
  no validator bond.
- The dormant `MembershipStakeVault.grant(member, amount) payable` still exists and
  would stake into the LiquidStakingPool + attribute shares — but has **no lock**.
- The longest lock anywhere is `LiquidStakingPool` withdrawal delay ≈ **7 days**
  (`50_400` blocks). There is **no `lockUntil` / unlock‑time / 1‑year mechanism** on
  any deployed contract.

## 2. The load‑bearing design question (needs owner call)
Is the locked 32k the **validator bond** or a **separate membership stake**?
- **(V) Membership == validator bond.** The 32k is the validator's stake in
  `ValidatorRegistry` (produces blocks + earns the subsidy). Problem: `registerValidator`
  is `msg.sender`‑bonded (staker = the caller) and needs a synced node + `proposer.key`
  — so a treasury‑granted, no‑user‑lift, timed‑1‑year lock does **not** map cleanly
  onto the current registry (which has no timed lock and no treasury‑on‑behalf path).
- **(M) Membership stake, separate from validator.** The 32k sits in a lock‑aware
  membership vault (staked, non‑withdrawable 1 yr), and becoming a block‑producing
  validator is a *separate, optional* step. Cleaner to build; but then "staked" ≠
  "earning validator rewards" until they also activate a validator.
- **(H) Hybrid.** Locked membership stake now (M), and later the same principal can
  be *delegated* to a validator without unlocking.

**This choice determines the whole contract.** My recommendation: **(M)** for the
first ship — a lock‑aware membership vault is the smallest correct contract that
meets "staked + non‑withdrawable + 1‑year, no user lift", and it decouples the money
lock from the (heavier) validator/producer path. Validator activation stays the
existing separate flow.

## 3. Proposed contract (assuming M)
Two implementation shapes:

**(A) Extend `MembershipStakeVault`** — add:
```
function grantLocked(address member, uint256 unlockTime) external payable onlyTreasury
  // stakes msg.value for `member`, records lockedUntil[member] = unlockTime
mapping(address => uint64) public lockedUntil;   // 0 = no lock
// withdrawal path reverts while block.timestamp < lockedUntil[member]
function unlockTimeOf(address) external view returns (uint64);  // for the app read
```
Pros: reuses the vault + its `attributedStake` read the app already knows. Cons:
requires the vault be upgradeable (or redeployed → reroll) and a withdrawal guard.

**(B) New `LockedMembershipVault`** — dedicated contract holding the bond with an
explicit `lockedUntil`, `grantLocked(member, unlockTime) payable onlyTreasury`, and a
`withdraw()` gated on `block.timestamp >= lockedUntil`. Pros: clean separation,
auditable in isolation. Cons: a new address (reroll) + the app learns a new contract.

Recommendation: **(A)** if the vault is upgradeable; else **(B)**. Either way the key
invariants for audit (@rule8):
- only the treasury signer can `grantLocked` (no user‑forgeable lock/mint);
- principal is **non‑withdrawable** until `lockedUntil` (revert, not UI‑only);
- `lockedUntil = grantTime + 365 days`, set at grant, immutable;
- re‑grant / top‑up semantics defined (extend lock? refuse? — decide);
- the member's `attributedStake`/balance reads reflect the locked principal so the
  app can settle + display it.

## 4. Cross‑repo wiring (after the contract lands + is audited)
1. **citrate‑chain**: implement + test the lock‑aware grant; audit (@rule8); deploy
   (address‑neutral if CREATE2, else re‑pin) + reroll.
2. **core‑membership** `grant/execute.ts`: switch leg 1 from `fundMember` (liquid) →
   `grantLocked(member, now + 365d)`. Keep idempotency (skip if already locked‑staked).
3. **citrate‑core app**:
   - `grant_status.rs`: read the locked stake + `unlockTime`; add to `GrantStatus`.
   - `isGrantOnChain` (store.ts): settle S3 on the **locked attributedStake ≥ 32k**
     (and REMOVE the temporary #105 native‑balance bridge — settling on liquid was
     only to unstick the current build).
   - Dashboard/Wallet: show "Staked · locked until <date>" from the real read (the
     honesty fix already reads real `attributedStake`, so it lights up automatically).
   - Withdrawal UI: hide/disable while locked (chain enforces it regardless).

## 5. Sequencing + gates
- **G0 — design sign‑off** (this doc): pick M/V/H + A/B; define re‑grant + reward
  semantics.
- **G1 — contract red/green**: failing tests first (only‑treasury, non‑withdrawable‑
  until, exact‑1‑year, reentrancy, top‑up), then implementation.
- **G2 — audit (@rule8)**: T1 money contract; independent review before deploy.
- **G3 — deploy + reroll**; re‑pin/getCode all addresses.
- **G4 — backend + app wiring**; E2E: pay → locked‑staked grant → app settles on the
  locked stake → dashboard shows the lock + unlock date → withdrawal blocked.

## 6. Interim state (until this ships)
The current build settles S3 on the **liquid** grant (#105) and the dashboard honestly
shows it as liquid (0 staked). That is a deliberate bridge so the app is testable now;
G4 removes it. No user funds are at risk in the interim — the 32k is liquid + spendable
(honestly labeled), just not yet locked.

## Open questions for the owner
1. **M / V / H** — is the locked 32k the validator bond, a separate membership stake,
   or hybrid? (Determines the contract.)
2. **Vault upgradeable?** (Extend `MembershipStakeVault` vs a new `LockedMembershipVault`.)
3. **Rewards while locked** — does the locked stake earn (staking yield / validator
   subsidy) during the year, or is it purely a lock?
4. **Unlock behavior at +1yr** — auto‑withdrawable, auto‑renew, or convert to liquid
   stake? What is the member's post‑lock state?
5. **KYC coupling** — does the locked grant require KYC‑verified (ties to the
   entitlement gate below)?
