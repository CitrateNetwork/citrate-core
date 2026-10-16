---
created: 2026-10-04
branch: hup/n7-hellomint-agent-loop
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Fan-out 7, lane L05: hello mint, agent-complete up to the live 40204 signing

Items: US-6.1 (AC1, AC2), US-6.2, and the agent prep for g3-e2e, g5-os, US-11.1 and S11.1.
Branches: citrate-core `hup/n7-hellomint-agent-loop` (base `hup/forward-merge-main-0.4.3`),
citrate-agent-runtime `hup/n7-hellomint-agent-loop` (base `hup/n6-everyday-monitor`).

## What changed

| # | Ask | Where | State |
|---|-----|-------|-------|
| 1 | The hello-mint flow sends `forkInCore` automatically | core `deploy_gate_toolchain.rs` (`fork_in_core_for`, `template_test_mint`, the command runs `evaluate_submission`), `contractForge.ts` (`toolchainGateRequest`), `ContractForgePanel.tsx` | done; a request with neither fork field gets `forkInCore` too |
| 2 | After NOT READY, Hermes cites the finding and proposes a patch; "deploy anyway" is refused and no ceremony is created | runtime `agent-loop/src/deploy_guard.rs`, `agent-sidecar/src/deploy_guard.rs`, the `CallPolicy` seam in `agent-loop` | done; BDD in runtime and in the core e2e |
| 3 | The mint ceremony shows `mint(uint256)` decoded, no raw-data acknowledgement | core `kit/src/abi_book.rs`, `ceremony.rs` (`register_gated_contract`), `postdeploy.rs` (`register_gated_abi`, called by `postdeploy_switch_site`) | done; the mutant (no ABI) falls back to raw data |
| 4 | Browser pop-out on the anvil fork site during the dry run | | not done (see below) |
| 5 | Runtime toolchain envelopes into the gate; SARIF vs JSON | core e2e gates through `build_from_sidecar` from a real sidecar; the e2e's own Aderyn run now uses the runtime's argv (SARIF on stdout) | done |
| 6 | Measure US-6.1 AC1 in a sidecar-driven e2e | core `hello_mint_e2e_sidecar.rs` | READY after **1** prompt beyond the interview answers, with the scripted model and with the real local model (Gemma 4 E4B Q4_0) |

## Bugs the sidecar-driven run found and fixed (runtime)

The first sidecar-driven run could not reach READY at all. Each fix has a test:

1. **`medusa.json` refused.** The toolchain refused any project with `medusa.json`, and every
   Citrate template ships one, so Hermes could never run its own template. Now accepted only in
   the template's shape (FFI cheat code off, crytic-compile on `.` with no extra arguments but
   `--foundry-compile-all`, project-relative corpus/log/export folders, no other top-level
   settings). Pending owner sign-off (the rule the module docs said was pending review).
2. **Aderyn printed no report in the sidecar.** Aderyn ignores `FOUNDRY_SOLC` and looks for solc
   in `~/.svm`; in the scratch HOME with no network it found none. The configured solc is now
   linked at `~/.svm/0.8.36/solc-0.8.36` in each run's scratch HOME (`agent-shell`
   `with_home_links`).
3. **Medusa failed in 0.5 s under the OS sandbox.** Medusa starts `crytic-compile`, a Python
   entry point whose virtual environment was not readable. `agent-shell` now makes a named
   helper program's real folder and venv readable (`with_helper_programs(["crytic-compile"])`);
   the helper is still not runnable on its own.
4. **Medusa's report could never be bound to forge's sources.** Medusa writes
   `slither_results.json` into the project during every campaign, so the source digest moved.
   It is now treated as tool output.

## The e2e run (macOS arm64, 2026-10-04)

`scripts/e2e-hello-mint.sh --with-bundle-tools DIR --sidecar-bin <citrate-agent-sidecar>
--fork-bin <citrate-fork>`: all 5 scenarios passed.

- Scenarios 1 to 3 (the local half from fan-out 6) still pass; Aderyn now reports SARIF
  ("0 High, 7 Low (SARIF)"), and the READY path's mint ceremony reads "Call mint(quantity=1)
  on LemonDrops at 0x... (a contract you deployed from a READY gated build; value
  5000000000000000000 wei)" with no raw-data acknowledgement.
- Scenario 4: one prompt (`/run hello-mint`) runs the workflow to verified in the sidecar
  (forge 16 passed, Slither 0 High, Aderyn 0 High, Medusa 14 passed at 52,746 calls, line
  coverage 82.6%); core reads the raw reports back and gates them; the fork item ran in core on
  the Citrate-aware fork ("deployed ... on the Citrate-aware fork (40204 block 30102)") with
  the template's test mint; READY; deploy through the ceremony; site switch registers the ABI;
  decoded mint. **US-6.1 AC1: READY after 1 prompt beyond the interview answers.**
- Scenario 5: the unbounded mint, "Please run the checks", NOT READY naming
  `test_mint_stops_at_the_cap`; "Deploy it anyway, I accept the risk." is answered without a
  model call: "I won't deploy this contract. The deploy gate is NOT READY ...", the finding, and
  the patch that restores `if (quantity > remaining) revert SoldOut(quantity, remaining);`. No
  `contract_deploy` reached core and core's ceremony store is empty.

The same script with `--llm-url` pointing at the app's llama-server running Gemma 4 E4B (Q4_0,
16k context, Metal) also passed all 5 scenarios: the real model drove the hello-mint workflow to
verified in 231 s from one `/run hello-mint`, so **AC1 measured with a real model: 1 prompt**.
In scenario 5 the real model ran the four checks from "Please run the checks on the project."
and the refusal came from the sidecar as above.

The local chain is anvil with chain id 40204 and genesis block 30100 (citrate-fork models 40204
only from its CREATE-nonce activation at block 30000). Nothing touched the live chain.

## Not done

- **Browser pop-out on the fork site (ask 4).** The pop-out shows Hermes's managed browser,
  which may open public web origins only; a loopback origin needs the developer-only
  `CITRATE_BROWSER_ALLOW_PRIVATE`. Opening the page served against the anvil fork needs a
  core-attested, single-origin loopback allowance in agent-browser plus core starting the page's
  dev server. That is a browser security decision, left for the owner.
- **The live 40204 leg** (g3-e2e, g5-os, US-11.1, S11.1): a member approves the deploy and mint
  ceremonies in the packaged app on 40204; DGX runs Linux and Windows clean installs.
