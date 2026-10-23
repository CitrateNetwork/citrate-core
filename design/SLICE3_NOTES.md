---
created: 2026-10-22
branch: beta/design-slice3
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: review (design slice 3, closes prototype v1 due 2026-10-30); Codex round 2 P2s addressed 2026-10-22; chain-team review not yet requested
---

# Design slice 3: Wallet, Groups, Settings

Slice 3 of the 0.5.2 design prototype in `design/CitrateCore.dc.html`, on top of slices 1 and 2 (`SLICE1_NOTES.md`, `SLICE2_NOTES.md`). It replaces the last two place stubs (Wallet, Groups) and redraws Settings against UI truth contract v1. With it, every K0 place and Settings has its screens, annotations and variants, which is what the board item "Design prototype v1 complete (all screen groups + 3 click paths)" asks for.

Inputs, in order of authority: UI truth contract v1 (§1 validator, bond, membership and transaction words; §2 catalog; §5 money copy), the C1-V and C1-P PRs in core (#348 one staked figure and the validator word, #349 grantCopy.ts, #379 the membership price from core-membership, #371 withdrawals), GROW-S2 §11 (join and invite words), core #355 and #374 (invite links and rendezvous states), core #361 (round cards behind a flag), the K0 route map (core #343), then the tokens and design system.

## How to review

Serve `design/` (`python3 -m http.server`) and open `CitrateCore.dc.html#/wallet`, `#/groups` or `#/settings/billing`. The annotation strip has the data, empty, error and unavailable switch on every screen, plus:

- Wallet: the validator word (all six §1 words), the balance (not read, read) on Overview, and the Activity rows (this session, drawn states).
- Groups: round cards (off in 0.5.2, on), the ledgers (not in the address book today, deployed · dormant), the join service (live, unavailable) on Members, and the ALF claim.
- Settings › Memberships & billing: the price (not read, read).

Deep links: `#/groups/<chat|rounds|members|cluster|alf>`, `#/groups/chat/alerts`, and the 0.5.1 ids as K0 maps them (`comms` → Alerts, `people` → Members, `cluster` → Cluster, `train` → Rounds, `alf` → ALF, `community` → the hidden `groups/community`). Wallet is one K0 route; `#/wallet/<overview|staking|activity|identity>` is a prototype deep link to its tab. Settings sections open as before (`#/settings/<section>`).

Prototype state moved to `citrate-core-proto-v5`.

## Screens

| Screen | Route | Bridge | Flag at genesis |
|---|---|---|---|
| Wallet › Overview | `wallet` | `wallet_balances` (eth_getBalance "latest"; an error is unknown, never 0), `wallet_send` through the signature ceremony, validator word from `validatorInfo` / `isActive` / `stakeOf` and `MemberBond.activated()` (#348, #358), `node_validator_earnings` | none |
| Wallet › Staking | `wallet` (tab) | `stakeOf`, `validatorInfo`, `isActive`, `currentEpoch`; MemberBond principal, activated, unlock reads (#348); activate: `node_register_validator` through the ceremony; liquid path: `wallet_stake`, `wallet_request_withdrawal`, `wallet_pending_withdrawals` (#371), `wallet_claim_withdrawal`; minStake from the C1 fixture | bond model per C4; liquid path per the money-path decision |
| Wallet › Activity | `wallet` (tab) | `wallet_activity` (CitrateScan txlist) and each row's receipt; head for confirmations | none |
| Wallet › Identity | `wallet` (tab) | `auth_userinfo`, `CitrateMemberSBT.isActive`, `sbt_token_uri`, `node_proposer_identity`, `groups_self_address`; network counts from the fixture | none |
| Groups › Chat › Groups | `groups/chat` | `groups_list`, `groups_messages`, `groups_send`, `groups_create`, `comms_relay_status`; paste: `pending_join_set` / `pending_join_resolve` (#355), then `group_invite_redeem` on one Join click | round cards: `VITE_FL_ROUND_CARDS`; invite links: join service (GROW-S1) |
| Groups › Chat › Alerts | `groups/chat/alerts` | comms notifications: **none yet** as a command | none |
| Groups › Rounds | `groups/rounds` | address book, then `contract_code_size` for FederatedRoundLedger and PatronageLedger (#361); `trajectories_settings_get` | `VITE_FL_ROUND_CARDS` (off); "Group rounds not live yet" |
| Groups › Members | `groups/members` | `groups_roster`; `groups_assign_role`, `groups_offboard` through the ceremony; `join_mint`, `group_invite_create`, `group_invites`, `join_revoke`, `group_invite_revoke` (#355); inviter states from the rendezvous (#374) | join service (GROW-S1), rendezvous (GROW-S2) |
| Groups › Cluster | `groups/cluster` | `cluster_status`, `cluster_join`, `cluster_leave`, `cluster_share_file`, `device_links`, `device_link_*` | none |
| Groups › ALF | `groups/alf` | `auth_userinfo` (alf claim) | ALF claim |
| Groups › Community (hidden) | `groups/community` | `groups_list` for the open network | none |
| Settings (7 sections) | `settings`, `settings/connections` | `auth_status`, `auth_userinfo`, `auth_logout`, `connections`, `config_read` / `config_write`, `custody_status` / `custody_lock` / `custody_ensure_unlocked`, `membership.entitlement`, `membership_price` (#379), `membership_grant_status`, updater, telemetry and funnel opt-in (#365) | billing: counsel memo before the money copy ships |

Every screen has its empty, error and unavailable variant. Errors and unavailable states name the call and what it returned.

## UI truth contract decisions

- **Validator word.** `validatorWord` mirrors core `chainTruth.ts`: Exiting and Slashed come first from the registry status; "Validator active" needs `isActive` and `actEpoch ≤ currentEpoch()`; an unread `actEpoch` or epoch reads "Registered · active from epoch —"; reads that disagree show "—". Node's validator switch gained Exiting and Slashed so both places agree.
- **Unknown is not zero.** The C1 fixture has no per-member reads, so balance, stake, grant amount, rewards, lock end and deposit read "—". The "read" balance variant draws `N SALT`, never a number. Send fails closed while the balance is unread: the over-balance check can't run, so nothing reaches the ceremony.
- **One staked figure.** Staked is `stakeOf(pubkey)` only. A locked bond reads "Bond locked · not staked"; a LiquidStakingPool deposit is labelled "not validator stake", and its buttons are drawn off with the reason.
- **Durations.** The withdrawal delay reads "50,400 blocks (~28 h)" from the measured ~2 s block time (V6).
- **Transactions.** Pending, then "Included in block —" (no receipt is read). Confirmations are "a depth count, not finality". No final, settled or irreversible anywhere.
- **Money.** The grant uses core's `grantCustody` wording with the §5.7 disclaimer and the §5.6 betanet line. No SALT price, no USD, no investment words. The membership price follows `membershipPriceLabel` (#379): with no read price, every surface links "See membership pricing" to `https://citrate.ai/membership` (not membership.citrate.ai, per the #349 review); the read variant draws the served `⟨displayAmount⟩ a year` slot, not an amount. The two old `membership.citrate.ai` links (0.5.1 onboarding, Settings) now point at citrate.ai/membership.
- **Groups.** "Joined ⟨cluster⟩" appears only once the group is in the list (GROW-S2 §11). An unknown code is "We couldn't find that invite", never expired. Rounds say "Group rounds not live yet" with the flag off or on; the fixture round states (Committed, Accepted, Rejected) appear only with the flag on and carry the fixture tag. "Deployed · dormant" is shown as proposed for §1 (core #361 asked the chain team).
- **Settings.** The old 0.5.1 Settings block is gone: no "behind checkpoint" health line (finality isn't running), no "passkey validator · ACTIVE", no "ATTESTED", no allowlist signature policy (Rule 3 allows only budgeted SIWE), keystore export drawn off as core ships it. Verification states include "not verified (optional for membership)" after a skipped Verify step (slice 2 V-03).

## Click paths

All three run end to end; slice 3 changes only path 1's landing.

1. **Join**: the onboarding Join step now adds the group to Groups (`groups_list`), so Groups › Chat opens it with a welcome line, instead of the slice-2 stub card.
2. **Everyday**: unchanged (Controls › Web search, Store, install paused, start, approve).
3. **Operator**: unchanged (Services › IPFS Quarantined, log, restart, pin with no bond).

## Tests

`scripts/design-prototype.test.mjs` adds 105 tests (64 → 169): the Groups and Wallet deep links, the validator word table, Wallet unknown-not-zero and the fail-closed Send, the ceremony for sends, activation and roles, the price label, the Groups join and invite flows, rounds behind the flag, ALF gating, an annotation check over every screen of the five places and Settings, and two copy tripwires (view values in every state and persona, and the slice-3 template text) for banned words and price numbers.

## Revised 2026-10-22: Codex round 2 (880e2b5)

- **App-only invites copy their own link.** With the join service down, the row showed a `citrate://invite` link but Copy link copied a citrate.ai/join link. Each invite now gets its own key and link (citrate.ai short link, or the app-only `citrate://invite?g&t&k` fallback), and Copy copies exactly that. The link lives in memory only, never in saved state (core #355: no key in the webview's storage); after a reload the row says to revoke and create a new one.
- **ALF needs the claim in every variant.** The empty variant used to show ALF content without the claim. Without it, every variant except error shows "ALF members only"; error says the claim couldn't be checked.

## Open questions

For core:

1. **Wallet tabs as routes.** K0 has one `wallet` route. Should Overview, Staking, Activity and Identity be routes (`wallet/staking`) so Settings and Hermes can deep-link to them? The prototype accepts them.
2. **Send with an unread balance.** The prototype refuses. Core checks `amt > s.liquid`, which passes against a stale or default value. Should `wallet_send` refuse when the balance read failed?
3. **Activation outside onboarding.** Wallet › Staking offers Activate for a locked bond. Is that the second entry point, or Node › Status (slice 2 question 8)?
4. **Alerts.** No notifications command exists. Is Alerts in 0.5.2 scope, or should the tab say "not connected yet" in the build?
5. **Liquid path.** Add stake and Withdraw are drawn off. Does 0.5.2 keep them while the money-path decision is open?

For the chain team:

6. Exiting and Slashed for Wallet and Node: confirm the words and that rehearsal 1 can export one of each.
7. A sample member's balance, stake, bond principal and lock end in the rehearsal-1 export (ruling 7) so v2 can replace the "—" values.
8. "Deployed · dormant" for the round ledgers (core #361).

## Known debt

- The 0.5.1 Wallet and Comms screens stay behind the reference bar.
- The Store stub values from slice 1 (`stPerms`, `stubInstall`) are still in the file; click path 2's test uses `stubInstall`.
