---
created: 2026-10-01T09:00:00Z
branch: hup/n3-faucet-adr-literacy
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S7
wp: HUP-S7.7
---

# HUP-S7.7: paraconsensus and precompile literacy pack

Planset `2026-09-30-hermes-upskill`: 05_SPRINTS_AND_WPS (S7.7), D-26 (consensus literacy), US-7.5
(precompile literacy), US-9.2 (Hermes understands paraconsensus). Sprint issue:
CitrateNetwork/citrate-federation#283 (lane shared with S6.8).

## What landed

| Item | Where | State |
|---|---|---|
| 4 first-party skills: `citrate-paraconsensus`, `citrate-belnap-aggregate`, `citrate-precompiles`, `citrate-sidecar-consensus` | `src-tauri/skills/` | written; accepted by the real runtime loader; **not bundled into the app yet** |
| 30-question QA set (27 answerable, 3 unanswerable), qa-v1 format, own anchor index | `src/agent/eval/qa-literacy-v1.json`, `.anchors.json`, `eval/QA-literacy-v1.md` | provenance-checked; not yet run against a model |
| Named QA packs: the validator accepts `qa-<pack>-vN`; `eval-qa.mjs` and `qa-anchors.mjs` take `--dataset` | `src/agent/eval/qa.ts`, `qaCliArgs.ts`, `scripts/` | implemented, default unchanged (qa-v1) |
| Skill format + citation test (mirror of the loader rules; live citation check) | `src/agent/skills/literacySkills.test.ts` | green |

## How the skills were sourced

Every claim cites `repo:path` or `repo:path#symbol`, pinned per skill to full commits:
citrate-chain `0aab474b`, citrate-docs `73ea7c56`, citrate-memories `65742bc`, citrate-agent-runtime
`3d75efb` (origin/main), citrate-core `525b9ca` (integration branch). The test re-reads every cited
file at its pin and checks every `#symbol` occurs in it (64 citations across the four skills).

The precompile list comes from the code, not the docs: `PURE_PRECOMPILE_ADDRESSES` and
`execute_pure_at` in `core/execution/src/precompiles/mod.rs`, the REVM bridge in `revm_adapter.rs`,
gas constants from each precompile file, and the address book `contracts/addresses/40204.json`.

### Checked on chain 40204 (read-only)

- The `0x0110` worked example in `citrate-belnap-aggregate` was run on 40204 (block 61,978,
  2026-10-01 08:50 UTC) through a throwaway contract's constructor inside `eth_estimateGas`, which
  reverts with the result so nothing is deployed. Output: values 0.8125, 0.0, 0.125, -0.5625 and
  states True, Both, Neither, True, matching the hand calculation.
- `0x0100` and `0x0101` called from contract code return success with empty data (not bridged).
- `0x0120` with malformed input returns the all-zero word; `0x0107`, `0x0111`, `0x0130` with a
  one-byte input fail and consume the forwarded gas.
- A top-level `eth_call` whose `to` is any precompile, including the standard `0x02`, returns `0x`.

## Code-versus-docs drift found (recorded in the skills, not fixed here)

- `0x0111 ROUTING_INFERENCE`: docs say future work; code marks the forward pass live and the EVM
  bridge includes it (no ZK proof yet).
- `0x0110` output: docs read as value-then-state per dimension; code emits all values, then all
  states. Docs say "weighted mean"; code computes a weighted sum (weights are per participant).
- `0x0110` never emits False and reduces an all-negative dimension to True; the off-chain engine
  classifies relative to the trust-weighted majority and can emit False. Different rules, same values.
- `core/learning/ARCHITECTURE.md` still says the classification function is not implemented;
  `classify_belnap` exists at the pinned commit.

These belong in a citrate-docs / citrate-chain docs pass; the QA questions avoid them.

## Interpretation recorded

The lane brief asked for "the sidecar consensus". No artifact by that name exists. The skill covers
the two sidecars that reach agreement beside the chain: the chain's learning daemon (Belnap
aggregation in process, `LearningDaemon.tla`) and Hermes's keyless agent sidecar (verifiers, risk
tiers, approval quorum, core's ceremony). If something else was meant, the skill is easy to retarget.

## Red-green

1. `qaCliArgs.test.ts`: 3 new tests for `--dataset`, `qaDatasetFiles`, and the result file name: red
   (not exported), then green.
2. `qaLiteracy.test.ts`: red on the missing dataset; with the old `^qa-v\d+$` version rule the file
   fails at load; green after the named-pack rule.
3. `literacySkills.test.ts`: red on the missing skills; green after. Mutation check: renaming a
   cited symbol and a cited file made the live citation test fail with both named; restored.
4. Dataset mutation check: a key point not in its section and a wrong anchor both fail the live
   provenance test by item id; restored.

## Gates (2026-10-01)

| Gate | Result |
|---|---|
| `npx vitest run` (no source repos) | 829 passed, 9 skipped (838); baseline 796 passed, 3 skipped (799) |
| `QA_SOURCES_ROOT=.. npx vitest run` | 837 passed, 1 skipped (838) |
| `npx tsc --noEmit` | clean |
| `node scripts/qa-anchors.mjs --check` (qa-v1) and `--dataset qa-literacy-v1 --check` | both OK |
| runtime loader (`SkillLibrary::load` from citrate-agent-runtime origin/main, scratch binary) | 4 loaded, 0 rejected, 0 shadowed |
| `cargo test --workspace` (core) | see the issue comment; no Rust changed in this WP |

The existing qa-v1 live-provenance test timed out at the 5 s default under a full parallel run (it
passed alone). It now has a 60 s timeout, like the new live tests.

## Not done

- The skills are not bundled: the app does not set `CITRATE_HERMES_SKILLS` and `src-tauri/skills/` is
  not in the Tauri resources (S3.1/S3.2 wiring).
- The QA set has not been run against a model.
- `precompile_call` helpers (S7.2) are not part of this pack; the skill says what they will need.
- The docs drift above is reported, not fixed.

## Journal

The useful surprise was how much of "literacy" turned out to be about the gaps between three
honest sources. The docs page, the code and the live chain each told the truth about a slightly
different thing: the docs about an intended shape, the code about the current algorithm, the chain
about what a contract actually sees today. A model taught from any one of them would answer some
questions confidently and wrongly. So the skills cite code, the questions cite docs only where docs
and code agree, and the one worked example that matters most (`0x0110`) was run on the chain itself.
The lesson for the next literacy pack: decide up front which source is the authority for each kind of
claim, and make a test that re-reads it at a pinned commit, or the pack will rot the first time
either repo moves.
