---
created: 2026-07-13
branch: feat/core-c2-earnings
author: Claude Opus 4.8, directed by @SaulBuilds
status: built — build-and-stop → INDEPENDENT review (@rule8)
sprint: CORE-C1 / C2 (earnings: real claimable + claim through the bridge → ceremony → claimRewards)
grounded_in:
  - citrate-chain/contracts/src/ContributionAccounting.sol — claimable(address) mapping getter;
    claimRewards() (line 218) zeroes claimable[msg.sender] + transfers out; NO per-source breakdown.
  - citrate-node-agent/crates/chainio/src/selectors.rs — claimable 0x402914f5, claim_rewards 0x372500ab;
    crates/chainio/src/accounting.rs (decode_claimable = u128); crates/chainio/src/generated/addresses.json
    (ContributionAccounting = 0xcdd2477387279c7d44a1053f44db5dac0fd8faef).
  - src-tauri/src/agent.rs (C1.2 bridge), ceremony.rs + rpc.rs (B1.4), src/surfaces/Node.tsx Earning tab,
    src/bridge/domains.ts AgentDomain.
depends_on:
  - C1.2 (node-agent bridge) built, Phase B (ceremony + B1.4 broadcast) COMPLETE.
---

# C2 — earnings: real claimable + claim through the ceremony @rule8

## Grounded ContributionAccounting ABI facts
- `mapping(address => uint256) public claimable;` → getter `claimable(address) returns (uint256)`,
  selector `0x402914f5` (a single balance in wei of SALT). File: ContributionAccounting.sol:49.
- `function claimRewards() external` (selector `0x372500ab`, no args): zeroes `claimable[msg.sender]`,
  `distributed[msg.sender] += amount`, transfers `amount` out via `call{value: amount}`. File: :218-230.
  It is a SIGNED value-bearing write (the contract PAYS the caller; the tx itself sends 0 SALT + gas).
- **NO per-SOURCE breakdown on-chain.** Contributions are tracked per `ContributionType`
  (`contributions[addr][ctype]`, 7 types) but `claimable` collapses to ONE `uint256`. There is no
  `claimable(address, ContributionType)`. So the Earning tab's validation/pinning/compute split has
  no live data source → DROPPED/labeled (Rule 1 / I-3), never fabricated.

## Work packages (built)
- **WP1 — real claimable poll.** `earnings::read_claimable` reads
  `ContributionAccounting.claimable(vaultAddress)` via a new `rpc::eth_call({to,data},"latest")` on
  40204 and decodes the uint256 (u128-safe, REFUSES to truncate a >u128 return). Command
  `agent_earnings` reads the vault wallet's public address (never the key) and returns the single real
  claimable (wei) + its contract data source. Selectors are PINNED constants cross-checked against a
  keccak derivation AND the node-agent's values (Rule 11 drift tripwire). Frontend: AgentDomain.earnings()
  (tauri invoke / sim wei-shape), Node.tsx Earning tab reads it on tab-open, labels the decomposition
  "off-chain estimate — not in claimable", captions Claimable with its eth_call source.
- **WP2 @rule8 — claim through the bridge + ceremony.** The user Claim button and the node-agent's
  ClaimRewards request emit the SAME unsigned `claimRewards()` intent (earnings::user_claim_request),
  routed through agent.rs bridge → SignatureIntent{origin:"agent:node-agent", to:ContributionAccounting,
  calldata:0x372500ab} → ceremony (human approve, true action+destination shown) → approve_and_broadcast
  (B1.4). **Folds in C1.2-F-1 (LOW):** a per-request-id dedup map so a still-pending node-agent request
  maps to AT MOST ONE ceremony → ONE broadcast; cleared after broadcast/stop.
- **WP3 — proof.** CI-safe: mocked eth_call claimable decode; the dedup invariant with a negative
  control; the claim→ceremony→claimRewards-calldata path (mocked RPC). Live: the REAL claimable read on
  40204 (documented value 0 for a fresh address).

## Acceptance status
- claimable ABI decode + selector cross-check (0x402914f5 / 0x372500ab) ✔
- real claimable read over eth_call (mocked) + zero-for-fresh + rpc-error + bad-address fail-closed ✔
- user claim request shape == node-agent shape; bridges to a legible ceremony intent ✔
- dedup: two bridges of one still-pending request → ONE ceremony → ONE broadcast (RED negative control) ✔
- dedup entry clears after broadcast so a re-accrual bridges fresh ✔
- LIVE (documented): eth_call claimable(0x9858…) on 40204 = 0 wei (honest; non-contributor address) ✔
- Gates: cargo test 188→204 (+1 ignored live = 4), fmt/clippy -D/audit-0-vuln/no-new-deps,
  npm typecheck/test 55→57/build — all green.

## Honest gaps (stated, NOT acted on)
- **Confirmed claim tx is a GAP.** A broadcast claimRewards() needs a node with ACCRUED earnings
  (ContributionAccounting only credits contributors) AND an unlocked funded vault. The real read is 0
  for a fresh node, so there is nothing to claim — NOT fabricated (Rule 1). Exact command once earnings
  exist + the vault is unlocked: in-app Node → Earning → Claim (routes ceremony → B1.4), or headless
  `mgr.bridge_one_pending(..) → approve_bridged_and_report(..)`.
- **Node.tsx surface still reads sim `s` for the decomposition cards** (earnVal/earnPin/earnComp). Those
  are now visibly labeled off-chain estimates; the single Claimable is the real chain read folded in via
  store.refreshEarnings(). Fully removing the sim decomposition fields is a later cosmetic pass.
- **`agent_earnings` uses the live-RPC RpcClient::citrate()** (rpc.citrate.ai), not the LOCAL spawned
  node's RPC — claimable is a chain-wide read, so either is correct; noted for consistency review.
