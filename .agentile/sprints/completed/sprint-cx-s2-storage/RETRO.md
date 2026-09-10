---
created: 2026-08-27
branch: chore/close-cx-s1-s2
author: Claude Opus 4.8, directed by @SaulBuilds
status: archived
sprint: CX-S2 (Commons — storage/pinning file store), Lane B
closing_commit: 51139bd (CX-S2.4 #151 merged)
purpose: Sprint retrospective for CX-S2 (agentile:retro). Closed on delivered scope; S2.2 carved to the backlog (externally blocked).
---

# Retro — CX-S2 (Commons storage/pinning file store, Lane B)

## Outcome
| | |
|---|---|
| Goal achieved? | **PARTIALLY — the safe, user-visible file store shipped; the on-chain bond is blocked on a chain-side fix and carved out.** Commons has a real drag-drop IPFS file store: add / local-pin / retrieve / unpin, over a supervised kubo seam, with D-22/RT-4-compliant copy. |
| WPs closed | S2.1 kubo seam + `StorageManager`, S2.3 drag-drop file-store surface, S2.4 subsidy copy + honesty tripwire. |
| WP carved out | **S2.2 ceremony-gated `IPFSIncentivesV3` bond** → `backlog/cx-s2.2-onchain-bond.md`. Blocked on `citrate-chain#170` (the CommD-bond finding); do NOT ship a real-SALT bond client until the chain fix lands. |
| Merged PRs | #148 (S2.1), #150 (S2.3), #151 (S2.4), #149 (the finding doc). |

## Metrics delta (ratchet — no axis decreased)
| Axis | S2 start | S2 close |
|---|---|---|
| Rust lib tests | 293 | **300** |
| Frontend tests | 348 | **356** |
| New warnings | — | 0 |

## The load-bearing finding (why S2.2 is carved out)
Implementing S2.2 faithfully surfaced a **HIGH money-path defect** in the deployed `IPFSIncentivesV3`:
`challengeWrongCommD` never recomputes CommD — it trusts a caller-supplied value and slashes on any
difference once `keccak256(data)==dataHash`. Registered files are public on IPFS, so any holder can
grief-slash an honest bond for a 50% reward. The ADR mandated `verifyMerkleRoot(data, challengerCommD)`;
the deployment dropped it. Also: no canonical CommD-over-bytes exists (only a reduced 4-leaf demo).
Full analysis: `docs/FINDING_PIN_COMMD_BOND_2026-08-26.md`; tracked in `citrate-chain#170`.

**This is the sprint's most valuable output** — trying to build the bond client honestly is what
caught a deployed-contract flaw before any user SALT touched it (Rule 1 in action: a bond that
"works" but is silently slashable is a stub with a straight face).

## What worked (concrete + causal)
- **Injected kubo transport.** `StorageManager` over a fixture `KuboTransport` (in-memory blobs + pin set) tested add → pin → list → unpin → retrieve + the index/live-pin reconciliation with no real daemon.
- **Stateless commands.** The pin index lives on disk, so storage commands build their manager per-call from the app data dir — no managed Tauri state, no edit to the s0-owned `lib.rs`, Lane B stayed race-free.
- **Honesty tripwire (S2.4).** A `renderToStaticMarkup` test mirrors the `cx-copy-lint` patterns so the surface and the CI gate agree; forbidden regexes were built from fragments so the test file itself stays copy-lint-clean (a small gotcha discovered when the lint flagged the test's own prose).
- **Refusing to ship the unsafe leg.** The strongest decision was *not* building S2.2 against a broken contract, and instead filing the fix where it belongs.

## Lessons carried forward
- A TLA+ pass that is green does not cover a gate it abstracts away — `PINIncentiveV4`'s `ChallengeWrongCommD` models only money-flow, which is exactly why TLC didn't catch the missing correctness check. Formal + implementation-diff review are complementary, not substitutes.
- When a WP is blocked on another repo, carve it to the backlog with the tracking issue + the finding — never leave it silently "in progress" (Rule 4).

## Status
Lane B safe-parts COMPLETE (S2.1/S2.3/S2.4). S2.2 backlogged, gated on `citrate-chain#170`. Reopen
S2.2 as a one-WP finish (client over the merged S2.1 seam) once the chain fix ships the canonical
`computeCommD` + the sound challenge.
