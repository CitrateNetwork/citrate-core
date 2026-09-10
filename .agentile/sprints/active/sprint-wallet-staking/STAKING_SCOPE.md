---
created: 2026-07-16
branch: feat/core-wallet-staking
author: Claude Opus 4.8, directed by @SaulBuilds
status: building — WP1 (stake/deposit + real self-stake read)
program: CORE finish-list item 1 — real Stake/Withdraw (wallet liquid-staking money action)
rule8: yes — a value-bearing write (deposit forwards SALT via msg.value) + a chain read
grounded_in:
  - citrate-chain/contracts/src/LiquidStakingPool.sol (deposit/requestWithdrawal/claimWithdrawal/balanceOf)
  - canonical address book: contracts/addresses/40204.json + node-agent chainio generated/addresses.json
  - transfer.rs::wallet_send (the ceremony money-write model) + earnings.rs::read_claimable (the eth_call read model)
---

# CORE — real wallet Stake/Withdraw (finish-list item 1)

The Wallet → Staking tab's **Add stake** and **Withdraw self-added stake** buttons
call `store.settleUnwired` (honest stub). This sprint makes them real 40204 money
actions grounded in the deployed `LiquidStakingPool`, routed through the one
SignatureCeremony (Rule 3), mirroring the real `Send` (transfer.rs).

## Conflict check vs the DGX/other-team work order (owner asked)

**No conflict.** Grounded 2026-07-16:
- `LiquidStakingPool` is a **pure liquid-staking contract**: `deposit()` (payable)
  mints stSALT shares, open to any wallet. It has **no** validator-pubkey registry,
  **no** proposer binding, and slashing is socialized via share price — **not** tied
  to running a validator (verified in LiquidStakingPool.sol; validator *eligibility*
  is a balance check in MembershipStakeVault, a separate subsystem).
- **WO-1.3 `[dgx]`** = deliver the *validator onboarding spec* (min-stake confirm,
  proposer-pubkey binding, `NematocystSlashing`/heartbeat, faucet). Infra/backend.
- **Phase 2 `[core]`** "validator registration + staking flow" = the larger
  validator-set flow (proposer pubkey, `blocksProposed`, set membership) that
  **depends on WO-1.3** — I stay entirely out of it.
- This sprint is only the **wallet self-stake money action** (a user staking their
  own SALT into the pool for yield), which the Wallet UI already models correctly and
  separately from the vaulted grant ("Granted principal is vaulted until mainnet ...
  Only self-added stake can be withdrawn"). Nothing here touches proposer keys,
  validator registration, or slashing.

## Grounding corrections vs the startup directive (both were wrong)

1. **Address.** Directive said `0xfd27a3c9…685e` (copied from the stale `seed.ts`
   CONTRACTS placeholder). Canonical (both 40204.json AND node-agent book) is
   **`0xfd272195b55cb4f5a240a5be75aabab0d1c5685e`**. This sprint also corrects the
   dead `seed.ts` CONTRACTS block (pool/SBT/vault/entryPoint/paymaster) to 40204.json.
2. **ABI.** Not `stake()/withdraw()/staked(addr)`. Real ABI:
   - `deposit() payable` → shares  (selector `0xd0e30db0`)
   - `requestWithdrawal(uint256 shares)` → requestId  (`0xdf5dd1a5`)
   - `claimWithdrawal(uint256 requestId)` after **WITHDRAWAL_DELAY = 50400 blocks (~7d)**  (`0x423b176f`)
   - `balanceOf(address)` → SALT value of the caller's shares  (`0x70a08231`)
   Withdraw is a **two-step, 7-day-queued** flow that takes **shares** (UI has SALT) →
   it is its own WP.

## WP1 (this PR) — stake (deposit) + real self-stake read

- **[core] `staking.rs`** — new module (@rule8), mirrors transfer.rs + earnings.rs:
  - `LIQUID_STAKING_POOL` const (canonical) + pinned `deposit()`/`balanceOf(address)`
    selectors, each with a keccak-derivation drift test (Rule 11, like earnings.rs).
  - `wallet_stake(amount_wei)` command — builds a ceremony `Transaction` intent
    `{from: vault, to: pool, value: amount, data: deposit(), gas: 200000, chainId: 40204}`
    and returns the decoded `CeremonyView` PENDING. **Signs nothing** — the human
    approves via `sign_and_broadcast` (B1.4). Explicit gas (calldata tx: `finalize`
    returns None without it — no estimate path). amount==0 / locked vault → fail closed.
  - `read_self_stake(rpc, addr)` — `eth_call` `balanceOf(addr)` → wei of SALT (the
    user's OWN pool position; the vault holds the grant shares separately).
- **[core] earnings.rs** — extend `WalletBalances` with `stakedWei` populated from
  `staking::read_self_stake`; `wallet_balances` now returns a REAL self-stake (kills
  the `-1` sentinel).
- **[core] bridge/store/Wallet** — `bridge.wallet.stake`; `wallet_balances` adapter
  maps real `stakedWei`→`staked`; `store.walletStake` (mirror `walletSend`);
  `refreshWallet` folds `selfStake` from the real read; `onAddStake` → `store.walletStake`.
- **Acceptance:** Add stake builds a real pending ceremony to the pool; on approve it
  signs+broadcasts a real 40204 `deposit()` tx; the Staked figure reflects the real
  `balanceOf(self)` + the (separately-tracked) vaulted grant. No fabricated hash/number.

## WP2 (next PR) — withdraw path
`requestWithdrawal` (SALT→shares via a `getSharePrice` read, clamp to `shares[self]`) +
`claimWithdrawal` + pending-withdrawal tracking (eth_getLogs/event) + honest 7-day-queue
UI. Its own @rule8 review. `onUnstake` stays `settleUnwired` (honest) until it lands.

## Process
Build-and-stop → INDEPENDENT adversarial @rule8 review (separate party) → merge on
CLEAR + CI green. Test count monotone. cargo `--no-default-features`.
