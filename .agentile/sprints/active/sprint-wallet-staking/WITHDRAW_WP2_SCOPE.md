---
created: 2026-07-16
branch: feat/core-wallet-withdraw
author: Claude Opus 4.8, directed by @SaulBuilds
status: building — WP2 (withdraw path: request + claim + pending enumeration)
program: CORE finish-list item 1 (Withdraw) — the second leg of wallet liquid-staking
rule8: yes — two value-bearing writes (requestWithdrawal burns shares; claimWithdrawal pays SALT) + reads
grounded_in:
  - citrate-chain/contracts/src/LiquidStakingPool.sol lines 52-61 (struct/getter), 157-196 (request/claim), 101-102 (events), 307/323 (price/preview)
  - WP1 (merged #53): src-tauri/src/staking.rs (LIQUID_STAKING_POOL const, wallet_stake, read_self_stake) + staking_tests.rs
  - transfer.rs / staking.rs (ceremony money-write model), earnings.rs (eth_call read model), rpc.rs (transport)
---

# CORE — wallet Withdraw (finish-list item 1, WP2)

WP1 (#53, merged) made **Add stake** real. This makes **Withdraw self-added stake**
real. Withdraw is a TWO-STEP, ~7-day-QUEUED flow, so request + claim MUST ship
together (requestWithdrawal burns shares into a queue; without claim, funds strand).

## Grounded ABI (LiquidStakingPool.sol) — VERBATIM

- `struct WithdrawalRequest { address staker; uint256 shareAmount; uint256 saltAmount; uint256 requestBlock; bool claimed; }`
- `mapping(uint256 => WithdrawalRequest) public withdrawals;` (getter `withdrawals(uint256)` → the 5-field tuple, ABI-encoded as 5 words: addr(padded), uint, uint, uint, bool(0/1))
- `function requestWithdrawal(uint256 shareAmount) external returns (uint256 requestId)` — requires `shareAmount>0`, `shareAmount<=shares[msg.sender]`, `saltOut>0`, `saltOut<=totalPooled`. BURNS shares, `requestId = nextWithdrawalId++`.
- `function claimWithdrawal(uint256 requestId) external` — requires `req.staker==msg.sender`, `!req.claimed`, `block.number >= req.requestBlock + WITHDRAWAL_DELAY`. Pays `saltAmount` to caller.
- `uint256 public constant WITHDRAWAL_DELAY = 50400;` (~7 days at ~12s blocks)
- `mapping(address => uint256) public shares;` (getter `shares(address)` → raw stSALT shares)
- `function getSharePrice() public view returns (uint256)` — SALT per stSALT × 1e18
- `function balanceOf(address) external view returns (uint256)` — SALT value of shares (WP1 already reads this = self-stake SALT)
- `event WithdrawalRequested(uint256 indexed id, address indexed staker, uint256 shares, uint256 salt)` — topic0 = keccak256("WithdrawalRequested(uint256,address,uint256,uint256)"); topic1 = id; topic2 = staker.

### Selectors to PIN (derive+prove via keccak drift test, like staking.rs / earnings.rs)
CORRECTED 2026-07-16 against `cast sig` (foundry ground truth) — the first-draft
values below the arrows were HALLUCINATED by the grounding pass and were wrong for
4 of 5; the keccak drift test in staking_tests.rs caught it at build time. Use the
`cast`-confirmed values:
| fn | selector (cast sig, ground truth) |
|---|---|
| `requestWithdrawal(uint256)` | `0x9ee679e8`  (draft wrongly said 0xdf5dd1a5) |
| `claimWithdrawal(uint256)` | `0xf8444436`  (draft wrongly said 0x423b176f) |
| `withdrawals(uint256)` | `0x5cc07076`  (draft wrongly said 0xf39c38a0) |
| `shares(address)` | `0xce7c2ac2` |
| `getSharePrice()` | `0x5b1dac60`  (draft wrongly said 0xb3370044) |
| `WithdrawalRequested(uint256,address,uint256,uint256)` topic0 | `0xe6d14ce42ad5a4efe0f111f81c1a1123db4ee41f561c3161e0960d54a9221ebe` |
Canonical pool address (reuse the WP1 const): `0xfd272195b55cb4f5a240a5be75aabab0d1c5685e`.

## Design

### Rust — extend `staking.rs` (@rule8) + `rpc.rs`
1. **`rpc.rs::get_logs(filter: Value) -> Result<Vec<LogEntry>>`** — `eth_getLogs([filter])`. `LogEntry { topics: Vec<String>, data: String, block_number: u64 }`. Injected-transport testable (mirror the other rpc methods). Filter shape: `{address: pool, topics: [topic0, null, staker_padded_32], fromBlock: "earliest", toBlock: "latest"}` (topic2 = staker left-pads the 20-byte addr to 32).
2. **`wallet_request_withdrawal(amount_wei)` command (@rule8)** — SALT→shares conversion done in Rust from live reads (no fabricated numbers):
   - read `shares[self]` (shares(address)) and self-stake SALT (`balanceOf(self)`, reuse read_self_stake).
   - if `amount_wei == 0` → err. if self-stake SALT `== 0` → err "no self-stake to withdraw".
   - if `amount_wei >= selfStakeSalt` → `shareAmount = shares[self]` (withdraw ALL; exact, no dust).
   - else `shareAmount = amount_wei * shares[self] / selfStakeSalt` (u128/U256-safe floor; proportional so it always `<= shares[self]` — avoids the "Insufficient shares" revert and share-price rounding drift). If it computes to 0 → err.
   - build ceremony `Transaction` intent: `to=pool`, `data = requestWithdrawal(shareAmount)` (selector ++ 32-byte shareAmount), `value=0`, explicit `gas` (~150000), chainId 40204. Return CeremonyView. Signs nothing (human approves via sign_and_broadcast). Mirror `wallet_stake`.
3. **`wallet_claim_withdrawal(request_id)` command (@rule8)** — build ceremony intent: `to=pool`, `data = claimWithdrawal(request_id)`, `value=0`, explicit `gas` (~90000). Return CeremonyView.
4. **`read_pending_withdrawals(rpc, addr) -> Vec<PendingWithdrawal>`** — enumerate via getLogs(WithdrawalRequested, staker=addr) → ids (topic1). For each id: `withdrawals(id)` eth_call → decode the 5-word tuple (staker, shareAmount, saltAmount, requestBlock, claimed). Read `block_number()` once. `PendingWithdrawal { id: String, saltWei: String, requestBlock: u64, claimableAtBlock: u64 (=requestBlock+50400), claimable: bool (currentBlock>=claimableAtBlock), claimed: bool }`. EXCLUDE `claimed==true`. Serde camelCase.
5. Register `staking::wallet_request_withdrawal`, `staking::wallet_claim_withdrawal`, and a `wallet_pending_withdrawals` command (reads self addr from custody, calls read_pending_withdrawals) in lib.rs.

### Frontend — bridge/store/Wallet
- `bridge.wallet.requestWithdrawal(amountWei)`, `claimWithdrawal(id)`, `pendingWithdrawals()`. tauri → invoke; sim → honest Unavailable / empty list.
- `store.walletRequestWithdrawal(wei)` + `store.walletClaimWithdrawal(id)` mirror `walletStake` (bridge → signing.broadcast → refresh). Add `pendingWithdrawals` to AppState + a `refreshPendingWithdrawals()` (fold on Wallet mount + after settle). It IS persistable (not PII) but the source of truth is chain — re-read, don't rely on persistence.
- `onUnstake` (Wallet.tsx) → `store.walletRequestWithdrawal(wei)` (strict decimal→wei like onAddStake; keep the `amt > s.selfStake` guard → "Only self-added stake can be withdrawn — granted principal is vaulted").
- New **Pending withdrawals** panel in the Staking tab: one row per pending withdrawal (amount SALT, status: "claimable now" or "~N blocks (~Xd) left", a Claim button enabled only when `claimable`). Claim → `store.walletClaimWithdrawal(id)`.
- Honest copy: withdrawing shows "requested — SALT unlocks after ~7 days (50,400 blocks), then Claim." No fabricated instant settlement.

## Acceptance
- onUnstake builds a real `requestWithdrawal` ceremony (correct shareAmount from live reads); on approve it broadcasts a real 40204 tx.
- Pending withdrawals list reflects real on-chain `withdrawals(id)` state + real block height; Claim is gated by the real 50400-block delay; Claim builds a real `claimWithdrawal` ceremony.
- No fabricated hash/number; a fresh wallet shows an empty pending list honestly.

## Non-negotiables (from citrate-core CLAUDE.md)
- Rule 3: every signature via the SignatureCeremony; commands sign nothing, return CeremonyView/reads only.
- Rule 1: name every data source in comments; no fabricated numbers; honest empty/failure states.
- Zero `.unwrap()` in prod (use `?`/`ok_or_else`); `.expect("ctx")` in tests only.
- Selectors PINNED + a keccak-derivation drift test (Rule 11). Explicit gas on every calldata tx (finalize has no estimate path).
- Test count monotone. cargo `--no-default-features`. Build-and-stop; NO self-review; NO merge.
