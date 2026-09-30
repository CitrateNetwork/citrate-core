---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core
companions: 02_ARCHITECTURE.md, gates.yaml
---

# Formal Specs Plan

New modules go in `src-tauri/formal/` next to the existing ones (`ModelRouter`,
`MemoryPack`, `ConsentGate`, `SidecarSupervisor`). Each is TLC-green before its WP
closes, with the run cited in gates.yaml.

| Module | Models | Key invariants / properties | WP |
|---|---|---|---|
| `AgentLoop.tla` | Planner/executor with retries, verifiers, stop | `Bounded`: steps ≤ MaxSteps. `TaintDowngrade`: untrusted content in context ⇒ no HIC-2/3 auto-approval for the rest of the task. `OnlyVerifierSucceeds`: outcome=success ⇒ all step verifiers passed. `StopIsLive`: ◇(stopRequested ⇒ halted within one tool deadline). `NoEffectWithoutGate` | HUP-S1.3 |
| `FolderGrant.tla` | Grants, path canonicalization, symlinks, TTL | `NoParentEscape`, `SecretsNeverReadable`, `ExpiredGrantInert`, `ReadNotImpliesWrite` | HUP-S2.1 |
| `WebSigningBudget.tla` | SIWE requests across origins, budgets, revoke | `OnlySiwe` (never a tx/permit), `OriginBound` (domain = top-frame origin), `TopFrameOnly`, `NonceUnique`, `NoCapabilityDelegation`, `BudgetMonotone`, `RevokeImmediate` | HUP-S2.3 |
| `SkillPersistence.tla` | Proposal → verifier → user → store → publish | `PersistImpliesVerifiedAndAccepted`, `PublishImpliesHIC1` | HUP-S3.4 |
| `DeployGate.tla` | Artifact hash, gate results, ceremony | `DeployImpliesGateGreenForSameHash` (hash = initcode ‖ ctor args ‖ compiler settings), `NoTOCTOU` (bytecode can't change between gate and sign) | HUP-S6.4 |
| `SpendBudget.tla` | Escalation + x402 + gas + faucet within a session | `SpendWithinCap`, `EgressOptInOnly` | HUP-S1.5 |
| `AnchorBatch.tla` | Nightly batch of decisions/memory/benchmarks | `EveryDecisionEventuallyAnchoredOrReported`, `NoDoubleAnchor` | HUP-S7.3 |
| `DeviceLink.tla` | Device keys, link attestation, roster admission | `DistinctPeerIds`, `RevokedDeviceEvicted`, reuses `ClusterAdmission` | HUP-S8.1 |
| FL round | **Reuse** `settlement-core` Round + `GradientAggregation*` + chain `ParaconsistentAggregation` | Extend with cluster membership = roster devices | HUP-S9.2 |

The existing `ConsentGate.tla` is extended rather than duplicated where the new gates
refine it (Rule 9).
