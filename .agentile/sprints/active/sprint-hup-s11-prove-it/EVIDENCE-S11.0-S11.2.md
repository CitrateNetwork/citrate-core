---
created: 2026-10-01
branch: hup/n3-release-gates
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# HUP-S11.0 + S11.2 evidence: release gates that cost no CI minutes

Planset `.agentile/planset/2026-09-30-hermes-upskill/` rows S11.0 (installer size budget, gate
`g5-size`) and S11.2 (eval suite in CI + scorecard). Sprint issue: federation #288.

## S11.0 installer size budget

- `scripts/size-budget.mjs`: measures installer/updater artifacts and every app component in a
  Tauri bundle dir, compares with `release/budgets.json`, prints a table, writes optional
  markdown/JSON, exits 0 / 1 (over) / 2 (usage). Standard library only.
- `release/budgets.json`: macOS aarch64 DMG 415,000,000 B and updater 385,000,000 B (shipped
  v0.4.2: 394,782,331 / 366,205,330, about +5%); 17 component rows at local-build baseline +10%;
  Linux and Windows rows tracked with `null` (no release to baseline from yet).
- `release/README.md`: what is measured, budgets and their basis, how to run, where it runs.
- `docs/RELEASE.md`: step 5 of "Cutting a release" runs the gate locally.
- Real run 2026-10-01 against `target/release/bundle` (local v0.4.2 build): PASS, 0 over,
  0 unbudgeted; DMG 87.5%, updater 94.5% of budget.

## S11.2 eval in CI + scorecard

- `.github/workflows/eval.yml`: `workflow_dispatch` only. Inputs `model`, `base_url` (blank =
  secret `EVAL_BASE_URL`), `tier`, `suites`; optional secret `EVAL_API_KEY` by env var name.
  Runs `eval-tools.mjs` and `eval-qa.mjs` with `--allow-remote`, renders `SCORECARD.md`, writes
  the job summary, uploads `eval-scorecard-<run id>`. Read-only token, SHA-pinned actions, inputs
  reach shell only through env.
- `scripts/eval-scorecard.mjs`: JSON scorecards to one markdown scorecard (tool-call table with
  the g1 bar per tier, QA table, failures with reasons, what is not measured). Refuses an empty
  directory (exit 2, nothing written).
- `eval/results/SCORECARD.md`: generated from the two committed real runs (Gemma 4 E4B T0,
  Qwen3.8 27B T1). A test fails if it drifts from the JSON.
- `eval/README.md`: scorecard and manual-CI sections.

## Proof

| Gate | Before | After |
|---|---|---|
| `npx vitest run` | 88 files, 796 passed, 3 skipped | 91 files, 835 passed, 3 skipped |
| `npx tsc --noEmit` | clean | clean |
| actionlint | not installed | not installed; YAML parsed by PyYAML 6.0.3 in the test, plus JS structural checks |

Red first: all three new test files failed on missing modules / missing `eval.yml` before the
code existed. Mutation checks (each mutant made the suite fail, then was reverted):
`>` to `>=` on the budget compare; `lstat` to `stat` (follow symlinks); dup counting removed;
`--strict` ignored; Linux product-dir match removed (needed a new fixture dir to bite); g1 bar
`>=` to `>`; pipe escaping removed; a `push` trigger added; an input spliced into `run:`; an
unpinned action; a broken YAML line.

No Rust was changed, so no cargo gate applies to this branch.

## Not done

- Not wired into `release.yml`; no workflow runs the size gate automatically (owner call).
- The eval workflow has not been dispatched: it needs a reachable endpoint and the owner's
  minutes. No new scorecard was produced tonight.
- Linux/Windows size budgets are `null` until the first measured build on those OSes.

## Journal

The useful surprise was in the llama dylibs. The table showed `resources/llama` at 59.6 MB with
every library present three times under its version names, all regular files of the same size.
The dup column, which hashes content, still said zero: each copy is signed on its own, so the
bytes differ. A size gate that only reports totals would never have surfaced it, and a dedup
check that trusts sizes would have overclaimed. The README records it as an observation with
the check it needs (install names) rather than a fix.

The other lesson is about cost. The owner asked for CI that does not waste minutes, and the
cheapest CI is a test that pins the trigger list. `eval-workflow.test.mjs` fails the moment
anyone adds `push`, `pull_request` or `schedule` to the eval workflow, and it runs inside the
existing vitest job, so the guard itself costs nothing extra.
