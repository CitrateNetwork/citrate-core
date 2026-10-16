---
created: 2026-10-04
branch: hup/n6-hellomint-e2e
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S6
wp: g3-gate, g3-e2e (local half), US-6.1 (local), HUP-S11.1 (macOS part)
issue: CitrateNetwork/citrate-federation#283
---

# hello mint end to end on this Mac (fan-out 6)

## What was built

- `src-tauri/e2e/hello_mint_local.feature`: the US-6.1 Gherkin, local half, three scenarios
  (missing tools, the READY path, an injected unbounded mint).
- `src-tauri/src/hello_mint_e2e_tests.rs`: a small Gherkin reader and one handler per step.
  Every step runs the real thing: the `citrate-templates` renderer, `forge test --json`,
  Slither (the runtime's `slither_scan` arguments, SARIF), Aderyn, Medusa, a dry run on an
  anvil fork, `deploy_gate::evaluate` plus the gate store, `contract_deploy_sync` (the body of
  the `contract_deploy` command), the SignatureCeremony signing a real EIP-155 transaction from
  a fresh test vault, and the post-deploy steps. Always-on tests check that the feature parses
  and every step has a handler, so the feature and the code cannot drift.
- `scripts/e2e-hello-mint.sh`: starts an anvil chain (chain id 40204) and an anvil fork of it,
  fetches the pinned Solidity dependencies, and with `--with-bundle-tools` fetches Aderyn and
  Medusa from the URLs in `components/toolchain-bundle.json` and checks each archive's SHA-256
  against the measured value. It runs the tests under a PATH that holds only the tools this
  machine provides, so "aderyn and medusa are not installed" is a property of the run.
- `deploy_gate.rs`: a failing Forge test and a failed Medusa property are now named in the
  item's reason (up to three, each bounded, then "and N more"), so the verdict card and the
  refusal cite the finding and not only a count (US-6.1 AC2, US-6.2).
- `contract_deploy_sync` takes the managed states' inner values (no behavior change), so the
  test drives the same body as the command. `postdeploy::forge_standard_json` and `pin_site`
  are `pub(crate)` for the same reason.
- Real Aderyn 0.6.8 and Medusa 1.5.1 captures replace nothing but sit beside the hand-written
  fixtures (`*.real.*`), with gate tests on them.

## The verifier configs

"The verifier config of this machine" finds each tool on PATH. On this Mac aderyn and medusa
are not installed, so the gate is NOT READY and `contract_deploy` refuses. That is the state a
member is in today: the component updater that would ship them has an empty key slot.

"The test-only verifier config" adds the two binaries from the measured bundle archives. It
exists only in a `#[cfg(test)]` file, is set only by the script, and changes where two binaries
are found, never how their output is judged. It never ships.

## Run on 2026-10-04 (macOS arm64, forge/anvil 1.5.1, slither 0.11.6, aderyn 0.6.8, medusa 1.5.1)

```
scripts/e2e-hello-mint.sh --work <dir> --deps-cache <dir> --with-bundle-tools <dir> --with-page-build
```

| Scenario | Result |
|---|---|
| Without aderyn and medusa | NOT READY; failing items exactly Aderyn and Medusa campaign, each "not installed"; `contract_deploy` refused naming both; no ceremony opened |
| READY path | Forge 16 passed; Slither 0 High (SARIF); Aderyn 0 High, 7 Low; Medusa 14 passed, 87,312 calls (T1 budget 50,000); fork dry run deployed, gas 1,413,265. READY. `contract_deploy` opened a ceremony showing a contract creation whose init-code hash equals the gated hash; the ceremony signed and broadcast; the runtime code equals the compiled artifact; the standard-JSON verifier input was produced; the site switched; a 1-token mint at 5 SALT went through a ceremony (balanceOf 1, ownerOf(1) the member, the contract holds 5 SALT); the Vercel export was written and built (`npm install` + `npm run build`); the page was built against the deployed address and pinned to a throwaway offline IPFS node (CID `bafybeidj2tu2hf2q57vlbdhxsdx5ow7jdnxi26fkgfbeiklrwr3gozy6jq`) |
| Injected unbounded mint (the `SoldOut` check removed) | NOT READY. Forge: `1 failed (LemonDropsTest.test_mint_stops_at_the_cap(): next call did not revert as expected), 15 passed`. `contract_deploy` refused naming the item; no ceremony opened |

`test result: ok. 6 passed; 0 failed` (115 s). TLC rerun the same day: `DeployGate.cfg` no
error (17,438 states, 3,378 distinct); `DeployGate_Reach.cfg` violates NeverSigns as intended.

## Findings from the run

1. **Only the Forge unit test catches the unbounded mint.** Slither, Aderyn (8 Low, 0 High)
   and a full T1 Medusa campaign (about 85,000 calls) all pass the injected bug: a 500-token cap
   needs more than 50 mints in one sequence before `property_supply_never_exceeds_cap` can
   break. The gate is NOT READY because the template ships a cap unit test. Raising the Medusa
   call-sequence length or adding a cheaper cap property is an S6.9 calibration question,
   pending owner sign-off.
2. **The mint ceremony shows "unrecognized" calldata.** The ceremony decoder does not know the
   template's `mint(uint256)`, so the member must give the raw-data acknowledgement to approve a
   mint. The test asserts that approval without it is refused. Decoding calls of a gated
   contract from its ABI would remove that friction (not built here).
3. **Medusa's console log keeps one ANSI-coloured line even with `--no-color`.** The parser
   already strips ANSI codes; the real capture now pins that.
4. **The runtime's `aderyn_scan` writes SARIF while the core gate parses Aderyn JSON** (the A27
   two-parser gap). This run used Aderyn JSON. Joining the runtime envelopes to
   `deploy_gate_submit` with one parser stays with the S6.3 lane.

## Not done

- The Hermes interview turn (the runtime's interview tests cover the at most 5 questions); this
  run starts from the interview's answers.
- The Browser pop-out showing the site on the fork; the faucet (HUP-S6.5, not built: the local
  chain credits the wallet); CitrateScan verification (live explorer, explorer deploy A45);
  the "proposed fix" half of AC2 (a Hermes turn, not the gate).
- g3-e2e proper: a member on a T1 machine in the packaged app approves the deploy ceremony on
  40204 with funded SALT. Agents may not sign or send on 40204.
- Linux x64 and Windows x64: the script runs on Linux once the bundle's linux-x64 archives are
  measured; Windows native needs the S6.0 spike. Steps for the DGX team are on
  CitrateNetwork/citrate-federation#288.
