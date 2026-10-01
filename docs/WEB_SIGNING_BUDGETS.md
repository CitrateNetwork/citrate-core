---
created: 2026-10-01T10:00:00Z
branch: hup/n4-siwe-budgets
author: Larry Klosowski + Claude Opus 5.5
status: implemented (auto-sign inactive until the managed browser attests origins, HUP-S5.1)
wp: HUP-S2.3
adr: docs/adr/ADR-2026-09-30-rule3-budgetable-signatures.md (accepted)
formal: src-tauri/formal/WebSigningBudget.tla
---

# Web-signing budgets (Sign-In with Ethereum)

US-2.3: "Sign into dApps for me, safely." A member can let Hermes sign in with their wallet on a
site they chose, a bounded number of times, without a click per login. Everything else still asks.

## What is built

| Piece | Where |
|---|---|
| Strict EIP-4361 parser, re-serializer and the D2 #6-18 checks | `kit/src/siwe.rs` |
| Budget store: budgets, counters, nonce ledger, rolling-window reservations, hash-chained decision records, HMAC-sealed file, crash recovery | `kit/src/web_budget.rs` |
| The one budgeted signer path (check, reserve, record, sign under one lock) | `SignatureCeremony::request_siwe_budgeted` in `kit/src/ceremony.rs` |
| x402 budget type (B-2), separate from B-1 and inert | `X402Budget`, `X402_ASSET_ALLOWLIST` (empty) in `kit/src/web_budget.rs` |
| Commands: `web_budget_status`, `web_budget_grant`, `web_budget_revoke`, `web_budget_revoke_all`, `web_budget_reset`, `web_signing_request` | `src-tauri/src/web_budgets.rs` (main-window ACL only) |
| Settings → Budgets: grant, list with countdowns, revoke, revoke all, decision records, reset after an integrity failure | `src/budgets/BudgetsPanel.tsx`, `src/budgets/budgets.ts` |

## Honest state

- **Auto-sign is not active yet.** D2 #1 requires core to read the top-frame origin over its own
  session with the managed browser. That browser is HUP-S5.1 and is not built, so
  `web_budgets::attest_origin` returns nothing and every `web_signing_request` becomes an
  ordinary approval card that says why. Budgets can be granted, viewed and revoked today; they
  apply as soon as the attestation source exists, with no other change.
- **No caller of `web_signing_request` exists yet.** The browser tool that will raise sign-in
  requests is also S5.1 work. The command and its tests are ready for it.
- **The nightly anchor does not include the records yet.** Records are hash-chained and the
  head hash is exposed (`snapshot.headHash`); folding it into the anchor batch is HUP-S7.3.
- **x402 (B-2) is inert.** The asset allowlist is empty until a wrapped-SALT token exists
  (owner decision O-1). Payment requests are always approval cards.
- **No budgets by default.** With no budget, behaviour is exactly as before: every sign-in asks.

## Values pending owner sign-off

| Value | Placeholder | Fixed by the accepted ADR |
|---|---|---|
| Sign-ins the grant form proposes | 10 (`PLACEHOLDER_DEFAULT_MAX_COUNT`) | ceiling 50 (O-2) |
| Days the grant form proposes | 7 (`PLACEHOLDER_DEFAULT_TTL_DAYS`) | ceiling 30 days (O-2) |
| Burst gap and rolling rate | | 30 s, 20 per origin per 24 h (D2 #21) |
| Same-origin content does not taint | | yes (O-3) |
| Grant is a card decision, no signature | | yes (O-4) |

The UI says the two proposals are placeholders pending owner sign-off. They only pre-fill a form;
nothing is granted until the member clicks.

## Invariants and the tests that pin them

`WebSigningBudget.tla` (TLC green, mutation-checked in core#129) names the properties. The
implementation keeps each one, and a test fails if it does not.

| TLA+ property | Implementation | Tests |
|---|---|---|
| `OnlyClosedList` | `SiweSignRequest` carries message text only (no kind); the path calls only `sign_personal`; a non-SIWE text fails the strict parse | `budget_path_never_signs_typed_data_or_plain_text`, `budget_tripwire_the_budget_modules_never_reach_a_signer`, `d2_6_strict_parse_rejects_malformed_input` |
| `OriginBound` | `domain` and `URI` origin must equal the attested origin, which must have a budget | `d2_7_*`, `d2_8_*`, `origin_bound_and_top_frame_only` |
| `TopFrameOnly` | attestation must be `Top` + `Managed`; none means a card | `origin_bound_and_top_frame_only`, `unattested_or_subframe_requests_become_cards` |
| `NonceUnique` | per-origin nonce ledger, persisted, checked under the lock | `nonce_unique_per_origin`, `a_replayed_nonce_becomes_a_card`, `budget_monotone_across_restart` |
| `NoCapabilityDelegation` | any `Resources` line is refused | `d2_17_any_resources_line_is_never_budgetable` |
| `NeverExceedsCaps` | `used < max_count`, 30 s gap, 20 per rolling 24 h, ceilings at grant | `never_exceeds_caps_count_gap_and_window`, `rolling_window_caps_at_twenty_per_origin`, `caps_hold_through_the_signer`, `grant_never_widens_a_live_budget_and_respects_ceilings` |
| `BudgetMonotone` | counters, ledger and records persist; a grant never widens a live budget | `budget_monotone_across_restart`, `grant_never_widens_*` |
| `RevokeImmediate` | revoke and revoke-all take the same budget lock the signer path holds | `revoke_immediate_and_revoke_all`, `revoke_takes_effect_before_the_next_request`, `revoke_waits_for_an_in_flight_auto_sign_and_then_wins` |
| `ExpiredInert` | budget expiry checked at decide time and re-read just before signing | `expired_inert`, `still_signable_rechecks_expiry_after_the_reservation`, `expiry_between_reserve_and_sign_signs_nothing` |
| `TaintDowngrade` | unknown or foreign taint means a card; O-3 exempts only the same origin; `hic: "required"` always a card | `taint_downgrade_with_the_o3_same_origin_exemption`, `tainted_or_hic_required_requests_become_cards`, `hic_required_calls_never_take_the_budget_path` |
| `RecordBeforeSignature` | the `Reserved` record is persisted before the signer runs; a failed save signs nothing | `happy_path_reserves_and_writes_the_record_first`, `write_ahead_failure_signs_nothing_and_reserves_nothing`, `auto_sign_within_budget_recovers_to_the_wallet` |
| `NoFalseNegative` (crash) | on open, a `Reserved` record becomes `outcome_unknown`, never `not_signed` | `crash_recovery_marks_open_reservations_outcome_unknown` |
| `FallThroughLive` / `NoDrop` | every failing request returns a pending ceremony with the same message | every `*_becomes_a_card(s)` test asserts `status(id)` is pending |

Store integrity (D4 "Storage"): `tampered_store_fails_closed_until_reset`,
`keychain_unavailable_fails_closed`, `records_are_hash_chained`.

## Model-to-code notes

- The model keeps the nonce ledger and reservations forever. The code prunes a ledger entry once
  its message has expired plus the 60 s skew (a replay then fails the expiry check), and drops
  reservations older than the rolling window (they can no longer count). Neither prune can change
  a decision.
- The model's `Grant` requires no live budget ignoring count. The code matches: a used-up but
  unexpired budget must be revoked before a new grant for the same origin.
- Wallet change (D4 voiding) is checked at decide time: a budget granted for another wallet is
  shown as "Different wallet" and never signs.
