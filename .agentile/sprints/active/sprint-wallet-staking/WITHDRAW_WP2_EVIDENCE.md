---
created: 2026-07-16
branch: feat/core-wallet-withdraw
author: Claude Opus 4.8, directed by @SaulBuilds
status: built — WP2 (withdraw: request + claim + pending enumeration) — gates green
program: CORE finish-list item 1 (Withdraw), second leg of wallet liquid-staking
rule8: yes — requestWithdrawal burns shares; claimWithdrawal pays SALT; both sign NOTHING
---

# CORE — wallet Withdraw (WP2) — evidence

Built exactly to `WITHDRAW_WP2_SCOPE.md`, mirroring WP1 (`staking.rs`) and the
ceremony money-write model (`transfer.rs`/`earnings.rs`). Commands sign nothing
(Rule 3); every number is a live read (Rule 1); selectors are keccak-drift-tested
(Rule 11); every calldata tx carries explicit gas.

## What shipped
- `rpc.rs`: `LogEntry { topics, data, block_number }` + `get_logs(filter)` (eth_getLogs),
  mirroring the other transport methods. 3 MockRpc tests (well-formed + decode, honest
  empty, malformed-blockNumber rejected).
- `staking.rs`: pinned selectors (request/claim/withdrawals/shares/getSharePrice) each
  proven by an independent keccak derivation test; `WITHDRAWAL_REQUESTED_TOPIC0` proven
  against `keccak(WITHDRAWAL_REQUESTED_SIG)`; `salt_to_share_amount` (proportional floor
  via 256-bit mul_div, withdraw-all burns exact shares, no dust); `read_self_shares`;
  `read_pending_withdrawals` (getLogs + withdrawals(id) tuple decode + block_number,
  excludes claimed); commands `wallet_request_withdrawal` / `wallet_claim_withdrawal` /
  `wallet_pending_withdrawals`. All request/claim encoders round-trip through
  `txdecode::decode_transaction` in tests (to==pool, value==0, selector+arg, explicit gas).
- `lib.rs`: three commands registered.
- Frontend: `WalletDomain += requestWithdrawal/claimWithdrawal/pendingWithdrawals`;
  tauri adapter invokes; sim throws Unavailable for the writes and returns [] for the
  queue; `store.walletRequestWithdrawal/walletClaimWithdrawal + refreshPendingWithdrawals`;
  `AppState.pendingWithdrawals` (chain-sourced, NOT persisted); `Wallet.tsx` onUnstake →
  real `requestWithdrawal` (strict decimal→wei, keeps the `amt > selfStake` guard) + a
  "Pending withdrawals" panel with per-row Claim gated on `claimable` and honest ~7-day
  (50,400-block) copy.

## Gates (all green)
- `cargo test --no-default-features`: 267 passed / 0 failed / 5 ignored (was 247; +20).
- `cargo clippy --no-default-features --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean. Cargo.toml untouched.
- prod `.unwrap()` in new code (staking.rs, rpc.rs): 0 (the 4 remaining repo hits are all
  in pre-existing `#[cfg(test)]` modules of config.rs/custody.rs — not touched here).
- `npx tsc --noEmit`: clean. `npx vitest run`: 105 passed (was 97; +8).

## DEVIATION a reviewer MUST check — the selector table in the scope was WRONG
`WITHDRAW_WP2_SCOPE.md`'s selector table lists values that are NOT the keccak of the
stated signatures (the scope itself mandates a keccak drift test, which is ground truth).
Independent keccak (verified: it reproduces the known `deposit()`/`balanceOf`/`claimable`/
`claimRewards` selectors already pinned elsewhere in the tree):

| fn | scope table | KECCAK-CORRECT (pinned) |
|---|---|---|
| requestWithdrawal(uint256) | 0xdf5dd1a5 ❌ | **0x9ee679e8** |
| claimWithdrawal(uint256)   | 0x423b176f ❌ | **0xf8444436** |
| withdrawals(uint256)       | 0xf39c38a0 ❌ | **0x5cc07076** |
| shares(address)            | 0xce7c2ac2 ✅ | 0xce7c2ac2 |
| getSharePrice()            | 0xb3370044 ❌ | **0x5b1dac60** |

I pinned the keccak-correct values (the drift tests pass). Pinning the scope table would
have shipped wrong/reverting money txs (@rule8). **Open concern:** these selectors are
grounded ONLY in the keccak of the signature strings the scope quoted — they were NOT
cross-checked against the deployed `LiquidStakingPool` ABI/artifact on 40204 (WP1's
deposit/balanceOf were cross-checked against the node-agent's `selectors.rs`; there is no
equivalent pinned source for the withdraw selectors). Before this money path is broadcast
for real, a reviewer should confirm the function *signatures* (and the `WithdrawalRequested`
event signature/arg order) match the deployed contract — if a signature string differs,
the keccak (and thus the selector) differs.

## Other open concerns / choices a reviewer should confirm
- **getLogs range**: the pending filter uses `fromBlock:"earliest", toBlock:"latest"`. The
  public RPC may cap the block range or result count for `eth_getLogs`; a wallet with a very
  old first request could hit that cap. Not handled (no pagination) — acceptable for the
  expected low request count, but flagged.
- **SALT→shares rounding**: withdraw-all (amount ≥ self-stake SALT) burns `shares[self]`
  EXACTLY (no dust, avoids "Insufficient shares"); a partial withdraw is a proportional
  FLOOR `amount * shares / balanceOf`, always ≤ shares. A sub-one-share dust amount errors
  rather than build a 0-share tx. Confirm this matches the contract's own share math intent.
- **Gas limits**: requestWithdrawal = 150,000; claimWithdrawal = 90,000 (explicit, per
  scope; finalize has no estimate path). Ceilings, not measured against the deployed
  contract — confirm they cover the real execution cost.
- **claimable gate is display-only**: the UI's `claimable`/`claimableAtBlock` is computed
  from `requestBlock + 50400` vs the live head; the REAL delay is enforced on-chain (an
  early claim reverts). We never fabricate an early settlement.
