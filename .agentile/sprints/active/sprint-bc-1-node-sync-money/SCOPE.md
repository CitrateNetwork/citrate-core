---
title: "Sprint BC-1 — Node sync truth + S5 live-money wiring"
created: 2026-07-19
branch: sprint/bc-1-node-sync-money
author: Claude (Opus 4.8) for SaulBuilds
status: active
planset: citrate-federation/.agentile/planset/2026-07-19-core-beta-completion/
---

# Sprint BC-1 — Node sync truth + S5 live-money wiring

The two highest-value gaps toward the beta goal (see planset §1 G-1, G-2). @rule8 on
BC-1.3 (money path) and a data-dir-encryption regression check on the node rebuild.

## Grounding established 2026-07-19 (verified, not assumed)
- **Chain 40204 is live post-reroll:** genesis hash `0x481a59bc8826c91cd05d897fafff1bce4c394e41093f5e1c10e308e6c7d748fb`,
  height ~71.6k, ~1 block/3s (`curl eth_getBlockByNumber 0x0` + `eth_blockNumber`, rpc.citrate.ai).
- **The bundled node binary is stale:** `src-tauri/binaries/citrate-aarch64-apple-darwin`
  dated 2026-07-13, built BEFORE the reroll → its compiled genesis cannot match the live chain.
- **citrate-chain `main` (HEAD `ee1f9c1`) added the sync fix after the reroll:** the
  `ops/reroll-deterministic-operator-keys` merge (`aa65acf`) then **execute-on-receive
  steps 1–6** (reorg, gap-extend, forward-drain, two-node divergence harness,
  "generalize VALIDATOR-S1 sync") + **VALIDATOR-S1** (ValidatorRegistry contract +
  ed25519-verify precompile + stake-gated proposer membership). This is almost certainly
  the fix for the journal-documented "node connected but height stayed 0."
- **Conclusion:** the 07-13 binary fails for two reasons (wrong genesis + no
  execute-on-receive). A rebuild from `main@ee1f9c1` is necessary and likely sufficient,
  and it also delivers the ValidatorRegistry that BC-1.4 reads. Node-source pin advances
  to `ee1f9c1` (Rule-12 drift entry).

## Work packages

### BC-1.0 — Rebuild node from post-reroll main  ✅ DONE 2026-07-19
- Correct source found: the live chain was deployed from `fix/ghostdag-select-tip-determinism`,
  merged to main as **PR #83 = `eb26e8f`**. (My first build from `ee1f9c1` had the STALE 7-account
  `testnet_beta` genesis `0x6b6d8b89`; the reroll-integration branch has 11 accounts → `0x481a59bc`.)
- Built `citrate` bin from `eb26e8f`, dropped into `src-tauri/binaries/citrate-aarch64-apple-darwin`
  (arm64; x86_64 in BC-7). **Genesis VERIFIED == live `0x481a59bc`** and peers connect (4 peers).
- Node-source drift pin ADVANCED `2d3d8ea` → `eb26e8f` (citrate-federation manifest.toml).
- CAVEAT: full sync-to-head blocked by a citrate-chain forward-sync bug (see BC-1.1 finding);
  binary is still strictly correct to bundle (right genesis + peering).

### BC-1.1 FINDING 2026-07-19 — BLOCKED upstream (WO-1 escalated)
Rebuilt node from `ee1f9c1` (BC-1.0 done, binary at citrate-chain `target/release/citrate`).
Ran a fresh node vs live testnet: TCP connects to all 4 boot peers incl. sequencer
(`rpc.citrate.ai`), **Noise trust root verifies for all 4** (static keys match — reroll didn't
rotate them), but every handshake dies with `Transport error: eof` right after → **peers=0,
height=0 for 90s**. Root cause (confirmed by diff): genesis schema changed AFTER the reroll
commit `aa65acf` (deployed) — `create_canonical_genesis_block` added `coinbase:[0u8;20]`
(commit `5c7d86b`) + state-layer changes → HEAD genesis ≠ deployed genesis `0x481a59bc…` →
deployed peers reject us post-handshake. The deployed network predates execute-on-receive
(fresh-sync) AND ValidatorRegistry, so **no onboarded node can join or validate as deployed.**
Handoff: `citrate-labs/handoffs/CITRATE_CORE_NODE_SYNC_WO1_2026-07-19.md`. Recommendation:
Option A — re-cut testnet from current `main`, then pin+rebuild the bundled node to match.
BC-1.1 acceptance blocks on that. BC-1.3/BC-2 proceed in parallel (independent of peering).

### BC-1.1 — Fresh-node sync proof  **(the gate)**
- Start the rebuilt node against 40204 with the real boot config (confirm the sequencer
  entry `rpc.citrate.ai:30303` is present — boot1/2/3 advertise height 0, so the sequencer
  is what triggers a fresh 0→head sync). Prove Noise handshake with ≥1 peer + height
  advancing toward head.
- **Red harness:** bounded automated proof (stub-node in CI + scripted real run) asserting
  `eth_blockNumber` strictly increases within N seconds and approaches head.
- **Escalation:** if the rebuilt node still stalls at 0, capture a repro and file a
  citrate-chain WO-1 upstream fix; BC-1.1 acceptance blocks on it.
- **Acceptance:** documented full 0→head run (wall-clock + block count) + automated proof green.

### BC-1.2 — Kill simulated node vitals
- Node surface + sidebar read `bridge.node.status()` (real), never `AppState` walk;
  disable `store.tick()` node animation in Tauri mode.
- Fix stale `boot-eu1.citrate.ai:30303` string (`src/surfaces/Settings.tsx:508`) → authoritative list.
- Poll node-agent supervision API (127.0.0.1:19600) in S6.
- **Red test:** fails if Node surface renders a non-bridge height in Tauri mode.

### BC-1.3 — S5 live grant/stake wiring (D3)  **@rule8**  *(building 2026-07-19)*
- Replace `onS5Begin`/`tick` fake 32k counter with real polling: vault
  `attributedShares(member)`, SBT `balanceOf(member)==1`, entitlement from `/userinfo`.
  `hasGrant/hasSbt` set ONLY from chain/authority reads; bounded poll; honest "settling".
- **Red test (negative control):** stub the attribution read to 0 → S5 must stay "settling",
  never reach "settled".

**Grounded facts (verified 2026-07-19 vs live 40204 + 40204.json):**
- MembershipStakeVault = `0x0aceb7B474eCC4abe12696CE48628f0CABE0267e` — getters
  `attributedShares(address)→uint256` and `attributedStake(address)→uint256` (both `view`);
  `VALIDATOR_STAKE_REQUIREMENT()→uint256` (= 32000e18). A granted member reads attributedShares > 0.
- CitrateMemberSBT = `0x7bE005aA8c45C1695b4C75468c6cA8B40238A7C4` — `balanceOf(address)→uint256`
  (selector `0x70a08231`); ==1 for a member.
- Pattern to mirror EXACTLY: `src-tauri/src/staking.rs` (eth_call + `decode_uint256_word` +
  `validate_address` + injectable `RpcClient` + PINNED selectors with keccak drift tests).
- COUPLING w/ BC-2: citrate-core must read the SAME vault/SBT core-membership grants to. These are
  the canonical 40204.json (post ca9599a reconciliation) addresses; BC-2 must confirm
  core-membership Vercel env grants to these exact addresses (memory had older 0x94c0A5/0x16041D).
- Member address = the wallet address (`wallet::address` from custody) OR the `/userinfo`
  `wallet_address` claim; use the same address the grant targets (the AA smart-wallet address).

### BC-1.4 — Validator status honesty  *(reads WO-1 / ValidatorRegistry)*
- Read validator-set membership + `blocksProposed` from the new ValidatorRegistry honestly;
  fix C-4 (self-staker mislabeled "validating"). Until the WO-1 onboarding step-list lands,
  show "synced full node — validator registration coming" rather than implying active-set.

## Acceptance / evidence (Rule 11)
- Rebuilt-node genesis hash == live genesis; node-source pin recorded.
- Sync proof: real run log (0→head) + green automated bounded proof.
- S5 negative control + positive (real attribution) tests; Rust + vitest counts recorded
  (ratchet, Rule 2). No `.unwrap()` in touched UI-facing Rust.
- Independent Rule-8 review of BC-1.3 before merge.
