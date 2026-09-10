---
created: 2026-07-16
branch: feat/core-ai-inference
author: Claude Opus 4.8, directed by @SaulBuilds
status: built — AI1 (real OpenAI-compatible inference, Rust-custodied key) — gates green
program: CORE finish-list item 2 — real AI inference (swap the demo provider for a real one)
rule8: yes — provider API-key custody + secret network egress (an exfiltration surface)
---

# CORE — real AI inference (AI1) — evidence

Built exactly to `AI1_SCOPE.md`, mirroring `custody.rs` (keyring seam) + `oidc.rs`
(ureq HTTPS seam) + `staking.rs`/`earnings.rs` (module + injected-mock discipline).
Chat is now REAL against any OpenAI-compatible endpoint (OpenAI, the Citrate gateway,
any generic `/v1`) with the provider key custodied in Rust. No provider configured →
the honest built-in demo agent (Rule 1), never a fabricated "gateway" reply.

## What shipped (explicit paths)
- `src-tauri/src/ai.rs` (new, @rule8) — `AiManager` over two injectable seams: an
  `AiKeyring` (keyring v3, service `ai.citrate.core`, one JSON blob `{baseURL,model,
  apiKey}` per `ai-<id>` account) and an `AiHttpClient` (ureq/rustls POST). Commands:
  `ai_set_provider` (https-validated, seals the blob, void), `ai_provider_status`
  (metadata only), `ai_clear_provider`, `ai_chat` (reads the STORED baseURL, POSTs
  `/chat/completions` with `Bearer {apiKey}`, parses `choices[0].message.content`).
  `AGENT_SYSTEM_PROMPT_REAL` is a tool-LESS system prompt (no memory/write claims);
  live context is injected as a separate system line. Coarse secret-free errors;
  `ProviderConfig` has a redacting `Debug` + `Drop` zeroize; zero prod `.unwrap()`.
- `src-tauri/src/ai_tests.rs` (new) — 12 tests over an in-memory mock keyring + a
  shared (`Rc`) mock HTTP client (no real keyring, no socket). Covers https validation
  (reject http/ws/file/garbage), bad key/id rejection, sealed round-trip, trailing-slash
  trim, `provider_status` never leaks the key, clear, the request-body shape
  (endpoint + Bearer + system/context/user messages), completion parse, provider-error
  coarseness, unconfigured fail-closed, and the CRITICAL exfil-binding negative controls.
- `src-tauri/src/lib.rs` — `mod ai;`, `app.manage(ai::build_ai_state())`, 4 commands
  registered.
- `src/bridge/domains.ts` — `ChatDomain` extended with `setProvider/providerStatus/
  clearProvider/infer` + `AiProviderStatus` type.
- `src/bridge/tauri/index.ts` — real invokes (`ai_set_provider` maps camelCase→snake).
- `src/bridge/sim/index.ts` — honest: `providerStatus()`→[], `setProvider/clearProvider/
  infer`→Unavailable (no keyring in web preview), `backend`→"built-in demo agent".
- `src/agent/harness.ts` — demo `label` fixed `infer.citrate.ai · local-proxy`→
  "built-in demo agent" (Rule 1); new `createRealProvider(providerId, getContext,
  infer)` reveals the REAL completion via `onToken` (no tool loop this WP).
- `src/shell/store.ts` — `pickChatProviderKind()` (pure selection rule) + `rebuildProvider()`
  (real iff tauri AND default configured, else demo) + `aiSetProvider/aiClearProvider/
  aiSetDefault`; provider rebuilt on launch + on config change.
- `src/shell/state.ts` — **`aiKeys` REMOVED** from AppState + PERSIST_KEYS + freshState
  + the p3 seed (invariant 2: the key lives only in the keyring).
- `src/surfaces/Settings.tsx` — key input → `store.aiSetProvider` (keyring); presets
  OpenAI / Citrate gateway (cgk_) / generic OpenAI-compatible baseURL; live status via
  `bridge.chat.providerStatus`; TRUE keyring/desktop-origin copy; local model =
  honest "requires the model runtime (WO-3), not yet available."
- Tests: `src/bridge/tauri.test.ts` (+5: set/status/infer/clear + backend-Unavailable,
  key never in any result), `src/bridge/sim.test.ts` (+3: honest unavailable + demo
  label), `src/shell/store.test.ts` (+3: selection rule, demo fallback, no aiKeys),
  `src/agent/harness.test.ts` (new, +3: demo label, real-completion reveal, honest error).

## Security invariants (@rule8 — the review attacks these)
1. **No key egress (I-2).** `ai_provider_status` returns `{id,baseURL,model,configured,
   isDefault}` only; `ai_chat` returns the completion string; `ai_set_provider/clear`
   return void. `ProviderConfig` (holds the key) is never serialized across invoke;
   `provider_status_never_leaks_the_key` + the tauri `providerStatus` test assert no
   `sk-`/`apiKey`/`Authorization` in the payload.
2. **Key at rest in the OS keyring**, never localStorage. `aiKeys` removed; a store
   test guards AppState/PERSIST_KEYS carry no key.
3. **Exfil-binding (THE critical property).** `ai_chat(provider_id, messages_json,
   context_json)` has NO url parameter — the endpoint comes from the STORED config.
   `ai_chat_uses_stored_base_url_never_a_caller_supplied_one` stuffs an attacker URL
   into the id/messages/context and proves egress still goes ONLY to the sealed baseURL;
   `a_provider_id_that_is_not_configured_cannot_borrow_another_providers_key` proves two
   sealed providers never cross keys/URLs.
4. **https-only baseURL** at set-time (`rejects_non_https_base_url`).
5. **Coarse secret-free errors** (`chat_surfaces_coarse_error_without_body_on_provider_failure`).

## Gates (all green)
- `cargo test --no-default-features`: **279 passed** (was 267; +12), 5 ignored.
- `cargo clippy --no-default-features --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean (Cargo.toml untouched — no rustfmt comment rewrite).
- `grep -rn '\.unwrap()' src/ | grep -v unwrap_or` (non-test): 0.
- `npx tsc --noEmit`: clean. `npx vitest run`: **119 passed** (was 105; +14).
- Deps/`cargo audit`: **no dep tree change** (Cargo.lock unchanged — reused keyring/
  ureq/serde_json/url/zeroize). `cargo audit` shows only pre-existing transitive
  Tauri advisories (glib/unic-ucd), none introduced by AI1.

## Seam / mock discipline (for the reviewer)
- Rust tests inject an in-memory `MockKeyring` + a shared `SharedHttp` mock — NO real
  OS keyring, NO live socket in CI (mirrors `earnings_tests`' `MockRpc`). The mock HTTP
  records `(url, bearer, body)` so the exact request shape + the STORED-URL choice are
  asserted.
- The `SharedHttp*` mock uses `unsafe impl Send/Sync` (single-threaded test use only)
  to satisfy the trait bound while holding an `Rc<RefCell<…>>` the test can inspect
  after the manager takes its `Box<dyn AiHttpClient>` handle.

## Deviations / open concerns
- None from spec. Auto-provisioning a gateway `cgk_` key to a member stays a `[cm]`
  follow-up (a member uses their cgk_ key via the generic path today). Local on-device
  inference is the honest WO-3 seam. Tool-calling loop + real SSE streaming are the
  named follow-up WPs (this WP is plain chat + live-context injection).
- Provider `Debug`/`Drop` scrub the transient key copies; the sealed blob's key still
  round-trips through `serde_json::to_string` (zeroized after the keyring write).
