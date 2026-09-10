---
created: 2026-07-16
branch: feat/core-ai-inference
author: Claude Opus 4.8, directed by @SaulBuilds
status: building — AI1 (real OpenAI-compatible inference, Rust-custodied key)
program: CORE finish-list item 2 — real AI inference (swap the demo provider for a real one)
rule8: yes — provider API-key custody + secret network egress (exfiltration surface)
grounded_in:
  - src/agent/harness.ts (ChatProvider contract: send({messages,callbacks})→{role,content}; onToken/onToolCall)
  - src/surfaces/Settings.tsx AI providers (#49 honest stub: key masked+discarded, "not wired for inference")
  - custody.rs/config.rs keyring pattern (keyring::Entry, service "ai.citrate.core"); ureq v3 + rustls (oidc.rs pattern)
  - [[citrate-inference-gateway]]: infer.citrate.ai/v1 = OpenAI-compatible, cgk_-bearer-gated
---

# CORE — real AI inference (finish-list item 2, AI1)

Today chat runs `createDemoProvider` (scripted). Settings BYO-key is honest-stubbed:
the key is masked+discarded, stored nowhere real, never calls a model. This makes
chat REAL via a generic OpenAI-compatible provider whose credential is custodied in
Rust — the exact thing #49's copy promised as "a scheduled build."

## What is finishable now vs blocked (grounded)

- **FINISHABLE (this WP): generic OpenAI-compatible BYO-key provider.** A user
  configures {baseURL, model, apiKey}; the key is sealed in the OS keyring (never
  the webview); a Rust command makes the `/v1/chat/completions` call. Works for
  OpenAI, any OpenAI-compatible endpoint, AND the Citrate gateway (baseURL=
  `https://infer.citrate.ai/v1` + a `cgk_` key).
- **BLOCKED — honest seam:** the Citrate gateway AUTO-provisioning a `cgk_` key to a
  member is a **[cm]** deliverable ("gateway keys aren't wired to the membership
  service yet", Settings.tsx). Until then, a member with a cgk_ key uses it via the
  generic path above; auto-issuance is a follow-up (WO-7/WO-10 adjacent).
- **BLOCKED — [dgx] WO-3:** the local Gemma model (weights URL/checksum/runtime
  contract) does not exist yet → local inference stays a one-line honest state.
- **Follow-up WPs (not this one):** tool-calling loop (memory recall / chain reads)
  and real SSE token streaming. v1 = plain chat + live-context injection + client
  reveal of the real completion.

## Security invariants (@rule8 — the review will attack these)

1. **No key egress to the webview (I-2).** NO `#[tauri::command]` returns the API
   key (or Authorization header). `ai_provider_status` returns only non-secret
   metadata (id, baseURL, model, configured:bool).
2. **Key at rest in the OS keyring**, never localStorage. Remove `aiKeys` from
   AppState + PERSIST_KEYS. The webview holds no key.
3. **Exfiltration-proof binding (THE critical property).** The API key is BOUND to
   its baseURL at set-time. `ai_chat` uses the STORED baseURL for the given
   provider id — it NEVER accepts a webview-supplied URL. A compromised webview can
   pick WHICH configured provider to call, but can never redirect a sealed key to an
   attacker endpoint.
4. **https-only baseURL**, validated at set-time (reject http/other schemes).
5. **Coarse, secret-free errors** — never echo the key, the Authorization header, or
   full request bodies.

## Build

### Rust — new `ai.rs` (@rule8), registered in lib.rs
- Keyring: `keyring::Entry::new("ai.citrate.core", "ai-<providerId>")` holding a JSON
  blob `{baseURL, model, apiKey}` (the whole config, so key+URL are bound). Reuse the
  custody.rs keyring idioms (set_password/get_password/delete_credential, keyring v3).
- `ai_set_provider(provider_id, base_url, model, api_key)` — validate https base_url +
  non-empty key; seal the JSON blob. Returns void. (@rule8; never returns the key.)
- `ai_provider_status()` → `Vec<{ id, baseURL, model, configured:true }>` for the
  configured providers + the default — NO key/secret.
- `ai_clear_provider(provider_id)` — delete the keyring entry.
- `ai_chat(provider_id, messages_json, context_json)` — read the sealed config; build
  the OpenAI `/v1/chat/completions` body: system prompt (AGENT_SYSTEM_PROMPT variant
  WITHOUT tool claims + the injected live context as a system line) ++ the messages;
  `ureq` POST to `{baseURL}/chat/completions` (baseURL already ends `/v1`) with
  `Authorization: Bearer {apiKey}`; parse `choices[0].message.content` → String.
  Non-streaming. Errors coarse + secret-free. (@rule8.)
- Data source named in comments (OpenAI-compatible chat/completions; the user's
  configured endpoint). Zero `.unwrap()` in prod.

### Frontend
- `harness.ts`: fix the demo provider's misleading `label` ("infer.citrate.ai ·
  local-proxy" → "built-in demo agent" — Rule 1, it calls no gateway). Add
  `createRealProvider(providerId, getContext)`: `send()` → `bridge.chat.infer(
  providerId, messages, context)` → completion; reveal via `onToken` (a display
  animation over the REAL content — honest, no tools this WP); a real-provider system
  prompt that does NOT claim tool access.
- `store.ts`: select the provider by `aiDefault` + configured status — real provider
  if that id is configured, else the demo provider (honest fallback). Rebuild the
  provider when the AI config changes.
- `Settings.tsx`: the key input → `bridge.chat.setProvider(...)` (keyring), not
  `aiKeys`. Provider presets: OpenAI (`https://api.openai.com/v1`), Citrate gateway
  (`https://infer.citrate.ai/v1`, cgk_ key), + a generic OpenAI-compatible baseURL.
  Update the copy to the now-TRUE keyring/desktop-origin statement; local model stays
  an honest "requires the model runtime (WO-3), not yet available."
- `state.ts`: remove `aiKeys` from AppState + PERSIST_KEYS (key lives in the keyring).
- `bridge`: extend `ChatDomain` with `setProvider`/`providerStatus`/`clearProvider`/
  `infer`; tauri → invoke; sim → honest Unavailable (web preview has no keyring) +
  demo backend.

## Acceptance
- With a real key configured (e.g. OpenAI or the gateway), the in-app chat returns a
  REAL model completion via a Rust-originated HTTPS call; the key is in the OS keyring
  and never in the webview or localStorage; `ai_provider_status` never leaks it.
- No provider configured → the honest built-in demo agent (clearly labeled), not a
  fabricated "gateway" response. Local model → honest WO-3 seam.
- A webview cannot exfiltrate the key to an arbitrary URL (baseURL is bound in Rust).

## Non-negotiables (citrate-core CLAUDE.md)
Rule 1 (name data sources; no fabrication; honest states), Rule 3 (unaffected — no
signatures here), I-2 (no secret-returning command). Zero prod `.unwrap()`. Test count
monotone. cargo `--no-default-features`. Build-and-stop; NO self-review; NO merge.
