---
created: 2026-08-27T00:00:00Z
branch: cx/s6.<wp>-<slug> (one branch per WP; Lane D — parallel, deploy-gated)
author: Larry Klosowski (@SaulBuilds) + Claude Opus 4.8
status: active
sprint: CX-S6
planset: Commons / citrate-core-social (citrate-federation/.agentile/planset/2026-08-26-citrate-core-social/)
tier: T1 (@rule8 — keyless agent; every chain effect via the SignatureCeremony)
lane: D (owns .agentile/cx-ownership.map lane s6 — hermes.rs, agent_tools.rs, hermes_tests.rs
       + bridge/*/agent.ts + slices/agent.ts + surfaces/Agent.tsx)
---

# Sprint CX-S6 — Hermes agent harness (Lane D, parallel · deploy-gated) · "Commons"

## Goal

A keyless agent harness (Hermes) that handles skills / code / comms as a real, RBAC-guarded member,
with EVERY chain effect routed through the SignatureCeremony (Rule 3 / D-18) — the harness holds no
key and signs nothing. Distinct from the legacy `agent` module (node-agent GPU market). Deploy is
gated on gD-security sign-off (@rule8). Every WP passes `scripts/cx-ownership-check.sh s6`.

## Baseline (Rule 2 — must not decrease)
- src-tauri lib tests: 306 (post CX-S3.1). CX-S6 adds tests per WP.

## Work packages (serial within the lane)
- [x] **S6.1** `HermesManager` sidecar in `hermes.rs`: resolve the bundled `hermes` binary → spawn
      ENV-configured (control bind + bearer-token FILE PATH, never the token in argv/env) → loopback
      `GET /health` liveness → bounded-backoff restart, mirroring serve.rs/comms.rs + agent.rs's
      bearer scheme (256-bit OsRng token, 0600 file the child adopts). Stub-binary tests (no real
      hermes in CI). +5 tests. **L.** — lib 306 → 311. (`#![allow(dead_code)]` one WP; consumer is
      S6.2.)
- [ ] **S6.2** enroll/setup-driving seam over the bearer control transport; lazy-start singleton
      instantiates the S6.1 manager; wire `hermes_start`/`hermes_status`. Source: `agent.rs`. **M.**
- [ ] **S6.3** chain-effect bridge → unsigned `SignatureIntent` → pending ceremony (identical to
      node-agent). Source: `ceremony.rs`, `agent.rs`. `hermes.rs`, `hermes_tests.rs`. **M.**
- [ ] **S6.4** skills/code (WASM capsules + agent-code) behind the mandatory HITL ApprovalFlow.
      `hermes.rs`, `agent_tools.rs`(new). **@rule8 deploy-gated. L.**
- [ ] **S6.5** comms-as-a-tool (agent as an RBAC-guarded member) — `bridge/*/agent.ts`,
      `slices/agent.ts`, `surfaces/Agent.tsx`. Dep: S6.3 + **S3.3 (blocked on Lane C)**. **M.**

## Daily
- 2026-08-27 — S6.1: HermesManager sidecar lifecycle + bearer (0600 token file) + 5 stub tests green,
  lib 306→311, no new warnings, ownership s6 green.
