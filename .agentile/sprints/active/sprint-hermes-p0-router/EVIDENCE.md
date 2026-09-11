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
