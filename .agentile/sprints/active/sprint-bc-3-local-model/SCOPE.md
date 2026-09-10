---
title: "Sprint BC-3 — Local model in-flow (download + verify + llama-server sidecar)"
created: 2026-07-19
branch: sprint/bc-3-local-model
author: Claude (Opus 4.8) for SaulBuilds
status: active
planset: citrate-federation/.agentile/planset/2026-07-19-core-beta-completion/ (BC-3)
---

# Sprint BC-3 — Local model in-flow

Owner decision D-BC-1: slim installer + first-run STREAMED download of the Gemma GGUF
with SHA-256 verify, run via a bundled `llama-server` sidecar under the SidecarSupervisor.
Remote `infer.citrate.ai` gateway is the fallback while downloading / offline. Gemma is
the model (the genesis-embedded `mistral-7b` pin is DEPRECATED — owner confirmed 2026-07-19).

## Grounded facts (verified 2026-07-19, no WO-3 gap for Gemma)
- **Model file:** `gemma-4-E4B-it-Q4_K_M.gguf`, **5,335,289,824 bytes**, SHA-256
  `90ce98129eb3e8cc57e62433d500c97c624b1e3af1fcc85dd3b55ad7e0313e9f` (committed sidecar
  `branding/models/gemma-4-E4B-it-Q4_K_M.gguf.sha256`; also in citrate-native).
- **Source (default, config-seam overridable):** `https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf`.
  A Citrate CDN mirror can override via env (backup mirror noted in the gateway model doc).
  GGUF header magic `0x47475546` ("GGUF") v3 — confirmable at download start.
- **Runtime:** `llama-server` (llama.cpp), serves OpenAI-compatible `/v1/chat/completions`.
  citrate-native already ships llama.cpp; the gateway `local-proxy` fronts a local
  `llama-server`. Standard CLI: `llama-server -m <gguf> --host 127.0.0.1 --port <p> --ctx-size N`.
- **ai.rs already does OpenAI-compatible inference** to a sealed `{baseURL, model, apiKey}`.
  The local model is just a provider whose `baseURL = http://127.0.0.1:<port>/v1` and NO key.
- **Patterns to mirror:** `node.rs` (externalBin resolve + supervised spawn + app-data dir),
  `memory.rs` (mcp_serve sidecar), `supervisor.rs` (SidecarSupervisor, injected Clock/health),
  `ai.rs` (provider routing), `staking.rs`/`rpc.rs` (injectable transport for testability).

## Work packages
- **BC-3.1 — model.rs (download + verify).** `model_status` (not-present / downloading{pct} /
  verifying / ready / error), `model_download` (STREAMED, **resumable** via HTTP Range, progress
  events, writes to app-data `models/gemma-4-E4B-it-Q4_K_M.gguf`, never on argv), `model_verify`
  (SHA-256 == the pinned hash; reject on mismatch, refuse to launch). Size + hash + URL are pinned
  consts (URL overridable via a config seam). Injectable HTTP transport so tests use a small fixture
  (verify checksum logic + resume + tamper-reject) — the real 5 GB pull is a documented manual proof.
- **BC-3.2 — llama-server sidecar + harness routing.** Bundle `llama-server` as an externalBin
  (resolve like `node.rs::resolve_node_bin`; binary gitignored, CI cross-build = S7/WO-2). Spawn it
  under the SidecarSupervisor with `-m <model path> --host 127.0.0.1 --port <p>`, health-checked.
  A `model_serve_start/stop/status` command. ai.rs harness picks the LOCAL provider
  (`http://127.0.0.1:<p>/v1`, no key) when the model is present AND the server is healthy; else the
  `infer.citrate.ai` gateway (if a cgk_ key is configured); else the demo provider. Honest states
  surfaced: `ready` (local) / `local-fallback` / `downloading` / `gateway-only` / `no-model`.
- **BC-3.3 — onboarding S6.5 model step.** After S6 node ignition, an honest "Download local model
  (4.96 GB)" step with a real progress bar + verify; SKIPPABLE (chat then uses gateway/demo honestly).
  Store poll like `startNode`; captions name the real source + checksum.

## Testability / red-green
- Rust red tests FIRST: checksum verify (fixture matches pinned → ready; 1 byte flipped → reject),
  resume (partial file + Range continues, not restarts), GGUF magic check, bad/short download → error;
  supervisor spawn via the existing stub pattern (no real llama-server needed). vitest: onboarding
  S6.5 shows real `model_status` progress, skip path is honest, no fabricated "verified".
- Rule 1: no fabricated download/verify; `downloading`/`ready` reflect real bytes+hash. Rule 2 monotone.
- Honest gaps (documented, not faked): the real 5 GB download + real llama-server inference are a
  manual/integration proof; the `llama-server` platform binaries are a WO-2/S7 packaging deliverable
  (like the node binary) — the download/verify/spawn/route LOGIC is built + unit-tested now.
