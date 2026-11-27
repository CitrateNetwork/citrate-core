---
created: 2026-11-27
branch: feat/v60-l8-wp01-fl-characterization
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# FL characterization (L8-WP01)

What the federated-learning code does today (citrate-core `a826e441`), pinned by name before any
0.6.0 lane L8 (Train together) work package changes it. The canonical list is the registry
[`ci/characterization.d/l8.json`](../../ci/characterization.d/l8.json); LQ runs every suite in it on
every merge-queue group. This page is the human view of that registry, and
`src/fl/flRegistry.characterization.test.ts` fails if the two drift apart.

**The rule.** A test here, or a protected test below, is replaced only by a stricter one, only by
the WP named in its row, in a separate commit that names the suite id and says why. Deleting one,
or lowering a protected file's test count, needs an ADR (Rule 2).

## Pinned suites

### `L8-TODAY`: network state and the dark writes (vitest)

File: `src/fl/fl.characterization.test.tsx`. Covers `src/fl/networkState.ts`, `src/fl/Matches.tsx`,
`src/fl/roundDraft.ts`.

| Test | Pins | May be replaced by | Stricter replacement |
|---|---|---|---|
| `today_patronage_reads_unchanged` | PatronageLedger is read with `totalUnits()` only (selector `0x6d86acc4`), one `eth_call` to the book's address; zero is "deployed, no rounds", a bad or failed read is unknown with the reason (`networkState.ts:153-162`) | L8-WP17 | the paid-half reads from `fl_payouts_status` and the pins, with the same PatronageLedger read byte for byte |
| `today_frl_rounds_inferred_from_workmarket` | FRL rounds are never read: "no rounds" is inferred from WorkMarket missing, "read per offer" when it is there (`networkState.ts:163-168`) | L8-WP12 | `ring_state_from_frl_only`: FRL `getRound(roundId)` per catalogue round |
| `today_phase_today_without_workmarket` | the phase matrix of `phaseFor` (`networkState.ts:116-125`): "today" without WorkMarket, "unknown" with the failing read named, "rounds-live" only with WorkMarket and FRL deployed; the payouts gate is never asked without WorkMarket | L8-WP16 | `round_card_matches_frl_deployed_and_catalogue_state`: the phase also reads the coordinator catalogue |
| `today_join_refused_while_dark` | `joinIntent` refuses first with `JOIN_WAITS_FOR_GSEC` whenever payouts are not open (`Matches.tsx:50-53`) | L8-WP17 | `dark_has_no_money_controls`, with the join built only when core's gate says open |
| `today_open_refused_while_dark` | `openIntent` refuses first with `OPEN_WAITS_FOR_GSEC`, plus core's reason when it gave one (`roundDraft.ts:300-306`) | L8-WP13 | `paid_section_uses_OPEN_WAITS_FOR_GSEC`, with the unpaid group-round path beside it |

### `L8-RUST`: training reads, the payouts gate, the collector (cargo)

File: `src-tauri/src/fl_characterization_tests.rs`, compiled into `fl_payouts::tests` from
`src-tauri/src/fl_payouts_tests.rs`; LQ filter `lq_l8_fl::`.

| Test | Pins | May be replaced by | Stricter replacement |
|---|---|---|---|
| `today_training_status_reads_patronage_only` | `training_status` and `training_reward` read the book's PatronageLedger only (`roundMergedHash`, `units`, `pendingDividendOf`, pinned selectors), and no training write command exists (`training.rs:1-7`, commands `:182`, `:211`) | L8-WP12 | the live round view adds FRL and coordinator reads beside these, which stay byte for byte |
| `payouts_gate_refuses_workmarket_writes_while_dark` | kit `check_intent` under the shipped r1007 profile: every distinctive WorkMarket selector to an unpinned address is refused as not this network's WorkMarket; with a WorkMarket pinned and payouts dark, every WorkMarket write (each money selector, an unknown selector, a plain transfer) is refused, and no code is read | L8-WP17 | the same refusals, plus the flip test: a profile with `payoutsOpen` and matching pins signs, from the same code |
| `payouts_gate_passes_frl_commit_and_finalize` | FRL `commitRound` and `finalize` are not money classes: the gate passes them with the flag off or on, and reads no code; an FRL challenge is still refused while dark | L8-WP17 | (lane invariant: coordinator rounds commit to FRL while payouts are dark) |
| `collector_safety_tests_stay_declared` | the eight collector safety tests below stay declared under `#[test]` in `src-tauri/src/fl_collect_tests.rs`, and that file stays compiled in | (lane invariant) | none: the collector rules 1-8 (`fl_collect.rs` header) do not change in 0.6.0 |

### `L8-REGISTRY`: the registry check (vitest)

File: `src/fl/flRegistry.characterization.test.ts`. Tests: `registry_lists_every_l8_suite_once`,
`every_listed_test_is_declared_and_every_declared_test_is_listed`,
`cargo_suite_is_compiled_in_where_the_registry_says`,
`protected_suites_keep_their_counts_and_named_tests`,
`characterization_doc_names_every_pinned_test`, `declaration_checks_ignore_commented_out_tests`.

## Protected honesty list (handed to LQ)

Existing suites L8 keeps. Each file keeps at least its test count, and each named test stays
declared until the WP in its row replaces it with the stricter test named there.

| File | Count | Named tests | May be replaced by | Stricter replacement |
|---|---|---|---|---|
| `src/surfaces/alfSurfaceHonesty.test.tsx` | 6 (all) | `shows the workbench with an honest 'activating soon' seam and never fakes a round`, `points ownership at the web portal (node = workbench, web = trophy case)`, `shows the real node status, honestly (sync line / synced height), never a fabricated block or a %`, `a signed-in non-member gets an honest prompt, not the workbench`, `shows the ALF tab for an ALF member, and never as a sidebar place`, `hides the ALF tab for a non-member` | L8-WP16 | `alf_contribute_opens_train_together_from_catalogue`, `alf_no_activating_soon_when_round_listed`, `alf_never_fakes_a_round_when_catalogue_empty` (keeps the ban on a fake "Start round") |
| `src/fl/flFoundation.test.tsx` | 49 (all) | `pins the three block-0 ledgers and not WorkMarket, FLRoundVerifier or DatasetRegistry` | (none planned) | only a stricter pin when the book changes in a reviewed address-book PR |
| `src/surfaces/node/nodeTabs.test.tsx` | 32 | `pin and round rewards are not live` (line 276) | L8-WP16 | `round_card_matches_frl_deployed_and_catalogue_state` |
| `src-tauri/src/fl_rounds_tests.rs` | 63 | `start_records_the_authorization_and_says_no_training_ran` | L8-WP10 | `receipt_says_training_scheduled_only_when_worker_installed` |
| `src-tauri/src/network_profile_payouts_tests.rs` | 8 | `r1007_ships_with_payouts_dark`, `no_other_profile_opens_payouts` | (none: X-8, owner sign-off only) | none |
| `src-tauri/src/fl_payouts_tests.rs` | 3 | `the_shipped_network_reports_payouts_dark_without_reading_the_chain` | (none: X-8, owner sign-off only) | none |
| `src-tauri/src/fl_collect_tests.rs` | 27 | `a_page_robots_disallows_is_refused`, `no_robots_txt_and_no_license_is_not_permission`, `denied_and_private_hosts_are_refused_before_any_fetch`, `a_429_or_403_stops_the_run_and_nothing_more_is_fetched`, `robots_is_checked_again_at_run_time`, `an_approved_run_writes_provenance_through_the_store_and_records_the_scan`, `bypass_closure_the_store_is_out_of_reach_of_the_agents_file_tools`, `every_fetch_is_pinned_to_the_one_vetted_answer_and_a_rebinding_answer_is_never_used` | (none) | none |

## Where the files live

The lane file named `src/fl/characterization.test.tsx`. The suite is at
`src/fl/fl.characterization.test.tsx` instead, so that `npm run test:characterization` (which
selects `.characterization.test.`) picks it up with the other lanes' suites. The Rust file keeps
the lane file's name and is included from an L8-owned test file, as the L1, L4 and L5 suites are.
