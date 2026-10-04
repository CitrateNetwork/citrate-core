---
created: 2026-10-04
branch: hup/n6-forge-wire
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# HUP-S6 product wiring: templates, toolchain into the gate, tier budgets, contract_view, fork guard

Fan-out 6, lane N5-forge (M4). Branches: citrate-core `hup/n6-forge-wire` and
citrate-agent-runtime `hup/n6-forge-wire`, both from the M2 combined branches. The end-to-end
hello-mint run and its automated e2e test belong to lane L-hellomint; this WP is the production
wiring that run exercises. Nothing here signs, deploys or touches chain 40204.

## What was built

1. **Templates for members (S6.2, US-6.4).** `template_list` and `template_render`
   (`src-tauri/src/template_forge.rs`) call the citrate-templates renderer. The list carries each
   template's parameter form (fields, defaults, bounds, required), the tier in effect and its
   Medusa budget. A render writes only into a folder with an active write grant (checked before
   anything is written) and reports which pinned libraries were copied from the toolchain's
   library cache and which are missing. The templates are bundle resources in every
   `tauri*.conf.json` (`../templates/...`, resolved under `_up_/templates`). The Agent page has a
   "Build a contract" panel (`src/shell/ContractForgePanel.tsx`) with the form.
2. **Toolchain into the gate, one source of truth (S6.3 to S6.4, retro A27).**
   - Core owns a toolchain switch, off by default (`forge_toolchain.rs`,
     `toolchain_settings_get/set`). On, it sets `CITRATE_HERMES_TOOLCHAIN=1`, the search path
     (installed components first, then the per-user and system folders, including slither's own
     environment, which holds the `crytic-compile` medusa needs) and the pinned solc for the
     sidecar at the next Hermes start. Off emits nothing.
   - The runtime's toolchain tools now attach their raw report to every completed run
     (`GateReport`: stdout, the project's source digest before and after the run, forge's built
     bytecode digests, medusa's call budget and lcov). The session takes it out of the result
     before the model sees it and keeps it (`agent-sidecar/src/toolchain_reports.rs`);
     `GET /sessions/:id/toolchain/reports` hands it to core.
   - `deploy_gate_submit_toolchain` (`deploy_gate_toolchain.rs`) reads those reports, binds them
     (same source digest for all four runs, and the artifact's bytecode must be exactly what the
     forge run built), reads the compiler settings from the artifact, and hands the raw outputs
     to `deploy_gate::evaluate`. Missing, unbound, timed-out or not-installed runs become failing
     items (NOT READY), never skipped.
   - **Which parser is canonical.** The deploy gate's parsers in core decide every deploy
     verdict. The runtime's verifiers only move workflow steps along; their verdict is not passed
     to the gate. Core's Aderyn parser now also reads Aderyn's SARIF stdout (what `aderyn_scan`
     runs), so both sides read the same bytes. Both repos test against the same real captured
     outputs (`agent-loop/tests/fixtures/toolchain/captured/`,
     `src-tauri/tests/fixtures/toolchain-captured/`) and agree on every verdict. Removing the
     runtime's own parsers entirely would need a crate shared across the two repos (a Rule 12
     drift entry), so it is listed under "Not done" below.
   - No fork dry-run producer here (lane CH-fork). Without one, the fork item fails.
3. **Tier budgets (S6.9).** `medusa_fuzz` takes its default call budget from the project's
   `citrate-template.lock.json` (the tier the renderer recorded). Independently, the gate holds
   the Medusa item to this machine's tier budget from the bundled `medusa-budgets.json`: the
   campaign must reach the tier's call count, and its line coverage of `src/` (from medusa's own
   lcov) must reach the tier minimum. T0/T1/T2 = 10k/50k/200k calls and 60/75/80% coverage are
   starting values, pending owner sign-off.
4. **US-6.3 AC2: `contract_view`.** A read-only agent tool (effect none, output untrusted) that
   calls a view or pure function through core's `contract_view_call` (`contract_reader::view_call`)
   on 40204 or a loopback fork, with the verified ABI or an `abi_fragment`. A function that would
   write is refused before core is asked anything. Annotation, parity fixture (both repos, new
   hash), the read-only tripwire list, the system prompt line, and eval items in
   `src/agent/eval/toolcall-v2.d/contract.json`.
5. **Fork-mode wallet guard.** The hello-mint page now refuses, in fork mode, to ask the wallet to
   sign a mint unless the wallet's latest block is one the fork mined after it forked, with the
   same hash on the fork RPC (`templates/hello-mint/files/app/src/forkGuard.ts`). Chain 40204
   itself, another fork, or an unreadable wallet is a refusal with the reason shown.

## Medusa calibration (S6.9), real runs

medusa 1.5.1 (macos-arm64 component archive, sha256 matches `components/toolchain-bundle.json`),
`fuzz --no-color --test-limit N --timeout 600|900`, rendered templates at their default
parameters, crytic-compile from slither 0.11.6's environment, one Apple-silicon Mac under load
from other lanes (load average about 15 to 19). "Calls" is the last progress line, so it overshoots
the limit by up to one 3-second window.

| template | budget | calls | fuzz time | wall time | tests | src/ line coverage |
|---|---|---|---|---|---|---|
| erc20 | T0 10k | 22,269 | 3 s | 110 s (first run, cold compile) | 6 pass | 3/3 (100%) |
| erc20 | T0 10k | 16,099 | 3 s | 43 s | 6 pass | 3/3 (100%) |
| erc20 | T1 50k | 68,681 | 9 s | 26 s | 6 pass | 3/3 (100%) |
| erc20 | T2 200k | 223,156 | 36 s | 73 s | 6 pass | 3/3 (100%) |
| erc721 | T0 10k | 29,725 | 13 s | 66 s | 14 pass | 19/23 (82.6%) |
| erc721 | T1 50k | 68,344 | 12 s | 27 s | 14 pass | 19/23 (82.6%) |
| erc721 | T2 200k | 211,431 | 36 s | 49 s | 14 pass | 19/23 (82.6%) |

Reading: on these templates coverage plateaus by 10k calls (erc721 branches 339 at T0, 345 at T1
and T2), and compile time dominates the wall clock. erc721's 82.6% clears T2's 80% minimum by
little; the owner may want the T2 minimum at 80% or lower. These are numbers for a decision,
not a decision.

## Proof runs (real forge, slither, aderyn, medusa)

`scripts/forge-gate-proof.sh` renders erc20 at T0, copies the pinned libraries, runs the four
programs with the toolchain tools' exact argv, and evaluates the raw outputs with the production
bridge and gate (`deploy_gate_toolchain::tests::recorded_proof_run_when_present`). forge 1.5.1,
slither 0.11.6, aderyn 0.6.8, medusa 1.5.1.

| run | flags | verdict | failing items |
|---|---|---|---|
| clean, local dry run | `--anvil --expect READY` | READY | none; medusa 100% of src/ lines, 10,000-call T0 budget met |
| injected selfdestruct, local dry run | `--anvil --inject-selfdestruct` | NOT READY | Slither: 1 High (`0-0-suicidal`); Aderyn: 1 High (`selfdestruct`) |
| clean, no dry run | (none) | NOT READY | Fork dry run: none was produced for this bytecode |

The `--anvil` dry run is a throwaway local anvil with chain id 40204 and an impersonated sender
(no key), not a fork of the live chain; the Citrate-aware fork producer is lane CH-fork's.
These runs drive the tools directly, not through a live sidecar session; the sidecar path is
covered by the runtime's session and route tests with stand-in programs.

## Tests

- core: `cargo test --workspace --locked` 1787 passed, 0 failed, 11 ignored (lib 1397, +42);
  vitest 1872 passed, 10 skipped (+33); `cargo fmt --check`, clippy 1.98.1 `-D warnings`, tsc
  clean. The rendered hello-mint app (with the fork guard) passes `tsc --noEmit` and
  `vite build`.
- runtime: agent-sidecar + citrate-agent-loop 579 passed, 0 failed (+18); clippy 1.98.1 clean.
- Hand mutants, all killed: contract_view's write refusal; the fork guard's fork-point check and
  its hash check; the session keeping the raw report out of the model's view; the before/after
  source binding; the gate bridge's bytecode binding and its coverage minimum.

## Not done

- A live run through the packaged app and a real sidecar session (needs a build of both; the
  hello-mint e2e lane covers the end-to-end run).
- Removing the runtime's own toolchain parsers: they still judge workflow steps. A shared crate
  would make them literally one parser; that is a cross-repo dependency (Rule 12) and an owner
  call.
- The pinned libraries reach a rendered project only from a git checkout cache at the pinned
  commit. Unpacking the bundle's hash-checked library archives into that cache is part of the
  component installer (S6.1), which is still off until the @rule8 key ceremony.
- Core reads the sidecar's report list over the control channel, whose reader stops at 10 MB. A
  project whose four raw reports together pass that (a very large forge test report) is refused
  with a plain error, not judged.
- `coverage_plateau_calls` still has no consumer (medusa has no such stop condition; the
  calibration above shows the plateau but nothing stops on it).
- `gates.yaml` g3-gate is not flipped here: the injected-bug run above is real but not through
  the sidecar; the hello-mint lane's e2e run is the gate evidence.
- Medusa needs `crytic-compile` on its PATH. On this Mac it lives only in slither's pipx
  environment, which the toolchain search path now includes; the slither component (S6.1) must
  ship it beside `slither`.

## Review (2026-10-04, adversarial reviewer)

- Fixed on both branches: `forge_test` now runs `forge test --json --force`. Without `--force`,
  forge skips an unchanged build and leaves `out/` as it is, so an artifact written into `out/`
  by hand (the source digest skips `out/`) was recorded as built by the run and passed the
  bytecode binding. Reproduced with forge 1.5.1: a rewritten `out/A.sol/A.json` survived
  `forge test --json` and was cleared by `--force`. Runtime test
  `forge_test_always_rebuilds_so_out_cannot_be_planted` pins the argv; reverting the flag fails
  it and two existing argv tests. The proof script uses the same argv.
- US-6.4 is wired, not runtime-proven: on a member machine the library cache is empty until the
  S6.1 installer ships, so a rendered template does not compile and cannot reach READY there.
- A Medusa `corpusDirectory` other than `medusa-corpus` is not skipped by the source digest, so
  such a campaign is reported as "ran on different sources" (fails closed; the templates use the
  default).
