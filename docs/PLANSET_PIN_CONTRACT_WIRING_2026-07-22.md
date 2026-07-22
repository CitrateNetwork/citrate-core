---
title: Planset — wire the pin action to a real 40204 transaction
created: 2026-07-22
branch: main
author: Claude (Citrate Core session)
status: proposed
scope: citrate-core (app) + citrate-chain (commD spec) ; PoSt sealer is separate/upstream
---

# Planset — Pin-contract wiring (IPFSIncentivesV3 on 40204)

## Goal

Replace the honest "not wired" surfaces with a **real** bonded pin: the user pins
content, the app builds a genuine `IPFSIncentivesV3.registerModel(...)` transaction
(with a bond) on chain 40204, routes it through the SignatureCeremony (human
approval, Rule 3), broadcasts it, and shows the real tx hash + an explorer link.
Then a drag-and-drop UX that hides the raw CID. PoSt (ongoing proofs) is a separate
track that depends on the sealer.

## Grounding (verified 2026-07-22)

- **Contract EXISTS on 40204** — `IPFSIncentivesV3` = `0xb024ad0b5aefa87fa1e10234afc85b6128932df4`
  (canonical `citrate-chain/contracts/addresses/40204.json`; live code on-chain). **No
  chain-ops deploy needed** — this is app-side wiring.
- **Pin entry point:** `registerModel(bytes32 cid, bytes32 commD, bytes32 dataHash, string dataUri)`,
  `payable`, requires `msg.value >= MIN_MODEL_BOND` and `cid != 0` and not-already-registered
  (`IPFSIncentivesV3.sol:493`).
- **Current honest surfaces (to replace):**
  - `citrate-core/src/surfaces/Journal.tsx:206` → `store.settleUnwired("Pinning")`
  - `citrate-core/src/surfaces/Node.tsx:167`   → `store.settleUnwired("Pin bond")`
  - `store.settleUnwired` (`store.ts:1670`) toasts "isn't wired to a real 40204 transaction yet".
- **Related on-chain (proof track):** `submitPoSt(...)`, `challengePin(...)`, `commitChallenge(...)`,
  `postPass/postFail`, `reclaimBond/returnBond/slash` on the same contract.

## THE hard dependency / risk — commD is consensus-critical

`registerModel` takes `commD` (the piece commitment). The contract's `challengePin`
**recomputes commD (a Merkle-root-style hash) off the registered data and SLASHES the
bond if it disagrees** (`IPFSIncentivesV3.sol` ~L520–532). Therefore the app's commD
MUST match the contract's expected computation **byte-for-byte** — a wrong commD is a
slashable pin (loss of funds). There is currently **no commD/piece-commitment helper in
the node or chain crates** (`grep commD|piece_commitment|CommP` over `node/`, `crates/`
→ none). So commD is net-new and must be authored against the canonical spec, not
reverse-engineered. This is the gating work-package.

Note: commD (data commitment) is distinct from the sealer's replica proof — the initial
pin needs only commD, so the pin CAN be wired before the full PoSt sealer. But the commD
algorithm is shared with the proof path, so author it once, canonically.

## Work packages

### WP-1 — commD / CID computation (BLOCKER, consensus-critical)
- Extract the canonical commD algorithm the contract recomputes (fr32 padding + the
  contract's Merkle-root hash). **Confirm the spec with the chain team** before coding —
  a mismatch is slashable. Name the source (contract fn + any spec doc).
- Implement in Rust (a `citrate-core` src-tauri module or a shared chain helper crate),
  zero `.unwrap()`, with tests that reproduce the contract's `challengePin` recompute for
  known inputs (round-trip: our commD == contract's recomputed commD → no slash).
- Also emit `cid` + `dataHash` (keccak256(data)). **This is the CID generator the
  drag-drop UX consumes.**
- Gate: a fuzz/property test that our commD survives `challengePin` on a forge fork.

### WP-2 — pin transaction command
- Rust `#[tauri::command]` (e.g. `pin_register`) that: computes WP-1 fields from the
  content, encodes `registerModel(cid, commD, dataHash, dataUri)` with `value = bond
  (>= MIN_MODEL_BOND)`, builds the intent, routes through `SignatureCeremony` (Rule 3 —
  no signing outside the ceremony), broadcasts to 40204, returns the REAL tx hash.
- Data-source named (contract addr + method) per chain CLAUDE.md. No fabricated hash on
  any failure (mirror `droplet-signer`/`settleUnwired` honesty). Integration test end-to-end.
- Reuse the wired-withdrawal pattern (`walletRequestWithdrawal`) as the template.

### WP-3 — app wiring (replace settleUnwired)
- `Journal.tsx` "Pinning" + `Node.tsx` "Pin bond" call `pin_register` instead of
  `settleUnwired`; render the real tx hash + `explorer.citrate.ai/tx/<hash>` link;
  honest pending/failed states. Remove the "grounded contract pending" copy.

### WP-4 — CID drag-and-drop UX
- Drop a file/entry → WP-1 computes CID/commD locally → WP-2 pins it → the UI shows a
  friendly pinned state and HIDES the raw CID by default; "details" expands to the
  CID/commD + the on-chain tx (explorer link). CID never dumped raw unless requested.

### WP-5 — PoSt proofs (SEPARATE track; needs the sealer)
- `submitPoSt(...)` + challenge lifecycle. Blocked on the **PoSt sealer sidecar**
  ("pending upstream", surfaced honestly at `Node.tsx:406`). Bundle/build the sealer,
  then wire challenge responses. Out of scope for the initial pin; tracked here so the
  coupling is explicit.

## Sequencing & gates
WP-1 → WP-2 → WP-3 → WP-4 (WP-5 parallel/after). Hard gates: (a) commD matches the
contract (fuzz vs `challengePin`); (b) a real pin tx lands on 40204 with a hash verifiable
on the explorer; (c) every signature via the ceremony; (d) zero mocks / zero unwraps /
data sources named (chain CLAUDE.md).

## Owner decision needed before WP-1
Confirm the **canonical commD spec/impl** (chain team) so WP-1 matches the source of truth
rather than reverse-engineering a consensus-critical hash.
