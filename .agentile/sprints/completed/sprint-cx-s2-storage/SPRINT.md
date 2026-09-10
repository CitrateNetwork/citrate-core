---
created: 2026-08-26T00:00:00Z
branch: cx/s2.<wp>-<slug> (one branch per WP; Lane B)
author: Larry Klosowski (@SaulBuilds) + Claude Opus 4.8
status: archived
sprint: CX-S2
planset: Commons / citrate-core-social (citrate-federation/.agentile/planset/2026-08-26-citrate-core-social/)
tier: T1
lane: B (owns .agentile/cx-ownership.map lane s2 — storage.rs, storage_tests.rs
       + bridge/*/storage.ts + slices/storage.ts + surfaces/StorageFiles.tsx)
---

# Sprint CX-S2 — Storage/pinning file store · "Commons"

## Goal

A drag-drop file store on IPFS: add files, pin them with a SALT bond, retrieve them, see their
pin state. D-22 = admin-subsidy pool; the network REWARDS pinners (RT-4), never "earn/pay to
store". Lane B off the merged CX-S0 scaffold; disjoint file ownership from Lanes A/C–F, so it
lands independently. Every WP passes `scripts/cx-ownership-check.sh s2`.

## Baseline (Rule 2 — must not decrease)
- src-tauri lib tests: 293 (post CX-S1). CX-S2 adds tests per WP.

## Work packages
- [x] **S2.1** kubo add/pin/ls/rm/cat Rust seam (`storage.rs`): injectable `KuboTransport`
      (`/api/v0/{add,pin/add,pin/ls,pin/rm,cat}`) + `StorageManager` (add/pin/unpin/retrieve/list)
      over a local pin index reconciled with the daemon's live pins. Stateless commands (built from
      the app data dir; no spine edit). +7 tests. **M.** — lib 293 → 300.
- [!] **S2.2** ceremony-gated `IPFSIncentivesV3` bond client — **BLOCKED on a chain-side fix.**
      Faithful implementation surfaced a HIGH money-path finding: the deployed `challengeWrongCommD`
      never recomputes CommD (grief-slashable), and no canonical CommD-over-bytes exists. Finding:
      `docs/FINDING_PIN_COMMD_BOND_2026-08-26.md`; PR #149; citrate-chain#170. Do NOT post real SALT
      bonds until the contract self-computes CommD + a canonical `computeCommD` lands. **L.**
- [x] **S2.3** drag-drop + list file-store surface (`bridge/tauri/storage.ts`, `slices/storage.ts`,
      `surfaces/StorageFiles.tsx`, contract test): native Tauri drag-drop → add; list with
      CID/size/pin-state; pin (LOCAL until S2.2)/unpin/retrieve; honest "network bond after the
      on-chain fix" note. +5 tests. **L.** — frontend 348 → 353.
- [x] **S2.4** subsidy copy + honest reward states in `StorageFiles.tsx`: D-22/RT-4 framing
      ("the network rewards pinners for keeping your data available," from a shared subsidy pool),
      honest that it is forthcoming (bond not yet live) — never "earn SALT by storing," guaranteed,
      or immediate cash. + `storageFilesHonesty.test.tsx` (render-and-assert tripwire mirroring the
      copy-lint patterns). **M.** — frontend 353 → 356.

## Lane B status

S2.1 + S2.3 + S2.4 SHIPPED — Commons has a real, safe drag-drop IPFS file store (add / pin-local /
retrieve / unpin) with compliant subsidy copy. **S2.2 (the on-chain SALT bond) is the ONLY open WP,
blocked on the chain-side CommD fix (PR #149, citrate-chain#170).** Lane B closes fully the moment
that lands and the S2.2 client is wired on top of the merged S2.1 seam.

## Daily
- 2026-08-26 — S2.1 merged: kubo seam + manager + fixture tests green (7), lib 293→300.
- 2026-08-26 — S2.2 BLOCKED: filed HIGH CommD-bond finding (PR #149, chain#170). S2.3 built the
  drag-drop file store over the S2.1 seam (local pin, honest bond-deferred copy); frontend 348→353.
- 2026-08-27 — S2.4 done: D-22/RT-4 subsidy copy + honesty tripwire; copy-lint + tsc clean,
  frontend 353→356, ownership s2 green. Lane B safe-parts complete; only S2.2 remains (chain-gated).
