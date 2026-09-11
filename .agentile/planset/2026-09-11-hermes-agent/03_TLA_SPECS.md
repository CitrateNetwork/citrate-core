---
created: 2026-09-11
author: Claude Opus 4.8, directed by @SaulBuilds + Luke
status: active
planset: 2026-09-11-hermes-agent
---

# TLA+ Specs — build where none exist

Owner directive: build TLA+ specs where they don't exist. Below are the specs this planset
requires, each with the invariants/properties to model-check. They are **skeletons +
property statements** here; the FULL `.tla` + `.cfg` are authored as the FIRST WP of the
owning sprint (spec-first, then code), living in `spec/hermes/` and TLC-checked in the
pre-push local-CI gate.

## `ModelRouter.tla` (P0, WP0.1)
State: a set of `ModelChoice`s from three sources; one `active` selection; per-choice
`ready` flag.
- **INV-Router-1 (single active):** at most one `active` choice at any time.
- **INV-Router-2 (ready-to-serve):** the send path only resolves an `active` whose `ready`
  is TRUE (or the gateway, always ready) — never a not-ready local/registry model.
- **INV-Router-3 (no phantom):** every enumerable choice traces to a real source
  (local file verified / registry entry / gateway) — no fabricated model (Rule 1).
- **LIVE-Router-1:** selecting a not-ready model eventually leads to ready OR an honest
  error (download/verify or register), never a silent stuck state.

## `MemoryPack.tla` (P1, WP1.1)
State: the packed tenant as an append set of `(sourceHash, nodeId)`.
- **INV-Pack-1 (monotone):** packing never removes a prior node (append-only).
- **INV-Pack-2 (no dupes):** a `sourceHash` already packed is not re-added (idempotent).
- **INV-Pack-3 (integrity):** every packed node's content hash matches its source
  (Almanac corpus / reference pack) — no drifted/fabricated knowledge.

## `SkillLoad.tla` (P2, WP2.2)
State: the on-chain skill manifest; the loaded capsule set; the ApprovalQueue.
- **INV-Skill-1 (determinism):** the loaded set is a pure function of the manifest at
  startup (same manifest → same skills).
- **INV-Skill-2 (no unapproved execution):** a skill that signs cannot execute a signature
  without a matching approved CeremonyId (Rule 3) — mirrors the existing ceremony spec.
- **INV-Skill-3 (honest-unwired):** a skill whose backing service is absent is loadable but
  reports not-configured; it never fabricates a result (Rule 1).

## Reuse
`SignatureCeremony` already has a TLA+ model (ceremony sprint) — `SkillLoad` REFERENCES it
for the signing property rather than re-proving it. Prefer extending existing specs over new
ones where the property already holds.
