---
created: 2026-09-30
branch: hup/s3-qa-eval
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# HUP eval: Citrate QA set v1

Work package **HUP-S3.5** (Citrate QA eval set, 150 questions with citation checks), for
**US-3.1** AC2 ("it knows Citrate out of the box"). Planset:
`.agentile/planset/2026-09-30-hermes-upskill/` (05 row S3.5, "Eval provenance", red-team
correction #4: no model-as-judge).

## Files

| File | What it is |
|---|---|
| `src/agent/eval/qa-v1.json` | 150 items: id, question, category, difficulty, key points, citations, `answerable` |
| `src/agent/eval/qa-v1.anchors.json` | for every cited file: git blob id and every heading anchor, at the pinned commit |
| `src/agent/eval/qa.ts` | schema validator, anchor extraction, deterministic scorer, run loop |
| `scripts/qa-anchors.mjs` | rebuilds (`--write`) or verifies (`--check`) the anchor index from local source repos |
| `scripts/eval-qa.mjs` | runs the set against a live OpenAI-compatible endpoint (loopback only by default) |

## The set

Ten categories, 15 items each: chain basics, staking and validators, wallet, node ops, AI and
inference, memory, Agentile and HIC, devtools and contracts, paraconsensus and federated
learning, governance. 15 items (10 %) are unanswerable probes: the right answer is that the
topic is not documented (for example a live SALT price, an APY, a hardware-wallet model list).

Sources are public-tier only, each pinned by full commit in `qa-v1.json`: `citrate-docs` (the
Almanac pages), `citrate-chain` (README), and `agentile-skills` (README). No private audit
material and nothing under `citrate-security`.

A citation is `<source>:<path>#<anchor>`, for example
`citrate-docs:content/chain/genesis.md#reference`. Anchors are GitHub-style heading slugs
(`slugify` in `qa.ts`).

## Scoring (deterministic)

- **Key points.** Each key point is a list of acceptable phrasings, matched as a normalized
  substring (case, code marks and digit-group commas ignored). Coverage is the share matched.
- **Citations.** Every cited ref must exist in the anchor index (file and heading). An answerable
  item needs at least one of its required citations (source and path) cited.
- **Abstention.** An unanswerable item passes only when the answer admits the topic is not
  documented and cites nothing invalid. "Admits" means the refusal markers of `src/agent/eval.ts`
  or a "the documentation does not cover / specify / contain ..." statement
  (`admitsNotDocumented` in `qa.ts`, added 2026-10-01 after the first live run).
- An answerable item passes with coverage at or above the threshold (default 0.6,
  `--coverage-threshold`), a required citation, and no invalid citation.

Scorecard fields: `passRate`, `keyPointCoverage`, `citationHitRate`, `citationValidity`,
`abstentionRate`, `falseAbstentionRate`, `byCategory`, `failures`, `failureReasons`.

## How to run

```sh
node scripts/eval-qa.mjs --base-url http://127.0.0.1:18080/v1 --model <name> --tier T1
```

Writes `eval/results/<YYYY-MM-DD>-qa-<model>.json` (scorecard plus each answer). Requests are
sequential, `temperature: 0`, 180 s timeout. A transport error aborts and writes nothing.
Non-loopback endpoints are refused unless `--allow-remote`. Node 22.18+ (built-in type
stripping).

To re-verify provenance (needs the source repos checked out next to this one, or
`QA_SOURCES_ROOT`):

```sh
node scripts/qa-anchors.mjs --check
QA_SOURCES_ROOT=.. npx vitest run src/agent/eval/qa.test.ts
```

The live part re-derives every blob id and heading from `git show <commit>:<path>` and checks
that each key point appears in the text of the section it cites. Without the repos it is skipped
and says so; the committed index is still checked.

## Scope and limits (v1)

- First run 2026-10-01 on T0 (Gemma 4 E4B): 10 % pass, all of it the unanswerable probes; the model
  abstained on 134 of 135 answerable items because it is given no documentation yet. The T1 run
  is pending (out of memory on the test machine). See `eval/results/README.md`.
- 2026-10-02, through the app's `memory_search` tool over the bundled corpus (digest
  `970966831d9c`): T1 (Qwen3.8 27B) 77.3 % pass with 99.7 % citation validity, T0 (Gemma 4 E4B)
  65.3 % pass with 98.8 %. The AC2 target is still unset; a proposal pending owner sign-off is in
  `eval/results/README.md`.
- Citations name repo paths. The bundled graph from S3.1 must carry these paths and headings so
  Hermes can cite them; until S3.1 lands, a model can only cite them from its own knowledge.
- Key-point matching is substring based, so a paraphrase that drops the exact figure or term
  scores as a miss. Phrasings were chosen to be the terms the docs use.
- Provenance: hand-authored, versioned, and kept disjoint from any E9 training trajectories.
  Never copy it into training data. To change an item, make `qa-v2`; do not edit v1 in place.
