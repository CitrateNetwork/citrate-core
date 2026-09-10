---
created: 2026-07-14
branch: docs/phase-c-close
author: Claude Fable 5, directed by @SaulBuilds
status: archived
sprint: CORE Phase C (native sidecar spine: supervisor + node + node-agent + earnings + memory)
closing_commit: a7a51d2 (C2-remediation #39 merged)
purpose: Sprint retrospective for Phase C (agentile:retro). The honest accounting — including what didn't work.
---

# Retro — Phase C (native sidecar spine)

## Outcome
| | |
|---|---|
| Goal achieved? | **YES, with named honest gaps.** The native process spine is live: a supervisor spawns/monitors the citrate-node + node-agent + mem-mcp sidecars; NodeDomain/MemoryDomain are wired to real processes; earnings read real on-chain claimable; agent signature-requests route through the ceremony. |
| WPs closed | C1.0 supervisor, C1.0b (F-1/F-2 + TLA+ model), C1.1 node (+upstream chain #71 encryption), C1.2 node-agent bridge, C2 earnings, C2-remediation (honest claim), C3 memory. |
| Independent Rule-8 sign-offs | citrate-security #25/#26 (supervisor), #27/#28 (node + #71), #29 (node-agent), #30/#32 (earnings + remediation), #31 (memory). |

## Metrics delta (four ratchet axes)
| Axis | Phase B close | Phase C close |
|---|---|---|
| Rust tests | 141 | **223** (+82) |
| Frontend tests | 51 | **65** |
| Formal specs | 0 | **1** (the TLA+ SidecarSupervisor model — the first formal spec in citrate-core) |
| Frontmatter coverage | 100% | 100% |
No axis decreased.

## What worked (concrete + causal)
- **Supervisor-first + a TLA+ model.** Building the process primitive before the real
  sidecars, and modelling its state machine formally, is what caught **F-1** (the restart cap
  was lifetime-not-consecutive → a long-lived daemon would be permanently killed) that 9
  example tests missed — the model's TLC negative control reproduced the exact bug.
- **Grounding in the real sidecars before scoping** (the A3 lesson, kept). It surfaced, before
  building: the citrate-node had at-rest AEAD but the entrypoint never enabled it (plaintext);
  node-agent's bearer is a 0600 token *file* that doubles as the IPC channel; mem-mcp takes
  **no** external store key at all.
- **build-and-stop → independent review → negative control, every WP.** The reviewer caught
  what the builder's green suite could not — most importantly C3's plaintext-in-production
  store behind a "ciphertext-at-rest proof" that used a stub the app never runs.

## What didn't work (named, with cost)
- **Concurrent build agents share the working tree.** Running C2/C3/remediation in parallel
  corrupted my local branch pointer **twice** (fast-forwarded onto the wrong commit, lost a
  local merge). Cost: two recover-from-remote + redo-the-merge cycles. Fix learned mid-sprint:
  dispatch parallel mutating agents with `isolation: worktree`, and do merge-resolution in a
  dedicated `git worktree` — never the shared main tree.
- **Merge-conflict churn.** Parallel WPs touching shared files (lib.rs command registry,
  domains.ts, the bridge adapters, BASELINE.md, and every scope doc) conflicted on nearly every
  merge — resolved each, but it was the dominant time sink of the phase.
- **A review agent died on an API socket error** without filing its verdict (C1.0b). Fix:
  brief reviewers to **commit+push the record BEFORE the summary** — done for all subsequent reviews.
- **Two builder misjudgments, both caught by grounding/review:** C3 claimed an "encrypted store"
  that is plaintext in production; C1.1's builder declared the testnet "halted (PIL-42)" when
  `cast` showed it advancing (the node just couldn't sync from the deployed boot peers).

## What surprised us (highest-value)
- **mem-mcp exposes no external-KEK intake at all.** Unlike the node (#71 could wire an existing
  AEAD to a keyring key), the memory store's per-tenant keys live on disk in its own KEYS CF —
  so the full @rule8 encrypted-store property is a genuine **upstream v2 change**, not something
  C3 could achieve. The right call was to accept the documented gap, not block the last piece.
- **The "encrypted store" proofs were stub/hand-sealed.** The CI test used an XOR stub; the live
  proof hand-sealed via a `reencrypt` the app never runs. The shipped app writes the personal
  graph in the clear. The reviewer, not the builder, found this — the referee earns its keep.

## Carry-forward (destination + reason)
- **C-2 (MEDIUM):** upstream mem-store external-KEK + a production encrypting-write mode →
  citrate-memories; the memory store is plaintext at rest today (honesty-labeled in the UI/code).
- **C1.0b-1 (MEDIUM):** supervisor slow-crash policy (a daemon flapping faster than
  `healthy_after` still perma-fails) → tune per real sidecar / decaying-window.
- **Node-sync from deployed boot peers** (node built from main couldn't sync) → citrate-chain.
- **forge-tests OOM** needs a ≥16GB runner (mitigation landed in #72, but the robust fix is a
  runner change) → DevOps.
- **citrate-execution 2 failing tests on main** (precompile-address assertions, pre-existing) →
  citrate-chain team; a real red on the chain's own CI.
- **ChatGPT cross-model quorum** (all Rule-8 reviews `Reconciliation: OPEN`) → owner, standing.

## Decisions ratified (→ ADRs)
D-C1-1 (bundle sidecars as Tauri externalBin, not download), D-C1-2 (bounded sync-progress
proof + documented full run), the memory store-key seam (keyring wrapping key forward-compat).
**Action: write the D-C1 ADRs (document-pass) — not yet done.**

## Action items (owner)
- [ ] C-2 upstream mem-store external-KEK — owner/citrate-memories.
- [ ] Provision a ≥16GB forge runner (or keep the #72 thread-cap mitigation) — DevOps.
- [ ] Triage the pre-existing citrate-execution 2-test failure — chain team.
- [ ] ChatGPT cross-model quorum on the Rule-8 reviews — owner.

## Notes
- Velocity: 7 WPs + 2 upstream (chain #71, chain-CI #72) + 1 remediation. The parallel-agent
  worktree-collision tax was the biggest surprise cost; isolation:worktree is now the default
  for parallel mutating agents.
- Journals: `phase-c-*` (ch08). Essay trigger: **considered** — the "referee caught the
  plaintext store" + "shared-worktree collision" arcs may feed a later essay; none written yet.
  Case-study trigger: considered, none warranted.
- Convention: completed sprints stay flat under `completed/` (repo convention > skill's YYYY-MM).
