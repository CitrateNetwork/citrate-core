---
created: 2026-10-04
branch: hup/n7-eval-model-runs
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S7.7 (ra-16, US-9.2 AC1), gate g2-knowledge
date: 2026-10-04
hardware: Apple M2 Max, 32 GB unified memory, macOS 15.6.1
server: "Citrate Core.app bundled llama-server, version 0.4.0-dev (build 10909, commit a2878d30d)"
runtime: "citrate-agent-runtime hup/n6-everyday-monitor @ 45fceda (debug build of citrate-agent-sidecar)"
corpus: "knowledge corpus 970966831d9c (32,702 nodes, BGE vectors), imported into a mem-mcp store"
results:
  - 2026-10-04-qa-literacy-v2-tool-gemma-4-E4B-it-Q4_0.json
  - 2026-10-04-qa-literacy-v2-sidecar-gemma-4-E4B-it-Q4_0.json
---

# Run record: qa-literacy-v2 on T0 (2026-10-04)

First runs of `qa-literacy-v2` ([../QA-literacy-v2.md](../QA-literacy-v2.md)) with retrieval over the
bundled corpus and, in the second run, the bundled skills. Each JSON file was written by
`scripts/eval-qa.mjs`; nothing here is hand-edited.

## Setup

- Model server: `llama-server -m gemma-4-E4B-it-Q4_0.gguf --host 127.0.0.1 --ctx-size 8192 -ngl 99
  -np 1 --jinja --reasoning-format deepseek --no-webui --no-slots`, with a per-run API key. Context
  8,192 is what the installed app was serving on this Mac at the time (its own llama-server flags);
  the app's server kept running beside the eval server.
- Memory: a `mem-mcp` daemon whose store imported the full corpus (`970966831d9c`), the same store
  the 2026-10-02 `qa-v1` runs used.
- Tool run: `--memory-socket` in the default tool mode (the model calls `memory_search`, at most 6
  model requests). This run used the tool-mode code that `--retrieval-mode sidecar` later extended;
  the tool path itself is unchanged.
- Sidecar run: `--retrieval-mode sidecar` with the real sidecar, `CITRATE_HERMES_SKILLS` set to
  `src-tauri/skills` (the four first-party skills, including the "Citing the docs" tables added in
  this branch) and the reviewed third-party skills staged from `skills.lock` (verified with
  `scripts/stage-skills-bundle.mjs verify`): 244 skills loaded. No embedding endpoint was set, so
  the sidecar ranked skills with BM25, as the app does.

## Results

| Metric | T0 tool path | T0 through the sidecar, with skills |
|---|---:|---:|
| started (UTC) | 2026-10-04 21:33 | 2026-10-04 22:29 |
| pass | 51.2 % (21/41) | 43.9 % (18/41) |
| key-point coverage | 57.4 % | 55.4 % |
| citation hit | 70.3 % | 59.5 % |
| citation validity | 100.0 % | 97.5 % |
| abstention on the 4 probes | 50.0 % | 75.0 % |
| false abstention | 10.8 % | 8.1 % |
| paraconsensus answerable items passed | 9/23 | 9/23 |
| the 11 new v2 items passed | 5/11 | 6/11 |
| items where the model read a skill | 0 | 6 |

Neither run meets the g2-knowledge bar (75 % pass, 80 % citation hit; the owner set it on T1, and
T0 is reported). US-9.2 AC1 is not met on T0.

## Why items failed

- **Key points phrased differently.** Of the failed answerable items, 7 (tool) and 7 (sidecar)
  missed only key points, often with a correct answer in other words: "True and False are swapped"
  against the key point "swap True and False", "Neither sits below both True and False" against
  "Neither sits below True and False". The scorer matches phrases, not meaning, so these count as
  failures. Widening the accepted phrasings would be a `qa-literacy-v3` change and is left to the
  owner, because it moves the bar.
- **The required page not cited.** Many paraconsensus answers cite Gradient Paper No.2
  (`gradient-papers:markdown/...`) or a tutorial page, which the corpus also holds, instead of
  `content/research/paraconsistent.md`. Those citations are valid but are not the required one.
- **Skills rarely read.** Through the sidecar the model read a skill on 6 of 41 questions; when it
  did, it often answered from the skill without searching and cited the skill rather than a docs
  section. The "Citing the docs" tables were added to the skills for this reason; on this run they
  did not change the outcome.

## Not run, and why

- **T1 (Qwen3.8 27B Q4_0 and the Qwen 3.6 candidates).** Two attempts on this Mac failed for
  machine reasons, not model reasons: first the memory daemon did not answer within 60 s while a
  27B model and other lanes' builds held memory (swap 25 GB of 34 GB); then a stopped T1
  llama-server stayed in uninterruptible exit with about 21 GB of GPU memory still wired, and every
  later model load failed with `kIOGPUCommandBufferCallbackErrorOutOfMemory`. No partial scorecard
  was written (Rule 1). Freeing that memory needs a restart of the Mac.
