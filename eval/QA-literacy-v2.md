---
created: 2026-10-04
branch: hup/n7-eval-model-runs
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S7.7 (ra-16, US-9.2 AC1)
---

# HUP eval: paraconsensus and precompile literacy set v2

`qa-literacy-v2` is `qa-literacy-v1` ([QA-literacy-v1.md](QA-literacy-v1.md)) plus the items v1
was missing for **US-9.2 AC1**: "Hermes explains FOUR, the knowledge/truth orders, the classifier,
and the aggregation, with citations." v1 stays frozen (its sha256 is pinned in
`src/agent/eval/datasetHashes.test.ts`); v2 starts with the 30 v1 items byte for byte.

## Files

| File | What it is |
|---|---|
| `src/agent/eval/qa-literacy-v2.json` | 41 items: 37 answerable, 4 unanswerable probes |
| `src/agent/eval/qa-literacy-v2.anchors.json` | blob ids and heading anchors at the pinned commits (`scripts/qa-anchors.mjs --dataset qa-literacy-v2 --write`) |
| `src/agent/eval/qaLiteracyV2.test.ts` | v1 kept unchanged, the AC1 coverage map, anchors, live provenance |

## Coverage of US-9.2 AC1

| AC1 topic | Items |
|---|---|
| FOUR (what the four values mean) | `qa-lit-four-both-neither`, `qa-lit-four-true-false`, `qa-lit-mean-collapses` (new) |
| knowledge order, truth order | `qa-lit-knowledge-order`, `qa-lit-truth-order` (v1) |
| the classifier | `qa-lit-classifier-rules`, `qa-lit-trust-weights` (new) |
| the off-chain aggregation | `qa-lit-aggregation-steps`, `qa-lit-dual-output`, `qa-lit-router-contested` (new); `qa-lit-reduction` (v1) |
| the `0x0110` aggregation | `qa-lit-0110-why-integer`, `qa-lit-0110-inputs` (new); `qa-lit-output-size` (v1) |

The new items cite `citrate-docs` @ `73ea7c56` only (the paraconsistent page and the precompile
page), which the bundled knowledge corpus holds, so a retrieval run can find and cite them. They
avoid the spots where the docs and the code differ (the `0x0110` output layout, the weight shape,
gas scaling by `n`, the False state). `belnap_codec` (US-9.2 AC2) is scored by the sidecar
`workflow-v1` eval, where the tool runs for real; it has no public-tier page to cite here.

## How to run

```sh
# the app's in-app tool path
node scripts/eval-qa.mjs --dataset qa-literacy-v2 --base-url http://127.0.0.1:<p>/v1 --model <name> --tier T0 \
  --api-key-env RUNKEY --memory-socket <mem-mcp socket> --corpus-dir <imported corpus>
# the app's path with the sidecar loop on: the real sidecar offers the bundled skills
node scripts/eval-qa.mjs ... --retrieval-mode sidecar --sidecar-bin </abs/citrate-agent-sidecar> \
  --context-tokens <llama-server ctx> --skills </abs/src-tauri/skills> \
  [--skills-lock </abs/skills.lock> --skills-third-party </abs/staged skills-bundle>]
```

## Results so far

Run record: [results/2026-10-04-qa-literacy-v2.run.md](results/2026-10-04-qa-literacy-v2.run.md).
T0 (Gemma 4 E4B) is below the g2-knowledge bar on this set (51.2 % pass through the tool path,
43.9 % through the sidecar); T1 is not scored yet.
