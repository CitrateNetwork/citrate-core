---
created: 2026-10-01
branch: hup/n3-faucet-adr-literacy
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S7.7
---

# HUP eval: paraconsensus and precompile literacy set v1

Work package **HUP-S7.7** (paraconsensus + precompile literacy pack), for **US-7.5** (precompile
literacy), **US-9.2** (Hermes understands paraconsensus) and planset decision **D-26**. It uses the
same format, validator and deterministic scorer as the Citrate QA set v1 ([QA-v1.md](QA-v1.md));
read that file for the scoring rules.

## Files

| File | What it is |
|---|---|
| `src/agent/eval/qa-literacy-v1.json` | 30 items: 27 answerable, 3 unanswerable probes |
| `src/agent/eval/qa-literacy-v1.anchors.json` | blob id and heading anchors of every cited file at the pinned commits |
| `src/agent/eval/qaLiteracy.test.ts` | shape, disjointness from qa-v1, anchor resolution, and live provenance |
| `src-tauri/skills/` | the four skills that teach this material (see its README) |

## The set

| Category | Items | Topics |
|---|---|---|
| `paraconsensus-fl` | 14 (1 unanswerable) | Belnap orders, join/meet/negation, lattice laws, reduction, dual outputs, Q16, `0x0110` output, the safety invariant |
| `devtools-contracts` | 10 (2 unanswerable) | precompile pages and ranges, how to call, x402 return values and gas, tensor format |
| `ai-inference` | 4 | Q16 compute ops, hosted inference receipts, attestation gate, verification precompiles |
| `agentile-hic` | 2 | the agent runtime's approval quorum by risk tier, the Auditor role |

Sources, public tier only, pinned by full commit: `citrate-docs` @ `73ea7c56` (the precompile,
ZK-precompile, paraconsistent and agent-runtime pages) and `citrate-chain` @ `0aab474b`
(`core/learning/ARCHITECTURE.md`). The ids start `qa-lit-` and none is shared with qa-v1. Some
topics overlap qa-v1 (the Belnap values, the `0x0110` gas); the questions do not.

The skills teach from code, and in a few places the code is ahead of the docs (for example
`0x0111` is live in code and called future work in the docs; the `0x0110` output is all values
then all states). The questions avoid those spots so that a correct answer from either the code or
the docs scores the same.

## How to run

```sh
node scripts/eval-qa.mjs --dataset qa-literacy-v1 --base-url http://127.0.0.1:18080/v1 --model <name> --tier T1
node scripts/qa-anchors.mjs --dataset qa-literacy-v1 --check   # provenance, needs the source repos
QA_SOURCES_ROOT=.. npx vitest run src/agent/eval/qaLiteracy.test.ts
```

Results go to `eval/results/<date>-qa-literacy-v1-<model>.json`.

## Scope and limits (v1)

- First run 2026-10-01 on T0 (Gemma 4 E4B): 10 % pass (the 3 unanswerable probes); all 30
  answers said the topic is not documented, as expected while the skills are not bundled. See
  `eval/results/README.md`.
- The skills are not bundled into the app yet, so today a model answers from its own knowledge.
  The first useful run is after the S3.1/S3.2 wiring.
- Provenance: hand-authored and kept disjoint from any E9 training trajectories. To change an
  item, make `qa-literacy-v2`; do not edit v1 in place.
