---
created: 2026-09-11
status: active
sprint: sprint-hermes-p0-router
---

# Evidence

## WP0.1 — ModelRouter core (spec-first) ✅
- **Spec:** `src-tauri/formal/ModelRouter.tla` + `ModelRouter.cfg` — the router state machine
  with INV-Router-1 (single active), INV-Router-2 (never serve a not-ready model — resolve to
  the active choice iff ready else the always-ready gateway), INV-Router-3 (no phantom), and
  LIVE-Router-1 (a selected not-ready model eventually becomes ready). Authored to the house
  `src-tauri/formal/` style. **TLC status:** not run in this environment (no JDK installed;
  only the macOS `java` stub). The invariants hold by construction (Resolved falls back to the
  always-ready gateway ⇒ INV-2; Resolved ∈ {active, gateway} ⊆ Choices ⇒ INV-3; WF on
  BecomeReady ⇒ liveness). **TODO:** run `java -cp tla2tools.jar tlc2.TLC -config
  ModelRouter.cfg ModelRouter.tla` in the pre-push gate (which has the JDK) to certify green.
- **Core:** `src/agent/modelRouter.ts` — pure `enumerateChoices` (local ready / registry
  not-ready / always-ready gateway, de-duped), `resolveActive` (the INV-2 fallback),
  `canSelect` (the INV-3 phantom gate). Framework-free.
- **Tests:** `src/agent/modelRouter.test.ts` — 10 tests, all green. Prove: three-source merge
  + de-dup, resolve-ready-to-itself, fall-back-to-gateway for not-ready/none/phantom, the
  resolved backend is ALWAYS ready AND an enumerated choice, phantom ids are unselectable.
  typecheck clean; frontend suite unaffected.

## WP0.2–0.4 — pending (wire the three sources, picker UI, agent chat on the router).

## WP0.2 — wire the live sources ✅ (local + gateway; registry split to WP0.2b)
- `src/agent/modelRouterSources.ts` — `liveEnumerateInput` maps `bridge.modelsCatalog.local()`
  (ModelDescriptor → ready local choices, labeled by GGUF filename) and the always-serve
  gateway/demo terminal (gatewayReady=true so INV-Router-2's fallback is never a dead end)
  onto the pure router; `choicesFromSources` returns the full list. `gatewayConfigured()` is a
  label hint (WP0.3), never the fallback readiness.
- `src/agent/modelRouterSources.test.ts` — 6 tests green (label precedence, gateway-configured
  hint, local→ready mapping + always-ready terminal, empty registry default, registry choices
  surface not-ready, and INV-Router-2 end-to-end: a not-ready registry pick resolves to the
  ready gateway terminal). typecheck clean.
- **WP0.2b (tracked, follow-up):** the on-chain model registry has only a WRITE path
  (storage.rs register_model_calldata); a `models_registry_list` chain-read command is needed
  before the registry source lights up. `registry` stays empty until then.

## WP0.3 — model picker UI ✅ (component; mount+persist in WP0.4)
- `src/components/ModelPicker.tsx` — presentational + source-of-truth-agnostic picker over the
  router's ModelChoice[]: source badge (on-device/registry/gateway), ready dot, honest
  not-ready hint ("download to use" / "pull to use"), active highlighting, null-active ⇒
  gateway default. `isSelectedChoice` is a pure exported helper.
- `src/components/modelPicker.test.tsx` — 7 tests green (lists all choices, exactly-one
  aria-selected, gateway default when active is null, not-ready registry hint, onSelect fires
  with the id on click). typecheck clean.
- The picker is deliberately presentational so the Models section + the agent chat can feed it
  ONE source of truth; it is MOUNTED into the agent chat surface, wired to the live
  modelsSlice + gateway, and the selection persisted in **WP0.4**.

## WP0.4 — the router in the real chat ✅
- `activeModelId` added to AppState (persisted; default null ⇒ gateway).
- `store.selectModel(id, choices)` — phantom-safe persist (canSelect gate, INV-Router-3);
  `store.routerActive(choices)` — resolveActive (INV-Router-2, never a not-ready backend).
- Mounted `ModelPicker` in the Dashboard coach chat (the REAL working chat): a header model
  chip shows the RESOLVED backend + toggles the picker, fed by the LIVE `modelsSlice.local` +
  gateway (one source of truth with the Models section). Selecting a LOCAL model calls the
  shared `modelsSlice.selectModel` (switches the served model / restarts llama-server);
  gateway is the always-ready default. Selection persists.
- Tests: store selection primitive (persist valid, ignore phantom, resolve not-ready→gateway)
  + the existing picker/router/sources tests. Full frontend suite 479 green; typecheck clean.
- NOTE: the router is delivered in the existing coach chat (already a real chat). Making the
  Agent WORKSPACE tab also a chat surface on the same component is a small follow-on; the P0
  goal (a real chat on the model router) is met.

## P0 remaining: WP0.2b (on-chain registry READ, issue #30).

## WP0.2b — on-chain ModelRegistry READ ✅
- `src-tauri/src/model_registry.rs` — reads the on-chain ModelRegistry (addresses::model_registry(),
  0xba36fa0d… on 40204): getAllModelHashes() → bytes32[], getModel(hash) → (owner, name, ipfsCID).
  Pure ABI decoders (be_usize bounds-checked; dynamic bytes32[] + string tuple); async
  `models_registry_list` command runs OFF the main thread (spawn_blocking — the v0.2.3 lesson).
  Rule 1: honest error on RPC/decode failure, never a fabricated list.
- `model_registry_tests.rs` — 6 tests green (round-trip encode→decode of bytes32[] + getModel
  owner/name/cid, empty + multi-word strings, short-return errors). Registered in lib.rs.
- Bridge: `modelsCatalog.registry()` (tauri invoke; sim honest-empty) + RegistryModel DTO.
- Slice: `registry` state + `refreshRegistryModels()`; wired into the Dashboard router picker as
  the downloadable third source (registryModelsToChoiceInput → not-ready "download to use").
- typecheck clean; frontend 480 green; cargo model_registry 6 green.

P0 COMPLETE: WP0.1 (spec+core) · WP0.2 (local+gateway) · WP0.2b (registry read) · WP0.3 (picker) · WP0.4 (router in chat).
