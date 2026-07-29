---
created: 2026-07-29
branch: docs/federation-state-and-roadmap-2026-07-29
author: Claude (Opus 4.8), directed by @SaulBuilds
status: handoff — verified federation state + roadmap for the money-path / vaulting /
  staking / ALF / consensus arc (2026-07-28 → 29). Every claim checked in-repo.
relates:
  - citrate-core/docs/MEMBERSHIP_STAKE_LOCK_DESIGN_2026-07-28.md
  - citrate-core/docs/DGX_HANDOFF_CONSENSUS_AND_ALF_2026-07-28.md
  - citrate-alf-web/docs/ALF_ND_NODE_BRIDGE_PLAN_2026-07-28.md
---

# Federation state + roadmap — money-path / vaulting / staking / ALF / consensus

Birdseye of everything in flight across the arc, split by **what's done on this machine
(local/app/backend PRs)** vs **what is intended remotely (DGX / citrate-chain / droplet)**.
All git tips, PRs, branches, and code claims below were verified in-repo on 2026-07-29.

## 0. TL;DR
- **Shipped + merged (citrate-core):** wallet provisioning (#104), grant-settle bridge
  (#105), custody seamless re-unlock (#106), serveStart + honest dashboard (#107),
  launch session-refresh (#108), lock design (#109), combined DGX handoff (#110).
- **Shipped + merged (citrate-identity):** paid-`commercial`-without-KYC gate (#79) —
  **DEPLOYED** to auth.citrate.ai (verified: openid-config live + serves the entitlement
  claim). Closes "public despite paid" for unverified members (with #108).
- **Preserved + PR-open:** ALF-ND-A surface (citrate-core #111) — the ALF team's work,
  rescued off unpushed local main, rebased onto #110, green.
- **Installed app:** built from citrate-core main (has #104–#108); functionally current.
- **The money-path decision (owner):** the $48 grant must land as a **staked, 1-year-
  locked validator bond** — a citrate-chain contract program (Track M), greenfield.
- **Three verified surprises** you should know (details below): (1) core-membership
  **main runs `vault.grant` (STAKED); the liquid `fundMember` is an unmerged branch**
  that appears to be what's deployed; (2) **M-1 is real** — consensus reads ONLY
  `ValidatorRegistry`, never the vault, so a vault grant does NOT make a block producer;
  (3) **the vault is CREATE2** (a lock redeploy MOVES its address → re-pin, not a reroll).

## 1. Per-repo verified state

| Repo | main tip (verified) | Our branches / PRs | Deployed? |
|---|---|---|---|
| **citrate-core** | `b18fbea` (#110) | #104–#110 MERGED; **#111 open** (ALF-ND-A, `feat/alf-nd-a`) | app installed from main |
| **citrate-identity** | `1f937ce` (#79) | #79 MERGED | **DEPLOYED** to auth.citrate.ai (verified live) |
| **core-membership** | `4de910f` (#27 merge) | `feat/grant-funds-member-eoa` = **9 ahead / 15 behind, UNMERGED**; no open PRs | membership.citrate.ai — **branch likely deployed (MUST VERIFY)** |
| **citrate-chain** | `dc21c2b` (#138 MP-DEPTH) | none of ours; DGX active on consensus/sync/pruning | 40204 live; local 5 behind |
| **citrate-alf-web** | `4631a7f` | `feat/alf-s1-identity` (19 ahead); ALF team owns | — |
| **citrate-compute-pool** | `24edf8e` (#7) | none; Track A (training-worker = event-logger only) | contracts live, orchestration absent |

## 2. The money-path truth (the load-bearing picture)

Three different "grant" behaviors exist — do not conflate them:

| Where | Grant behavior | attributedStake | Lock | Status |
|---|---|---|---|---|
| core-membership **origin/main** | `vault.grant` → STAKES 32k into the vault | 32k | none (orchestrator-driven release) | merged, but apparently NOT what's deployed |
| core-membership **`feat/grant-funds-member-eoa`** | `fundMember` → funds member EOA LIQUID (ADR-2026-07-27) | 0 | none | UNMERGED; **on-chain evidence says THIS is deployed** |
| **Owner's target** | staked + **1-year time-lock** + KYC-gated withdrawal | 32k | ~1yr (block-height) | greenfield (Track M) |

- **Verified on live 40204:** the test member `0x354F…16C6` has native `32000.05 SALT`,
  `attributedStake = 0`, no validator bond → the **LIQUID `fundMember` path ran** for
  them. Since `fundMember` is branch-only, **production must be deploying
  `feat/grant-funds-member-eoa`, not main.** `/api/health`'s "vault.grant" text is a
  static seam string, not proof of the code path.
- **MUST VERIFY (backend owner):** the Vercel production branch for `core-membership`
  (dashboard → Settings → Git). If it's the feat branch, note it conflicts with main's
  money-guards `fd5a8bb` ("refuse to bond-fund the derived placeholder — it burned 32k
  SALT") + `671740a` ("only a PROVEN wallet may become a payment target"), which exist
  BECAUSE the bond-fund model misfired once.
- **Direction implied by the owner's decision:** the lock program builds on **main's
  `vault.grant` (staked)** path + adds the lock — NOT the liquid feat branch. So
  `feat/grant-funds-member-eoa` is a **detour to retire**, not to merge; the interim
  citrate-core native-settle bridge (#105) is removed when the staked+locked path ships.

## 3. Track M — 1-year locked validator bond (citrate-chain, greenfield). VERIFIED facts:
- `MembershipStakeVault.sol` is **plain `Ownable` (NOT upgradeable)**; `pool` is
  `immutable`. Owner wants upgradeable → convert to UUPS first (storage-safe).
- Lifecycle `None→Attributed→Lapsed→Released→Claimed`; only `claimReleased` moves SALT;
  release is `onlyOwner` + a one-way `enableMainnetRelease` flag. **No time/block lock
  exists** (`lockUntil`/`unlockBlock`/`365` grep = none). `grantedAt` is **stored but
  never read** — the exact hook (vault natspec Q5 TODO). Add `block.timestamp >=
  grantedAt + LOCK_SECONDS` (or block-height per owner) in `releaseGrant`.
- **M-1 (consensus) is REAL + unbridged:** the node reads eligibility ONLY from
  `ValidatorRegistry` (`registry_sync.rs` epoch snapshot → `activeSet()`/`minStake()`;
  bond = the validator's OWN `msg.value` on `registerValidator`). Consensus **never
  reads `MembershipStakeVault.attributedStake`/`meetsRequirement`** (grep = zero hits);
  the vault's `isValidatorEligible` is APP-FACING only. So a vault-granted member is NOT
  a block producer today. DGX must choose: (a) consensus also honors the vault, or (b)
  the vault/orchestrator registers the member in the registry on grant.
- **KYC gate mechanism:** `CitrateMemberSBT` has **no `kycVerified` flag** (only
  `quarantined`/`revoked`/`isActive`). So the on-chain KYC gate is either (i) add a flag
  / read `isActive` as a proxy, or (ii) orchestrator-gated (treasury signs release only
  for verified members). Pick at G0.5.
- **Reroll vs redeploy (CORRECTED, verified):** the vault deploys via **CREATE2**
  (`DeployCoreMembership.s.sol`, Arachnid factory, salt-based) — an L1 reroll does NOT
  move it. BUT the address is a function of init_code, so **changing the bytecode (the
  lock code) OR a constructor arg (owner/pool) MOVES the address.** Shipping the lock =
  new bytecode = **new address → re-pin `40204.json` + update the
  `CoreMembershipCreate2.t.sol` tripwire + re-point readers.** This is a **redeploy +
  re-pin, NOT a chain reroll.**
- **Test harness:** foundry; extend `contracts/test/core_membership/MembershipStakeVault.t.sol`
  (31 tests, real pool, `GRANT_AMOUNT = 32_000 ether`) with `vm.warp` lock tests.
- **DGX collision check:** DGX's active work (MP-DEPTH pruning #138, blue-ancestry halt
  #134, sync fixes) does NOT touch membership/vault. One latent watch: MP-DEPTH pruning
  vs `registry_sync` epoch-snapshot state reads — flag, no code coupling today.

## 4. Track A — ALF real compute (citrate-compute-pool + coop). VERIFIED blockers:
- `training-worker` is an **event logger only** (own comment); no coordinator/gather
  service; `recordContribution` is a manual governance call; ALF coop not instantiated.
  All four (A-1…A-4) are DGX/ops. Until then ALF "contribute" stays an honest seam.
- ALF-ND-A (the citrate-core surface) is done + PR'd (#111). ND-B (deep-link) deferred
  post-QA (@rule8). Scheme + claim locked: `citrate-core://alf/contribute?round=<u32>`;
  `alfMember = (citrate_role === 'alf_member')`.

## 5. ROADMAP — done vs not, LOCAL (this machine) vs REMOTE (DGX/droplet/backend)

| # | Item | Owner | Status | Verified |
|---|---|---|---|---|
| 1 | Wallet provisioning + custody re-unlock + launch-refresh | LOCAL (me) | ✅ merged #104/#106/#108 | yes |
| 2 | S3 settle bridge (native) — INTERIM | LOCAL (me) | ✅ merged #105 (remove when staked path ships) | yes |
| 3 | serveStart-on-launch + honest dashboard | LOCAL (me) | ✅ merged #107 | yes |
| 4 | Paid-`commercial`-without-KYC gate | LOCAL→DROPLET | ✅ merged #79 + **DEPLOYED** auth.citrate.ai | yes (live) |
| 5 | ALF-ND-A surface | ALF team | ✅ PR #111 (preserved/rebased) | yes (green) |
| 6 | Combined DGX handoff + lock spec | LOCAL (me) | ✅ merged #109/#110 | yes |
| 7 | **Real `llama-server` binary** (chat = stub today) | LOCAL/build | ❌ NOT done (33KB stub) | yes |
| 8 | core-membership: confirm/settle deployed grant branch | BACKEND | ❌ MUST VERIFY (Vercel prod branch) | open |
| 9 | **M-1 consensus decision** (vault↔registry eligibility) | DGX | ❌ not started | greenfield |
| 10 | **M-2 lock contract** (upgradeable → block-lock → KYC gate) | DGX/contracts | ❌ greenfield (hook = Q5 TODO) | yes |
| 11 | M-3 audit (@rule8) + redeploy + re-pin | DGX/security | ❌ blocked on M-2 | — |
| 12 | Track-M cross-repo wiring (backend `vault.grant`, app settle on attributedStake, remove #105 bridge, dashboard lock) | LOCAL + BACKEND | ⏳ blocked on M-1/M-2 | — |
| 13 | Commissary KYC gate covers owner intent | BACKEND | ⚠️ partial — tier-keyed (blocks unverified from commercial.kyc+ artifacts incl. citrate-core; allows native/SDKs/docs) | yes |
| 14 | Withdrawal KYC gate (supersedes time lock) | DGX/contracts | ❌ part of M-2 | — |
| 15 | ALF Track A: coordinator + real worker + patronage hook + coop | DGX/ops | ❌ not started | yes |
| 16 | ALF-ND-B deep-link + ND-C contribution | ALF team | ⏳ deferred (post-QA / blocked on A) | — |

## 6. MUST-VERIFY / open decisions (before building on top)
1. **core-membership Vercel production branch** — is it `main` (vault.grant/staked) or
   `feat/grant-funds-member-eoa` (fundMember/liquid)? On-chain says liquid. (BACKEND)
2. **M-1** — consensus honors vault, or vault registers in registry? (DGX)
3. **KYC-gate mechanism** — SBT flag vs orchestrator-gated. (DGX + contracts)
4. **Lock duration** — exact `LOCK_BLOCKS` for ~1yr−1day at 40204 block time. (DGX)
5. **QA-freeze settle** for ALF ND-A/ND-B merge timing. (DGX)

## 7. Next actions
- **DGX:** answer M-1; build M-2 on the vault (upgradeable→block-lock→KYC); stand up ALF
  A-1…A-4. Ref: `DGX_HANDOFF_CONSENSUS_AND_ALF_2026-07-28.md`.
- **Backend:** confirm the core-membership production branch; plan the retirement of the
  liquid `feat/grant-funds-member-eoa` detour in favor of main's `vault.grant` + the lock.
- **citrate-core (next session):** build the real `llama-server` binary (#7); after M
  lands, do the Track-M wiring (#12) and remove the #105 bridge; merge ALF #111 when QA
  clears.
- **ALF:** Phase 0 now; ND-A (#111) merge on the green light.
