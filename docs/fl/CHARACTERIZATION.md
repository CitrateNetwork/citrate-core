---
created: 2026-11-27
branch: feat/v60-l8-wp01-fl-characterization
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# FL characterization (L8-WP01)

What the federated-learning code does today (citrate-core `a826e441`), pinned by name before any
0.6.0 lane L8 (Train together) work package changes it. The canonical list is the registry
[`ci/characterization.d/l8.json`](../../ci/characterization.d/l8.json). When LQ-WP12 (the shared
regression suite runner) lands, LQ runs every suite in it by that registry on every merge-queue
group; until then nothing in CI reads the registry, and CI runs the suites inside the full cargo
and vitest suites. This page is the human view of that registry, and
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
| `today_training_status_reads_patronage_only` | `training_status` and `training_reward` read the book's PatronageLedger only (`roundMergedHash`, `units`, `pendingDividendOf`, pinned selectors), and no training write command exists (`training.rs:1-7`, commands `:182`, `:211`). The blocking bodies (`training_status_sync` `:191`, `training_reward_sync` `:222`) build `RpcClient::citrate()` themselves, so their source is pinned: the ledger only from `patronage_ledger()?` (the one `crate::addresses` read), `RpcClient::citrate()` twice, the three pinned selectors only, and no call beyond `scan_rounds(&rpc, ledger, &gid)`, `read_patronage(&rpc, ledger, &member)` and the round lookup | L8-WP12 | the live round view adds FRL and coordinator reads beside these, which stay byte for byte |
| `payouts_gate_refuses_workmarket_writes_while_dark` | kit `check_intent` under the shipped r1007 profile: every distinctive WorkMarket selector to an unpinned address is refused as not this network's WorkMarket; with a WorkMarket pinned and payouts dark, every WorkMarket write (each money selector, an unknown selector, a plain transfer) is refused, and no code is read | L8-WP17 | the same refusals, plus the flip test: a profile with `payoutsOpen` and matching pins signs, from the same code |
| `payouts_gate_passes_frl_commit_and_finalize` | FRL `commitRound` and `finalize` are not money classes: the gate passes them with the flag off or on, and reads no code; an FRL challenge is still refused while dark | L8-WP17 | (lane invariant: coordinator rounds commit to FRL while payouts are dark) |
| `collector_safety_tests_stay_declared` | the eight collector safety tests below stay declared and live in `src-tauri/src/fl_collect_tests.rs`: at module scope (brace depth 0, braces in literals and comments aside), under `#[test]`, with no `#[ignore]`, `#[should_panic]`, `cfg_attr` or `cfg` other than `cfg(test)` anywhere in the attribute block, and no block comment or `allow(unnameable_test_items)` in the file, and that file stays compiled in: `fl_collect.rs` includes it, once and uncommented, in its column-0 `#[cfg(test)] mod tests { ... }` with no other attribute on that module and no inner `#![cfg...]`, and `lib.rs` declares `mod fl_collect;` at column 0 with no attribute | (lane invariant) | none: the collector rules 1-8 (`fl_collect.rs` header) do not change in 0.6.0 |
| `collector_check_rejects_an_ignored_or_cfgd_out_test` | the check above fails a safety test that is `#[ignore]`d above or below `#[test]`, on the same line, behind a trailing comment, cfg'd out or behind a `cfg_attr`, and passes another attribute (an `allow`) and `cfg(test)` | (lane invariant) | none |
| `collector_check_rejects_a_test_in_a_cfgd_out_module` | the check above fails a safety test inside a module that is cfg'd out (`cfg(any())`, `cfg(not(test))`, a wrapped or same-line `cfg`, one after `cfg(test)`), behind an inner `#![cfg(...)]`, or inside any `mod` at all, and passes a `derive` and `cfg(test)` | (lane invariant) | none |
| `host_check_rejects_a_cfgd_out_test_module` | the host check above fails a test module under `cfg(any())` or `cfg(not(test))`, an attribute stacked above or below `#[cfg(test)]`, a commented or cfg'd include, an inner `#![cfg(...)]`, a block comment, a missing or doubled include; and the `lib.rs` check fails a cfg'd, commented, nested or missing `mod` line and a crate-level `#![cfg(...)]` | (lane invariant) | none |
| `collector_check_rejects_a_nested_or_inverted_test` | the check above fails a safety test nested in a block that is not a module (a fn body, with or without `allow(unnameable_test_items)`, a closure, an `impl`, a `const` block), one under `#[should_panic]`, one hidden in a block comment, and the review's case on the real file (the DNS-rebinding guard wrapped in a fn); it passes braces inside a string, a char, a raw string or a trailing comment | (lane invariant) | none |

This suite cannot report its own host chain cfg'd out (`fl_payouts.rs`'s test module, or `mod
fl_payouts` in `lib.rs`), since it would then not run; the registry check below covers that chain
and every other protected Rust file's.

### `L8-REGISTRY`: the registry check (vitest)

File: `src/fl/flRegistry.characterization.test.ts`. Tests: `registry_lists_every_l8_suite_once`,
`every_listed_test_is_declared_and_every_declared_test_is_listed`,
`cargo_suite_is_compiled_in_where_the_registry_says`,
`protected_suites_keep_their_counts_and_named_tests`,
`protected_cargo_files_are_compiled_in`, `no_protected_or_l8_vitest_suite_is_skipped`,
`no_pinned_or_protected_cargo_test_is_disabled`,
`characterization_doc_names_every_pinned_test`, `declaration_checks_ignore_commented_out_tests`,
`cargo_checks_reject_an_ignored_or_cfgd_out_test`,
`cargo_checks_reject_a_test_in_a_cfgd_out_module`, `host_checks_reject_a_cfgd_out_test_module`,
`cargo_checks_reject_a_nested_or_inverted_test`,
`vitest_checks_reject_a_skipped_or_focused_describe`.

A test that no longer runs counts as lost, at every level it can be turned off:

- **A Rust test.** One under `#[ignore]`, a `cfg_attr` or a `cfg` other than `cfg(test)` is not
  declared and does not count, so a protected or pinned file whose test is turned off fails the
  same way as one that lost the test. So is one under `#[should_panic]`: it inverts the test, which
  then passes only while the behavior it guards is broken (as vitest `.fails` does, below).
- **A Rust test nested in a block.** rustc collects a `#[test]` fn at module scope only; one inside
  a fn body, a closure, an `impl` or a `const` block never runs, and
  `allow(unnameable_test_items)` silences the lint that says so. So a test counts only when every
  block around it is a module body (braces in strings, chars, raw strings and comments aside), and
  no protected or pinned file carries `unnameable_test_items` or an unbalanced brace. The Rust-side
  collector check applies the same rule (brace depth 0) and also fails on any block comment.
- **A Rust module inside a file.** A `cfg` on an enclosing module turns off every test in it while
  each fn keeps its own `#[test]` block. So no protected or pinned Rust file carries, anywhere in
  its code (strings and comments aside), an `#[ignore]`, a `cfg_attr`, a `cfg` other than
  `cfg(test)`, an inner `#![...]`, or a `mod` item; the L8-RUST file's own opening
  `mod lq_l8_fl {` is the one module allowed.
- **A Rust file.** Each protected Rust file names its `host` in the registry. The host must include
  it, once and uncommented, in its column-0 `#[cfg(test)] mod tests { ... }` (include lines only),
  with no other attribute on that module and no inner `#![cfg...]`; and `lib.rs` must declare the
  host module as a plain column-0 `mod <host>;` with no attribute. The L8-RUST file must be a
  top-level include of a protected file (after a closed item, no attribute) and open with its
  `mod lq_l8_fl {`. So cfg'ing out `fl_collect.rs`'s test module (all 27 collector tests) or
  `fl_payouts.rs`'s (this lane's own Rust suite) fails here.
- **A vitest test or describe block.** A protected or L8 vitest file carries, outside comments, no
  `.skip`, `.only`, `.todo`, `.skipIf`, `.runIf` or `.fails` on any call (`describe`, `it`, `test`
  or a context), no bracketed form of them, no `{ skip: true }`-style option and no x-prefixed
  `describe`, `it` or `test`. A `describe.skip` keeps every name and the count while nothing runs,
  so the count and name checks alone would not see it.

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

## Open at WP close

- **LQ CI job names it.** The acceptance item stays open until LQ-WP12 lands the registry runner
  (lane LQ, "Shared regression suite and characterization registry"). LQ-WP02 (the `lq / ratchet`
  job) is already on main and does not read `ci/characterization.d/*.json`.
- **A name-based check.** The checks here read source text, so each new way to turn a test off
  needs its own rule. The lasting fix is to compare the registry's names with what the runners
  report: `cargo test -- --list` and the vitest JSON reporter's test names. That belongs to LQ-WP03
  or LQ-WP12 (the LQ ratchet today counts only names matching `tripwire`).
- **Protected counts.** Counts are compared with `>=`, and only 8 of the 27 collector tests are
  named, so an unnamed test can be swapped for a new one. The runner-count ratchet (LQ) is the
  check for that, not these text checks.
- **The gateSec refusals.** `today_training_status_reads_patronage_only` pins the "@rule8-gated
  (gateSec not yet passed)" statement where it lives, in `training.rs`'s module doc (no code
  returns it, since no training write command exists), and the "not confirmed on 40204" refusal
  where `patronage_ledger()` returns it (the shipped book never reaches it). The WP that adds a
  training write path asserts its refusal on the value the code returns.

## Where the files live

The lane file named `src/fl/characterization.test.tsx`. The suite is at
`src/fl/fl.characterization.test.tsx` instead, so that `npm run test:characterization` (which
selects `.characterization.test.`) picks it up with the other lanes' suites. The Rust file keeps
the lane file's name and is included from an L8-owned test file, as the L1, L4 and L5 suites are.
