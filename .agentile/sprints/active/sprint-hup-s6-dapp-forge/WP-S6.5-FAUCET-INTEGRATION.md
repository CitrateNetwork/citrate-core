---
created: 2026-10-01
branch: hup/n5-chain-faucet
updated: 2026-10-04
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S6
wp: HUP-S6.5
---

# HUP-S6.5: faucet integration (core + chain), federation F-5

Planset `2026-09-30-hermes-upskill`: 05_SPRINTS_AND_WPS (S6.5), 04_FEATURES_BDD US-6.5,
ADR-2026-10-01-faucet-for-deploy-gas (proposed). Branches: citrate-core `hup/n5-chain-faucet`
(base `release/0.5.0-hermes-upskill` @ 00e6353), citrate-chain `hup/n5-chain-faucet` (base
`main` @ 0aab474b).

## Acceptance (US-6.5) against this build

| AC | Status | Evidence |
|---|---|---|
| AC1 `faucet_request` calls the faucet; anti-abuse challenge handled in-app; origin allowlisted | Built, not deployed | Core: one unsigned POST for the member's wallet (`faucet_tests.rs`); CAPTCHA answer opens the faucet's page in an in-app window with no capability (`no_capability_names_the_challenge_window`, `the_challenge_window_stays_on_the_faucets_origin`). Chain: CAPTCHA page + token, `FAUCET_DESKTOP_ORIGINS` (`s65_captcha_page_is_opt_in`, `s65_desktop_origins_are_opt_in_and_exact`). Needs the operator deploy. |
| AC2 respects the 24 h per-address limit and shows the next eligible time | Built | App window (`the_window_reopens_after_24_hours_and_is_per_wallet`), faucet `next_eligible_at` and `/eligibility` (`s65_eligibility_is_read_only_and_reports_the_next_time`, anvil test), old-faucet text fallback (`rate_limits_new_and_old_faucets`). |
| AC3 sits under an HIC-2 budget | Built | Off by default; HIC-1 grant in Settings; per-wallet; revocable (`grant_and_revoke_persist_privately_and_revoke_keeps_history`, `off_by_default_sends_nothing_and_points_at_the_page`). MCP tool runs only inside it and takes no recipient (`the_mcp_tool_runs_at_once_with_the_callers_origin_and_no_recipient`). |

## Gates run

- citrate-chain `cargo test -p citrate-faucet`: 37 -> 62 passed, including a live drip on a local
  anvil (chain id 40204, throwaway key made at run time). Mutation checks: membership bypass,
  cap bypass, fake readiness, slot not returned: each killed. clippy -D warnings clean on 1.96.0
  and 1.98.1; gitleaks clean; chain tripwires clean.
- citrate-core: `faucet::` 33 tests; `cargo test --workspace --locked` 1666 -> 1699 passed, 0 failed, 11 ignored; node_mcp, popout ACL and main-thread tripwire green; clippy
  1.98.1 -D warnings clean; fmt check clean; tsc clean; vitest 1663 passed, 10 skipped.

## Review and finish (fan-out 6, 2026-10-04)

Branches: citrate-core `hup/n5-chain-faucet` (merged `origin/hup/m2-core`; conflicts in the
command list and the main-window ACL, both sides kept), citrate-chain `hup/n5-chain-faucet`
(main has not moved), citrate-agent-runtime `hup/n5-chain-faucet` (new, from
`origin/hup/m2-runtime`).

Review findings, each fixed with a test that failed first:

1. The in-app challenge window allowed only the faucet's own origin, but the webview asks the
   navigation handler about every frame (wry 0.55 on macOS passes iframe navigations too) and
   the faucet page embeds the CAPTCHA as an iframe from the provider. The CAPTCHA could not
   render, so AC1 could not work in-app. The window now allows the faucet origin, exactly
   `https://challenges.cloudflare.com`, and `about:blank`/`about:srcdoc`
   (`the_challenge_window_lets_the_captcha_frame_load_and_nothing_else`).
2. Chain: with the membership check on, a caller already in cooldown still cost one `eth_call`
   per request. Now answered from memory first
   (`s65_a_caller_in_cooldown_costs_no_membership_rpc`).
3. Chain: the anvil drip test read the balance right after `eth_sendRawTransaction` and failed
   about one run in three on the unchanged branch. It now waits for the mined balance.

Gaps closed:

- Decision records (ADR D4.3): grant, revoke and every faucet call go to core's HIC outbox
  (`faucet.budget_granted`, `faucet.budget_revoked`, `faucet.topup`; HIC-2 when Hermes or an MCP
  client asked). Fail closed for the call and the grant; revoke never blocked. The sidecar
  accepts the kinds (runtime branch; merge it first).
- Integration against a real faucet binary: `scripts/e2e-faucet.sh --chain <checkout>` runs
  `e2e_faucet_binary_drips_once_then_both_sides_report_the_next_time` (anvil, chain id 40204,
  throwaway key made at run time). Passed 2026-10-04.

## Not done

- ADR acceptance and owner answers O-1 to O-4 (all values are placeholders).
- Deploying the faucet change (DGX team, asked on the sprint issue).
- Faucet calls are in the decision records, but the activity monitor does not show them as a
  separate row (a Hermes call shows as its tool call in the turn activity).
- No packaged-app run: the in-app challenge window and the live faucet were not exercised from
  the installed app.
