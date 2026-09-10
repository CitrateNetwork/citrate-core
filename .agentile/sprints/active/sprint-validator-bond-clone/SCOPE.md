---
title: "Sprint — Validator staking realignment: adopt the MemberBond bond-clone model"
created: 2026-08-05
branch: reconcile/onboarding-m2-and-self-bond (to be recut)
author: Claude (Opus 5, 1M context) for SaulBuilds
status: active
planset: citrate-federation/.agentile/planset/2026-07-11-citrate-core/ (money path / validator)
supersedes_decision: ADR-2026-07-27 (EOA-direct self-bond) — REVERSED by owner 2026-08-05
---

# Sprint — Validator staking realignment (bond-clone canonical)

## Context / why

Onboarding "completed" while **0 SALT was actually staked** (the app claimed
*Validating · eligible for proposer* with an empty bond). Fixing that surfaced a
deeper problem: **the system has two incompatible validator-staking models built
around the same week, and the live money path uses the wrong one.**

| | EOA-direct (live) | Bond-clone (canonical) |
|---|---|---|
| Origin | ADR-2026-07-27 (after the 2026-07-28 fund-loss) | `MemberBond.sol` contract design; PR #114 read |
| Who stakes | the member's **custody EOA** | the member's **`MemberBond` clone** |
| Registry key | `pubkeyOfStaker[EOA]` | `pubkeyOfStaker[clone]` |
| Deployed today | treasury-signer `server.mjs` + app `node_register_validator` | contracts only; app/treasury never call it |

**Owner decision (2026-08-05): the bond-clone model is canonical.** It is the
contract's intended design and it structurally avoids the permanent
`StakerHasValidator()` lock (the EOA-direct path registers the member's EOA
*forever* — it can never re-register after a re-roll or slash; the clone gives each
member a fresh, disposable staker). It also cleanly solves the 2026-07-28 fund-loss
that motivated ADR-2026-07-27: the 32k goes into a real, member-only-withdrawable
contract, not an unspendable predicted EOA. **This reverses ADR-2026-07-27.**

## The canonical flow (from the contracts — authoritative)

Two transactions by design (`MemberBond.sol:179-196`, `MembershipStakeVault.sol:222-253`):

1. **Grant (at payment)** — `MembershipStakeVault.grant(member, 32k, memberTokenId)`:
   `Clones.cloneDeterministic` deploys the member's `MemberBond` clone, and
   `MemberBond.initialize{value: 32k}(...)` funds it. **The 32k lives in the clone.
   It does NOT stake yet** (the member has no node/proposer key at payment time).
2. **Activate (when node synced)** — member calls `MemberBond.activate(pubkey, sig)`
   on their clone (`onlyMember`, via the app's SignatureCeremony). The **clone**
   calls `registry.registerValidator{value: principal}(pubkey, sig)`, so
   `msg.sender` = the clone = the staker. The registration digest binds the **clone**
   address (CREATE2-deterministic — the app can sign it before the clone exists).

Contract addresses (40204, current book): vault `0x04c32967…`, SBT `0xAD826D04…`,
registry `0x61d44d8a…`. `bondOf(member)` returns the CREATE2 clone address;
`bondDeployed`/`getCode` says whether it exists yet.

## Work items (per component)

### WI-1 — Treasury signer (`citrate-identity/services/treasury-signer/server.mjs`) · @rule8 · DEPLOYED
Restore **Leg 1** to `vault.grant(member, 32k, tokenId)` (deploy + fund the clone)
in place of the ADR-2026-07-27 native EOA bond-fund. The `VAULT_ABI.grant` is still
present. `memberTokenId` must be the SBT token id (mint SBT first, as the current
Leg-2/leg-order already does — vault.grant reverts `NotTheMembersToken` otherwise).
Idempotency: skip if `bondExists(member)` (the escrow is deployed). Remove the
`fundTxHash`/GAS_HEADROOM native-transfer path (or keep a tiny gas top-up ONLY for
the member's own `activate` tx — see WI-2 gas note). **Redeploy the droplet**
(`/opt/citrate-identity`, `citrate-treasury-signer` container; docker restart does
NOT re-read `--env-file`, and caddy reload won't apply new handle blocks — see
[[reference-identity-droplet-deploy]] / [[project-phase-d-treasury-signer]]).

### WI-2 — App validator registration (`citrate-core/src-tauri/src/node.rs`, `validator.rs`)
`node_register_validator` currently builds `ValidatorRegistry.registerValidator`
with **staker = the member EOA**. Change to call **`MemberBond.activate(pubkey, sig)`
on `bondOf(member)`**, and make the registration digest bind the **clone** address as
the staker (`validator::registration_digest` / `sign_registration` take `staker` —
pass the clone address, not the EOA). The tx is sent from the member EOA (the
clone's `onlyMember` gate is the member EOA), but **it carries no value** — the 32k is
already inside the clone (`principal`), so `activate` forwards it. Gas: the member EOA
needs a little native SALT to send `activate`; either a tiny treasury gas top-up
(WI-1) or the AA/paymaster path (the S5 UI already says "treasury UserOp · gas
sponsored"). Preserve the honesty guard, but re-point it: check the **clone** is
deployed + funded (`bondDeployed` && `attributedPrincipal >= 32k`), not the EOA
balance. Keep the arm-mining wiring (proposer.key still needed for `pubkey`).

### WI-3 — PR #114 (`feat/m2-bond-status`) — MERGE
Confirmed correct: it reads `attributedPrincipal`/`bondOf`/`bondDeployed` and keys the
registry read on `pubkeyOfStaker(bondOf(member))` (the clone staker) gated on
`bondDeployed`, plus `describeBond`. This is the canonical read. It is
CLEAN/MERGEABLE against main. **Merge first** as the read foundation.

### WI-4 — PR #125 (`fix/onboarding-validator-self-bond`) — REBASE onto #114
- **Drop** the parts superseded by #114: the `attributedShares`-revert band-aid in
  `grant_status.rs` (#114 removes that read), and the `bondedStake` read keyed on the
  EOA in `refreshNode`/`snapshot` (use #114's clone-based `bondedStakeWei`/`has_validator`).
- **Keep + retarget**: the auto-bond wiring (`maybeAutoBond`) — but it now opens a
  `MemberBond.activate` ceremony (WI-2), and gates the node "validating" on #114's
  clone bond. Keep `node_arm_mining` (proposer.key mint), the S6 `error`/Retry
  branch, and the model-download Skip/Restart + launch auto-resume (all independent,
  still valid).
- Reconcile the frontend: `isGrantOnChain`/`describeBond`/GrantStatus shape come from
  #114; the node "staked" gate reads #114's clone bond, not `hasGrant`.

### WI-5 — Broken-member cleanup
`0x9aee4eec…` self-bonded EOA-direct during testing → its EOA is permanently
`StakerHasValidator`. It can never use the clone path. **Testing the clone flow needs
a fresh linked wallet** (re-link a new custody EOA) or a fresh member. The stray EOA
registration + its 32k are orphaned in the registry under the EOA staker — flag for
whether that 32k is recoverable (likely not, without an EOA-staker withdraw path).

## PR / merge order

1. **#114** merges (read foundation). 
2. **WI-1** treasury-signer restore + redeploy (clones start deploying at grant).
3. **WI-2/WI-4** app: recut `#125` as a rebase on #114 with the `activate` retarget;
   new PR (or force-update #125). Owner merges.
4. Re-run the E2E on a FRESH member: pay → clone deployed+funded → node sync → arm →
   `MemberBond.activate` ceremony → approve → `pubkeyOfStaker(clone)` non-zero,
   `stakeOf` = 32k, app "Validating".

## Test plan (acceptance)

- Contract-level: after `vault.grant`, `bondOf(member)` has code and
  `attributedPrincipal(member) == 32k`; after `activate`,
  `pubkeyOfStaker(bondOf(member))` != 0 and `stakeOf(that) == 32k`. (These are the
  reads #114 already asserts.)
- App: S5 grant settles on the funded **clone** (not the EOA); S6 "Validating" only
  after `activate` lands; the node stays "synced — not yet a validator" until then.
- Rust `grant_status` tests (from #114) + the store settle tests must pass with the
  clone read. Add: an `activate`-ceremony builder test in `node.rs`.

## Cross-release (Linux + Mac; Microsoft deferred)

The onboarding code is 100% platform-agnostic (no OS conditionals; Mac keyring =
`apple-native`, dmg/app targets + signing already configured). **But nothing here is
release-ready until this realignment lands** — shipping the current EOA-direct app
would permanently lock every member's EOA as a staker. So this sprint gates BOTH the
Linux (x86_64 + arm64) and macOS (arm64 + Intel x86_64) releases. Mac build per
`BUILDING.md` (sidecar per-arch, on a Mac). No Mac-specific onboarding code changes.

## Open questions / risks

- **Gas for `activate`**: does the member EOA get a tiny native top-up (treasury) or
  a sponsored UserOp? The S5 copy implies sponsored — confirm the paymaster path
  exists and funds the `activate` call.
- **The 2026-07-28 fund-loss rationale**: reverting to `vault.grant` is safe under the
  clone model (funds a real contract, not an unspendable EOA) — confirm the original
  loss was the predicted AA wallet, not the MemberBond clone.
- **#114 author coordination**: #114 built the clone read on 2026-07-30 while the
  treasury/app shipped EOA-direct — loop them in so the two halves land together.
- **Orphaned EOA stakes** from EOA-direct testing (WI-5) — recoverable?

## Related
[[project-membership-self-bond-2026-08-05]] · [[project-phase-d-treasury-signer]] ·
[[project-phase-d-money-path]] · [[reference-identity-droplet-deploy]] ·
PR #114 (feat/m2-bond-status) · PR #125 (fix/onboarding-validator-self-bond)
