---
created: 2026-10-01
branch: hup/n3-tier-drives-serve
author: Larry Klosowski + Claude Opus 5.5
status: review
---

# HUP-S1.6 (rest): the tier drives the served model context

Closes the gap the 2026-09-30 retro recorded: "onboarding only; does not drive serve.rs". The
effective tier (probe plus the stored override) now sets what `llama-server` starts with.

## What changed

- `src-tauri/src/serve_plan.rs` (new). A pure `plan_serve(tier, facts, model_bytes, gguf)` and a
  bounded GGUF header reader (`read_gguf_facts`: v2/v3 only, 64 MiB header budget, per-field caps,
  metadata only).
  - Target context = the tier's (T0 16k, T1 32k, T2 64k), capped by `<arch>.context_length`.
  - Steps down by halves to 8192 until `model bytes + 1.5 GiB + KV(ctx)` fits the budget.
  - KV per token is an upper bound: every layer full attention, f16 K and V.
  - Budget: Apple Silicon = usable memory (total minus the 7 GiB node reserve), also bounded by
    an approximation of the macOS GPU working-set limit (2/3 of RAM up to 36 GiB, 3/4 above).
    A probed NVIDIA GPU = VRAM, used only if the model plus an 8k context fits in it. Everything
    else = usable RAM.
  - `-ngl 99` on Apple Silicon and on a probed GPU that fits the model. Everywhere else no flag
    (the existing behaviour).
  - Fallback: 8192 (or less if the model was trained for less), `fits = false` and a plain note,
    when memory, model size or header cannot be read, or nothing fits.
- `serve.rs`: the manager holds a `ServePlan`. `--ctx-size` and `-ngl` come from it.
  `start_with_plan` / `select_model_planned` apply a plan only after every gate passes, so a
  refused start keeps the previous plan. `ServeStatus` reports `ctxTokens`, `gpuLayers` and
  `planNotes`.
- `model_serve_start` and `model_catalog_select` compute the plan off the main thread for the
  model being started.
- `hermes.rs`: the session body sends the running server's planned context (`contextTokens`). It
  also sends `maxTokens` = min(2048, ctx / 4), the same cap as direct chat.
- Onboarding tier panel says the context is a target, set at the next model start and capped by
  the model file and free memory.

The bundled model is unchanged.

## Proof

- Red first: 55 compile errors against the empty module, then green.
- Mutation checks, each killed by a test:
  - the memory fit check
  - the trained-context cap
  - the `-ngl` push
  - the GPU working-set cap
  - the reply budget
- Manual proof on a 32 GB Apple Silicon Mac against the real files (ignored test
  `manual_proof_reads_a_real_gguf_header`, `CITRATE_GGUF_PROOF=<path>`):
  - Gemma 4 E4B Q4_0: `gemma4`, n_ctx_train 131072, 168 KiB/token. T0 16384, T1 32768,
    T2 65536, all `-ngl 99`.
  - Qwen 3.8 27B Q4_0 (16 GB file): `qwen35`, n_ctx_train 262144, 260 KiB/token upper bound.
    16384 on every tier, with a "reduced to fit" note.
- Gates:
  - `cargo test --workspace` (src-tauri): 555 + 215 passed, 7 ignored.
  - clippy 1.98.1 `-D warnings` clean.
  - `tsc` clean.
  - vitest: 797 passed, 3 skipped.

## Not done / honest limits

- Hybrid and sliding-window models are over-costed. Qwen 3.x only has full attention every 4th
  layer, and Gemma 4 has sliding-window and shared-KV layers. So their context is smaller than
  the hardware could hold. A per-architecture KV model is a follow-up.
- The macOS GPU limit is approximated, not read. citrate-sizeup reads `iogpu.wired_limit_mb`, but
  that needs the Rule-12 drift entry first.
- A tier override does not restart a running server. It applies at the next start or model
  switch, and the UI says so.
- Linux and Windows `-ngl` paths are unit-tested only. The bundled Linux and Windows llama-server
  builds may be CPU-only, and then `-ngl` has no effect.

## Journal

The tier table looked like a lookup, but the real constraint was memory beside the node. The
override lets a member pick T2 on a 16 GB Mac. Planning against the tier's own usable-memory
math, rather than trusting the tier's label, is what keeps that choice safe: the plan steps down
and says why. Reading the GGUF header turned the "cap conservatively" fallback into a real
number for every model the app offers today.

## Review (2026-10-01, adversarial reviewer)

Two fixes on this branch, each red first:

- The context walk now always tries the 8192 floor. Before, a model trained for a length off the
  power-of-two ladder (Qwen3-style 40960) halved 40960, 20480, 10240 and then stopped, so a
  machine where 8192 fits got the "not enough free memory" fallback note. Test:
  `a_trained_context_off_the_halving_ladder_still_tries_the_8192_floor` (one case plus a
  completeness grid: whenever the floor fits, the plan fits).
- A plan that cannot be computed (the app data dir or the stored tier setting cannot be read)
  no longer blocks `model_serve_start` or `model_catalog_select`. The start proceeds on the
  pre-plan default (8192, no `-ngl`) with a note, which is the behaviour before this WP. Test:
  `a_plan_that_cannot_be_computed_never_blocks_the_model_start`.

Mutation checks rerun by the reviewer, each killed: the `-ngl` push, the trained-context cap, the
memory fit check, the floor step, the reply budget, and applying the plan at start.
