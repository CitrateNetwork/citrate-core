---
created: 2026-10-01T10:00:00Z
updated: 2026-10-01T20:00:00Z
branch: hup/n5-siwe-rest
author: Larry Klosowski + Claude Opus 5.5
status: implemented and wired in the managed browser (off by default with the browser); not yet run in the packaged app
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
| Commands: `web_budget_status`, `web_budget_grant`, `web_budget_revoke`, `web_budget_revoke_all`, `web_budget_reset`, `web_signing_request`, `web_signing_approve`, `web_signing_reject` | `src-tauri/src/web_budgets.rs` (main-window ACL only) |
| The live path: read the request from the sidecar, attest the origin, decide, deliver, export records | `src-tauri/src/web_signin.rs` |
| The page side: an EIP-1193 provider in the managed browser's top frame whose `eth_requestAccounts` and `personal_sign` wait for core | citrate-agent-runtime `agent-browser/src/signin.rs`, sidecar `GET /browser/sign-in`, `POST /browser/sign-in/answer` |
| Copies of the decision records in the log the nightly anchor batches | sidecar `POST /records/web-signing` (citrate-agent-runtime `agent-sidecar/src/web_signing_records.rs`) |
| Settings → Budgets: grant, list with countdowns, revoke, revoke all, decision records, reset after an integrity failure | `src/budgets/BudgetsPanel.tsx`, `src/budgets/budgets.ts` |
| "Signed for you" notice with "Revoke this budget" | `src/budgets/SignedForYou.tsx` (core emits `web-budget://auto-signed`) |
| Sign-in cards at the Signature Ceremony (review kind `web-sign-in`) | `src/shell/store.ts` `handleWebSignIn`, `src/components/BrowserControls.tsx` |

## How a sign-in flows

1. A page in Hermes's managed browser calls `eth_requestAccounts` or `personal_sign` on the
   provider the browser worker installed in its top frame. The request waits in the sidecar.
2. The main window sees the waiting id in the browser status and passes **only the id** to
   `web_signing_request`. The webview supplies no message, origin, taint or `hic` flag.
3. Core reads the request from the sidecar's `GET /browser/sign-in` over its own bearer channel:
   the message bytes, the origin Chrome gave the asking context, whether that context is the tab's
   top frame, and the taint the sidecar computed from every live session (D2 #19).
4. Core attests the origin itself (D2 #1): it reads the managed browser's loopback DevTools
   `/json/list`, takes the top-level URL of Hermes's tab and requires its origin to equal the
   asking context's origin. Attach mode is never attested as managed (D2 #3).
5. `eth_requestAccounts`: the member's EIP-55 address is shared only with an attested top-frame
   origin that has a live budget for the active wallet; otherwise the page gets EIP-1193 code 4100.
   `personal_sign`: `request_siwe_budgeted` signs inside a live budget, or returns a pending HIC-1
   card with the reason. A message that is not UTF-8 text is an ordinary card.
6. Core delivers the signature (or the refusal) with `POST /browser/sign-in/answer`. A card is
   delivered after the member approves it (`web_signing_approve`), or declined
   (`web_signing_reject`, code 4001).
7. After an automatic signature core emits `web-budget://auto-signed`; the main window shows
   "Signed for you" with "Revoke this budget".
8. Every grant, revoke, reset and closed signature record is copied, oldest first, into the local
   decision records the nightly anchor batches (`POST /records/web-signing`; HIC-2 `siwe` with
   `auto_within_budget`, HIC-1 `siwe_budget` for member decisions). A record still `Reserved` holds
   back itself and everything after it. The export cursor is part of the MACed budget file and moves
   only after the sidecar accepted a batch. Exports run after each decision and before each nightly
   planning pass.

## Honest state

- **Off by default.** The provider exists only while the sidecar runs with
  `CITRATE_HERMES_BROWSER=1` (default unset), and only in the managed browser. With no budget,
  every sign-in asks, exactly as before.
- **Not yet run in the packaged app.** Proven by: a real headless Chromium against a local dApp
  page (provider in the top frame only, binding hidden, accounts then `personal_sign`, answers
  delivered; `agent-browser` live tests), and core's path with the real ceremony, store, vault and
  EIP-191 signer against recorded sidecar and DevTools answers. Budgeted origins must be https with
  a domain name, so an end-to-end automatic sign-in needs a real https dApp and a member run.
- **Residual: frame provenance comes from the sidecar.** Core verifies the tab's top-level origin
  and that it matches the asking context's origin with its own DevTools read, but which frame
  raised the request comes from Chrome's execution-context events as the sidecar reports them.
  Core does not hold its own DevTools session on the frame tree. A compromised sidecar is already
  bounded by D8 (allowlisted origins, caps, records, Stop all autonomy).
- **x402 (B-2) is inert.** The asset allowlist is empty until a wrapped-SALT token exists (owner
  decision O-1). Payment requests are always approval cards.
- **The anchor itself is not live.** The records reach the batched log; `AnchorRegistry` use on
  40204 and the anchor key's gas path are HUP-S7.3 and O-5.

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

Live-path placeholders, also pending owner sign-off:

| Value | Placeholder |
|---|---|
| Who learns the member's address (`eth_requestAccounts`) | only an attested top-frame origin with a live budget; every other site is told no (no "share address" card) |
| How long a page request waits for core | 120 s, then the page is told no |
| Requests waiting at once | 4 in the browser, 8 sign-in cards in core |
| A sign-in that arrives while another review is open | declined (the page can ask again), never queued |
| "Signed for you" notices shown at once | 3 |

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
| `OriginBound`, `TopFrameOnly` (live path) | core reads the tab origin from the browser, requires it to equal the asking context's origin, and refuses budgets for a non-top frame or attach mode | `a_page_that_moved_before_core_looked_is_not_attested`, `attach_mode_a_subframe_or_an_unreadable_browser_is_never_budgeted`, `attest_takes_the_origin_from_the_browser_not_the_request`; runtime `an_embedded_frame_or_isolated_world_is_never_marked_top`, live `the_managed_browser_bridges_a_dapp_sign_in_and_hides_the_binding` |
| `TaintDowngrade` (live path) | taint comes from the sidecar's sessions, never from the webview; every source is kept, not only the first | `taint_comes_from_the_sidecar_and_other_sites_force_a_card`, `same_site_content_does_not_taint_a_sign_in_to_that_site`, `unknown_taint_or_an_empty_source_list_is_a_card`, `the_webview_can_only_name_a_request_id`; runtime `taint_folds_sessions_and_the_pages_the_model_read`, `every_taint_source_is_kept_not_only_the_first` |
| `BudgetMonotone` against a replaced file | the newest saved generation is sealed in the keychain; an older (validly MACed) file or a deleted file fails closed | `a_rolled_back_budget_file_fails_closed`, `a_deleted_budget_file_after_a_save_fails_closed`, `a_crash_between_the_file_and_the_keychain_generation_is_accepted`, `a_store_from_before_generations_opens_and_starts_counting` |
| `FallThroughLive` (live path) | a card is delivered to the page only after the member's approval, once; a decline tells the page | `an_approved_card_is_signed_once_and_delivered_to_the_page`, `only_sign_in_cards_are_approved_here_and_a_decline_tells_the_page` |
| `FallThroughLive` / `NoDrop` | every failing request returns a pending ceremony with the same message | every `*_becomes_a_card(s)` test asserts `status(id)` is pending |

Store integrity (D4 "Storage"): `tampered_store_fails_closed_until_reset`,
`keychain_unavailable_fails_closed`, `records_are_hash_chained`.

US-2.3 AC3 (records in the nightly anchor): `records_export_in_order_and_stop_at_an_open_reservation`,
`records_are_exported_in_batches_and_the_cursor_moves_only_on_success`; runtime
`a_budgeted_sign_in_becomes_an_hic2_decision_with_its_outcome`,
`records_are_written_all_or_nothing_and_verify`.

The ceremony tripwires (`adv1`, CORE-G2 and the S2.3 signer tripwire) now scan all of
`ceremony.rs` up to its terminal test module, including the budgeted path that follows an inner
test-only item: `the_tripwire_scan_covers_the_whole_production_ceremony_module`.

Mutation check (2026-10-01, this branch): removing each of these guards turns a named test red:
the origin-equality check, the accounts top-frame check, the sign-in-card check on approve, the
generation check on load, the export stop at an open reservation, the runtime top-frame check, the
answer-fits-request check, the `ext:browser` fallback, the binding removal in the provider script
(live Chromium), and recording every taint source. 10 of 10 killed.

## Model-to-code notes

- The model keeps the nonce ledger and reservations forever. The code prunes a ledger entry once
  its message has expired plus the 60 s skew (a replay then fails the expiry check), and drops
  reservations older than the rolling window (they can no longer count). Neither prune can change
  a decision.
- The model's `Grant` requires no live budget ignoring count. The code matches: a used-up but
  unexpired budget must be revoked before a new grant for the same origin.
- Wallet change (D4 voiding) is checked at decide time: a budget granted for another wallet is
  shown as "Different wallet" and never signs.
