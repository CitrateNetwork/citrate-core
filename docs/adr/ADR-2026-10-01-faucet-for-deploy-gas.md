---
created: 2026-10-01T09:00:00Z
branch: hup/n3-faucet-adr-literacy
author: Larry Klosowski + Claude Opus 5.5
status: proposed
planset: 2026-09-30-hermes-upskill
wp: HUP-S6.8
decisions: D-3 (gas), D-31 (faucet eligibility per member)
supersedes: ADR-2026-07-27-membership-stakes-the-validator-bond.md, the "No faucet" statement, for deploy gas only
relates_to: ADR-2026-09-30-rule3-budgetable-signatures.md, ADR-2026-09-30-hermes-loop-in-sidecar.md
blocks: HUP-S6.5 (faucet integration)
---

# ADR-2026-10-01: a faucet for deploy gas, routed through core

## Status

**Proposed.** Nothing in this ADR is implemented. It records the design for HUP-S6.5 and a cost
table measured against chain 40204 on 2026-10-01. Until the owner accepts it, the in-app faucet
stays off: core sends no faucet request, and members fund deploys from their own SALT exactly as
today.

## Context

### What ADR-2026-07-27 said

[ADR-2026-07-27](ADR-2026-07-27-membership-stakes-the-validator-bond.md) routes the membership
grant (32k SALT) to the member's wallet as the validator bond and says, under "Why no faucet /
no paymaster-for-stake", that a faucet is unnecessary because the treasury already sources the
grant. That reasoning is about **the bond**: a faucet cannot and should not supply 32k SALT of
`msg.value`.

### What changed

The Hermes upskill planset (D-3) adds a dApp forge: a member asks Hermes to build the hello-mint
ERC-721, and the app deploys it from the member's wallet through the SignatureCeremony. A deploy
needs gas. A member who has just bonded the grant can be left with little or no liquid SALT, so the
first deploy can fail for want of a fraction of a SALT. D-3 decided to integrate the existing
faucet into the node flow for exactly this case, and D-31 says faucet eligibility is per member,
not per device.

### The existing faucet

`citrate-chain/faucet` (the service behind `https://faucet.citrate.ai`, which answered
`{"amount_per_request":"10 SALT","network":"citrate-testnet-beta","status":"online"}` on
2026-10-01) already has the abuse controls a public faucet needs:

- `POST /faucet {address}` sends a fixed **10 SALT** (`DRIP_AMOUNT`) from the faucet's own hot
  wallet, as a legacy transaction at 1 gwei.
- Cooldowns: **24 h per recipient address and 1 h per client IP**, reserved atomically before the
  send (`faucet/src/cooldowns.rs`). They are persisted to disk, so a restart does not reset them,
  only when the deployment sets `FAUCET_COOLDOWN_FILE`; otherwise they live in memory.
- An optional CAPTCHA (`FAUCET_TURNSTILE_SECRET`; when set, a request without a valid
  `turnstile_token` is refused), a CORS origin allowlist, an optional address allowlist (`FAUCET_WHITELIST`), and a
  trusted-proxy list for the client IP.

The faucet's key is an operations key of that service. It is not a member key and is never in
the app or the sidecar.

## Decision (proposed)

### D1. Scope: deploy gas only

The in-app faucet exists to cover **gas for actions the member signs themselves**, starting with
deploying a contract from the dApp forge and its first follow-up calls. It never funds:

- the validator bond or any other `msg.value` (ADR-2026-07-27 stands for the bond);
- the anchor key (Rule-3 ADR D5 and O-5 decide how anchor gas is paid; the anchor key holds no
  member funds);
- a third-party address, a contract, or any address the model or the sidecar supplies.

ADR-2026-07-27's "No faucet" statement is superseded for deploy gas and nothing else.

### D2. Who pays

The faucet's hot wallet pays the drip. Citrate operations fund that wallet (on testnet-beta, from
the treasury). The member pays the deploy gas from the dripped SALT, signing in their own
ceremony. No member, device, app or sidecar key ever pays for or signs a drip.

### D3. How it routes (no sidecar keys)

```text
Hermes (sidecar)          citrate-core                         faucet.citrate.ai        chain 40204
   | faucet_request ------>|                                         |                       |
   |  (node MCP tool,      | 1. recipient := the member's active     |                       |
   |   no arguments that   |    wallet address, read by core         |                       |
   |   choose a recipient) | 2. need := upfront cost of the pending  |                       |
   |                       |    deploy (estimate x gas price)        |                       |
   |                       |    balance >= need: no request          |                       |
   |                       | 3. HIC-2 budget check (D4)              |                       |
   |                       | 4. POST /faucet {address[, token]} ---->| 24 h / 1 h cooldowns  |
   |                       |<---- tx hash, or "next eligible at" ----| signs with its own    |
   |                       |                                         | key, sends 10 SALT -->|
   | deploy_propose ------>| 5. DeployGate binds hash(initcode, args,|                       |
   |                       |    solc settings) to the gated artifact |                       |
   |                       | 6. SignatureCeremony, HIC-1: the member |                       |
   |                       |    approves this exact CeremonyId ------------------------------>| deploy tx
```

- The faucet request is an unsigned HTTP call. It carries no signature, so the closed list of
  budgetable signatures in the Rule-3 ADR is unchanged.
- The deploy is a transaction and stays **HIC-1** (Rule-3 ADR D1: deploys are never budgetable).
- The sidecar can ask for a top-up; it cannot choose the recipient, the amount, or the time, and
  it never sees a key. Core decides and performs the request.

### D4. Abuse limits on the app side

On top of the faucet's own limits, core enforces:

1. **Need-gated.** Core requests a drip only when the member's balance is below the upfront cost
   of a deploy the member has started (gas limit x gas price, from the table below), never
   speculatively.
2. **Once per member per 24 h.** The budget is per member (D-31): all of a member's devices share
   one wallet address and one cooldown. Core shows the faucet's "next eligible" time instead of
   retrying (US-6.5 AC2).
3. **HIC-2 budget.** The member grants the faucet budget once through an HIC-1 ceremony (scope:
   faucet top-ups for deploy gas; cap: one per 24 h; revocable in Settings). Each top-up is
   recorded in the decision log and shown in the activity monitor.
4. **No silent fallback.** If the faucet refuses (cooldown, CAPTCHA, outage), Hermes says so and
   the member funds the deploy themselves. Nothing retries in a loop.

### D5. Default

Off until this ADR is accepted. With the setting off, `faucet_request` reports that the in-app
faucet is not enabled and points the member at the faucet page. Turning it on is the HIC-1 budget
grant in D4.3.

## Measured cost table (chain 40204)

Read-only measurements, 2026-10-01 between 08:36 and 08:50 UTC, blocks 61,570 to 61,978. No
transaction was sent to chain 40204.

**Gas price.** `eth_gasPrice` = 1,000,000,000 wei (1 gwei). `baseFeePerGas` = 1 gwei on every
block sampled, `eth_maxPriorityFeePerGas` = 1 gwei, `eth_feeHistory` priority rewards 0 and
`gasUsedRatio` 0 (the chain was idle). Core's ceremony signs legacy transactions at
`eth_gasPrice`, so cost = gas x 1 gwei, and 1 SALT = 10^18 wei. At 1 gwei, 1,000,000 gas costs
0.001 SALT.

**Two columns of gas.**

- *Gas used* is standard EVM accounting, measured by executing the same calls on a local anvil
  fork of 40204 at block 61,598 (hardfork Cancun, which is what the Citrate executor uses).
- *Node estimate* is what the 40204 node's `eth_estimateGas` returns, which is the gas limit a
  wallet will set. The node pads it: +10 % for a deploy, and `max(2 x used, used + 50,000)` for
  any state-changing call, to cover refunds. The member's balance must cover gas limit x gas
  price before the transaction is accepted, so the node estimate sets the funding need.
- Calibration: for every call measured both ways, the node's simulated gas was the standard figure
  **+700** (anchor, SBT mint), and **+32,000** for the contract creation. These measurements do
  not show which figure the chain bills; budget with the node's.

| Action | Contract on 40204 | Paid by | Gas used (standard EVM) | Node estimate (gas limit) | Cost at 1 gwei (used) | Balance needed up front |
|---|---|---|---|---|---|---|
| Deploy hello-mint ERC-721 (initcode 6,460 bytes) | (new) | member | 1,409,540 | 1,585,694 (live) | 0.00141 SALT | 0.00159 SALT |
| Verify source | n/a | nobody | 0 (no transaction) | n/a | 0 | 0 |
| `mint(1)`, first token of the collection | (new) | minter | 93,753 | about 188,906 (projected) | 0.000094 SALT | about 0.00019 SALT |
| `mint(1)`, holder's next token | (new) | minter | 59,553 | about 120,506 (projected) | 0.00006 SALT | about 0.00012 SALT |
| `mint(10)` | (new) | minter | 287,802 | about 577,004 (projected) | 0.00029 SALT | about 0.00058 SALT |
| `setBaseURI(ipfs://…)` | (new) | member (owner) | 116,454 | about 234,308 (projected) | 0.00012 SALT | about 0.00023 SALT |
| `CitrateMemberSBT.mintMember` | `0xA24aa35fbA269f8755C2173779cc3DBC9690c4C9` (code 10,219 bytes) | membership orchestrator (contract owner), not the member | 166,225 | 333,850 (live, from the owner address) | 0.00017 SALT | 0.00033 SALT |
| `AgentSBT.mintAgent` | `0xd16b1ad6e744F3E92223C65F492c35D36ae07c7b` (code 4,897 bytes) | contract owner | not measurable today | not measurable today | n/a | n/a |
| `AnchorRegistry.anchor(NightlyMerkle, root)`, first root of the kind | `0x41e0f9A4dCD29C650dc58Ee569BF267fD9ba4817` (code 1,286 bytes) | anchor key (Rule-3 ADR O-5) | 179,465 | 360,330 (live, from a zero-balance address) | 0.00018 SALT | 0.00036 SALT |
| `AnchorRegistry.anchor`, a later root | same | anchor key | 162,365 | about 326,130 (projected) | 0.00016 SALT | about 0.00033 SALT |
| Plain SALT transfer | n/a | sender | 21,000 | 21,000 | 0.000021 SALT | 0.000021 SALT |

"Live" is the 40204 node's own `eth_estimateGas`. "Projected" applies the node's padding rule and
the +700 calibration to the standard figure, because the node cannot estimate a call into a
contract that is not deployed (it ignores state overrides).

Notes on the rows:

- **Hello-mint contract.** The `erc721` template from `hup/n3-templates` (OpenZeppelin v5.7.0 at
  commit `cab1993`, solc 0.8.36, EVM Cancun, optimizer 200 runs), rendered with name "Hello Mint",
  symbol "HELLO", supply 500, price 0, owner `msg.sender`. A different name or symbol changes the
  deploy by a few hundred gas.
- **Verify.** Source verification is the explorer's job plus the node's `citrate_verifyContract`
  RPC, which is gated by an operator token. It is not a transaction and costs the member nothing.
- **Member SBT.** The contract is on chain and `mintMember` is `onlyOwner`. Estimated from the
  owner address `0xA5d097C0abbb3B6Bbdb75bD06d51B44F9277E6d4` with a throwaway sub hash; from any
  other address it reverts `OwnableUnauthorizedAccount`. The member does not pay it, so it is out
  of the faucet's scope.
- **AgentSBT.** The contract is on chain, but `mintAgent` is `onlyOwner` (the owner is
  `CitAgentTimelock`, `0xBaC05BC639af6eF107F40fe606f1c4A22b7836A2`) and needs an active
  OrganizationSBT. `OrganizationSBT.nextTokenId()` is 0 at `0xB1Bb65Fc3F2188Ff1209845cBe64eba985461689`,
  so no organisation exists and every `mintAgent` would revert `OrgNotActive`. S7.4 ("AgentSBT
  mint at onboarding") therefore needs an organisation and a minting path first. The member
  would not pay this mint either.
- **Anchor.** `anchor()` is append-anyone and estimation from an address with zero balance
  works: the node simulates with the sender funded. The dummy root was
  `keccak256("citrate-hup-s6.8-estimate-only")`, which was not anchored. A nightly anchor costs
  about 0.00018 SALT, about 0.066 SALT a year at today's price.

### What this means for the faucet

- The member's whole hello-mint path (deploy, set the base URI, one test mint) needs about
  **2.0 million gas of limit, about 0.002 SALT up front**, and uses 1.62 to 1.65 million gas (standard and node figures).
- One 10-SALT drip covers that path about **5,000 times** at today's price. Deploy gas is not
  what drains a faucet; the drip size is. That is why D4 gates on need and on one drip per member
  per day, and why O-1 asks whether a smaller gas-only drip is worth a faucet change.
- The price is flat today because the chain is idle. The table states a block and a date; the
  app must re-estimate before every request (D4.1) rather than reuse these numbers.

## Consequences

- Members who bonded their grant can still deploy their first contract without buying SALT.
- No signing path changes. The faucet request is unsigned, the deploy stays HIC-1, the sidecar
  stays keyless.
- The faucet's hot wallet and its limits become a dependency of the dApp forge. A faucet outage
  degrades to "fund it yourself", stated plainly.

## Open questions for the owner

| # | Question | Proposed answer |
|---|---|---|
| O-1 | Keep the 10-SALT drip, or add a smaller gas-only drip for in-app requests? | Keep 10 SALT for 0.5.0 (no faucet change). Revisit after real usage data. |
| O-2 | Should the faucet check membership (for example, the member SBT) before dripping, to make D-31 hold server-side? | Yes, eventually. It is a faucet change in citrate-chain plus a deploy by the DGX team; until then core enforces per-member limits on the app side only. |
| O-3 | If the faucet turns its CAPTCHA on, how does the desktop app pass it? | An in-app challenge window on an allowlisted origin (US-6.5 AC1), or a member-signed request that the faucet accepts instead of the CAPTCHA. Needs a faucet change either way. |
| O-4 | Where does the in-app faucet live in Settings, and is it on by default after acceptance? | Off by default; the member turns it on with the HIC-1 budget grant. |

## Method (reproducible, read-only)

```sh
RPC=https://rpc.citrate.ai
cast chain-id --rpc-url $RPC; cast block-number --rpc-url $RPC; cast gas-price --rpc-url $RPC
cast rpc eth_feeHistory 0x5 latest '[]' --rpc-url $RPC
# hello-mint: render templates/erc721/files/src/Token.sol from hup/n3-templates, forge build
cast rpc --rpc-url $RPC eth_estimateGas '{"from":"0x000000000000000000000000000000000000c17a","data":"<initcode>"}'
cast estimate --rpc-url $RPC --from 0x000000000000000000000000000000000000c17a \
  0x41e0f9A4dCD29C650dc58Ee569BF267fD9ba4817 "anchor(uint8,bytes32)" 2 <root>
cast estimate --rpc-url $RPC --from 0xA5d097C0abbb3B6Bbdb75bD06d51B44F9277E6d4 \
  0xA24aa35fbA269f8755C2173779cc3DBC9690c4C9 "mintMember(address,bytes32,uint64,uint64)" <to> <subHash> <start> <end>
# standard-EVM gas: a local anvil fork; transactions here go to the fork only
anvil --fork-url $RPC --fork-block-number 61598 --hardfork cancun --chain-id 40204 --port 18545
```

The fork transactions (deploy, mints, `setBaseURI`, anchors, the SBT mint impersonating its
owner) ran against the local anvil process only and never reached chain 40204.
