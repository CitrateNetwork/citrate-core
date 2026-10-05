---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: proposed (Stage-1 draft; pending owner sign-off; changes nothing until accepted)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (primary); citrate-agent-runtime
adopts: docs/adr/0001-owned-sidecar-lifecycle.md (ADR-0001) and docs/specs/001-owned-sidecar-lifecycle.md (SPEC-001), by reference; those files are not edited
contributor: "@mfarzanansari (Farzan Ansari): issue #240, PR #241"
relates_to: ADR-2026-09-30-hermes-loop-in-sidecar.md, ADR-2026-08-28-cluster-daemon-extraction.md, ADR-2026-10-01-device-keys-and-devicelink.md
tracking: citrate-core #244
companions: .agentile/planset/2026-10-05-sidecar-lifecycle/00_OVERVIEW.md, .agentile/planset/2026-10-05-sidecar-lifecycle/gates.yaml
---

# ADR-2026-10-05: Owned sidecar lifecycle (adopting ADR-0001 / SPEC-001)

## Credit

This decision adopts the design of **@mfarzanansari** (Farzan Ansari), proposed in
[issue #240](https://github.com/CitrateNetwork/citrate-core/issues/240) and written up as
[ADR-0001](0001-owned-sidecar-lifecycle.md) and
[SPEC-001](../specs/001-owned-sidecar-lifecycle.md) in
[PR #241](https://github.com/CitrateNetwork/citrate-core/pull/241) (head `01772a3`). The
evidence, the eight-story breakdown (SC1 to SC8), the numeric budgets and most of the
normative contract are theirs. Thank you.

The two linked files are merged to core `main` from #241 and reach
`release/0.5.0-hermes-upskill` by forward merge; until then the links resolve on `main` and
in #241. They carry a `content_sha256` lock and are **not edited** here or later. Their
`status: locked` is a term from the contributor's tooling meaning "content-locked"; it does
not mean owner-accepted. This ADR is the adoption record.

## Status

**Proposed, 2026-10-05.** The owner has decided D-1 to D-14 of the planset (scope, release,
home, base design and the deltas below). Acceptance of this ADR is gate criterion `g0-adr`.
Owner decisions O-1 to O-17 in the planset are recommended defaults, pending owner sign-off.

## Context

- The shared supervisor (`kit/src/supervisor.rs`) is byte-identical on `main` at `30c789e`
  (the contributor's audit pin) and on the release branch at `d16f194`.
- We re-ran the contributor's five observations on the release branch on macOS (twice, 5 of
  5 reproduced): repeated Stop respawns, elapsed-time retry credit, unbounded probe threads,
  unbounded log bytes, a surviving descendant. Results and reachability limits are in
  [06_BUG_TRIAGE](../../.agentile/planset/2026-10-05-sidecar-lifecycle/06_BUG_TRIAGE.md).
  We did not re-run their Linux namespace baseline, and no packaged app or live incident was
  measured.
- An inventory of the v0.5.0 line found **34** process kinds, not 8: 9 supervised (the BGE
  embed server is a second `llama-server`), 12 bespoke spawns in core, 10 children of the
  Hermes sidecar, 3 descendants of the node
  ([07_PROCESS_INVENTORY](../../.agentile/planset/2026-10-05-sidecar-lifecycle/07_PROCESS_INVENTORY.md)).
- Some lifecycle items have security impact. Their specifics are tracked privately (federation
  #298) and appear in public text only as "pre-cut safety fixes tracked privately".

## Decision

1. **Adopt ADR-0001 and SPEC-001 as the base design**, by reference: one lifecycle cell per
   supervised service on the existing std monitor; intent sequence, generation and
   incarnation fencing; spawn and publication tickets; acknowledged receipts where
   `Accepted` is never `Complete`; readiness separate from liveness; one probe permit per
   service held across timeouts; a literal-only loopback health adapter; byte-bounded
   diagnostics; an app exit coordinator; retained-anchor Unix containment. SPEC-001's
   "Normative contract" is the detailed contract for supervised services.
2. **Apply the deltas below.** Where SPEC-001 and a delta disagree, the delta governs.
3. **Ship in v0.5.0** (owner, 2026-10-05). All of SC1 to SC8, plus the deltas, are gated by
   the SCL gates; the HUP v0.5.0 release gate depends on them. The 40204 reroll is not gated
   on SCL.

### Deltas from ADR-0001 / SPEC-001

| # | Topic | ADR-0001 / SPEC-001 | This ADR | Why |
|---|---|---|---|---|
| Δ-1 | Scope | 8 shared services | **All 34 process kinds**: 9 supervised (adds the BGE embed server), 12 core bespoke via an ownership handle, 3 node descendants via the node's containment, 10 Hermes-sidecar children via the same contract in citrate-agent-runtime | The core invariant is about every process the app starts |
| Δ-2 | Windows descendants | Job ownership PARKED; Windows reports direct-child capability | **In scope**: create suspended, assign to an owned Job, resume; no breakaway; kill-on-job-close; `Complete` only at `ActiveProcesses == 0`. Native gate run by the Windows team | Owner scope "everything"; the Windows update path is where orphans matter most |
| Δ-3 | Startup orphan cleanup | Remove executable-directory and name-wide killing | **Replace** it with recorded-ownership cleanup (pid + process start time + exact binary path, all must match), plus a pre-cut narrowing hotfix and a one-release legacy path for 0.4.x leftovers (O-17) | Removing it alone would leave crash orphans and 0.4.x leftovers, which the #243 chain reset depends on being gone |
| Δ-4 | Containment implementation | Core-only Unix adapter | **One shared crate** used by core's kit and the runtime (recommended home: citrate-agent-runtime, O-9), with a `[[drift]]` entry first (Rule 12) | Rule 9; the runtime already has its own process-group code that must follow the same rules |
| Δ-5 | Nested ownership | Not addressed | The Hermes sidecar owns its children, keeps its own anchors, writes ownership records core can read, and acknowledges child cleanup inside Hermes Stop | Children that start their own session or group are outside a group signal to Hermes |
| Δ-6 | Drain order | All owners concurrently | Hermes and its children first, then the rest concurrently, inside one absolute deadline (O-11, pending sign-off) | Hermes depends on the node and memory |
| Δ-7 | Exit paths covered | Quit, restart, update install | Adds factory reset, the Hermes setting-change restart and the #243 chain reset (which runs only after the node owner reports `Complete`) | Every path that ends or replaces a process |
| Δ-8 | Release | No release approval implied | Ships in v0.5.0; SCL gates gate the cut | Owner decision 2026-10-05 |
| Δ-9 | Keeping it fixed | Future acceptance tests | A **standing fault-injection suite** as a required CI check on three OSes, a manifest tripwire so fixtures cannot be dropped without an ADR, TLA+ mutant configs that must reproduce bugs (a) to (c), a named cross-cutting owner, and a red-team pass on merged code | The prior model and tests were green while all five defects were present |
| Δ-10 | Formal model | Not specified | Extend `SidecarSupervisor.tla` (intent and generation fencing, attested retry credit, probe permit) and add `AppExitCoordinator.tla` | Rule 9: extend, do not duplicate |
| Δ-11 | Thread budget | 35 (43 with Windows Comms slots) | 38 (46) for 9 cells, plus a bounded bespoke reader pool; ceilings to measure (O-5) | Embed counted as its own owner |
| Δ-12 | Validation split for another tool's ADR schema | Proposed as policy | Declined as federation policy (O-1, pending sign-off); Rule-5 frontmatter only | One validation contract for our docs |
| Δ-13 | Red-team | Contributor's three internal rounds | Inputs only; our own second-model pass before Stage-2 and on merged code (O-6) | Independence |

Unchanged from SPEC-001 and adopted as written: no Tokio migration; no generic durable
operation registry; no new process-management crate; std monitor as sole process writer;
retry credit attested for probed services and liveness-only for unprobed ones (the owner
locked exactly this split); the status DTO with `stopping` and `quarantined` and decimal
string counters; the diagnostic budgets as starting values to be measured; removal of the
raw `process:default` and `updater:default` capabilities in favor of native coordinated
commands; force exit and crashes are never reported as `Complete`.

## Consequences

- A cross-cutting change to every manager, its commands and the frontend store, behind a
  compatibility layer (O-2). The existing supervisor tests that pin today's semantics are
  rewritten in place; any removal needs an ADR line (Rule 2).
- The F-1 retry property in `SidecarSupervisor.tla` changes for probed services. INV-1 is
  kept for liveness-only profiles and restated for probed ones.
- Availability drops on purpose when a worker never returns: the service is quarantined
  with a visible reason and a user restart (O-3), rather than accumulating threads.
- The app updater path and the process-spawn code are @rule8 surfaces; the WPs that change
  them carry a recorded @rule8 review.
- One new cross-repo dependency (core's kit on the shared containment crate), added through
  the drift map first.
- v0.5.0 waits for the SCL gates.

## What this ADR does not claim

- Nothing here is implemented. Reproduction is recorded; fixes are planned.
- No claim of cleanup for descendants that deliberately leave their session or Job; the
  escape fixture records them as outside scope.
- No total-app thread or memory ceiling; the budgets are for the named owners.
- Native claims (Windows Job behavior, Linux and macOS containment on packaged builds, the
  Windows update path) are unproven until the native runs listed in the planset's
  01_SCOPE_OF_WORK are recorded.

## Alternatives considered

- **Fold SCL into the HUP planset as another epic.** Rejected by the owner: a separate
  planset keeps the contract reviewable on its own, with its own gates, while the HUP gate
  depends on it.
- **Merge the contributor's text into our format and drop their files.** Rejected: their
  files are the record of their work, carry a content lock, and are now on `main`. We adopt
  them by reference and keep our deltas here (Rule 9).
- **Keep Windows parked as SPEC-001 proposes.** Rejected by the owner (scope "everything").
- **Remove the startup sweep without a replacement.** Rejected: crash orphans and 0.4.x
  leftovers would hold the chain database and ports.

## Sign-off

| Role | Name | Date | Decision |
|---|---|---|---|
| Owner, federation lead | Larry Klosowski | | |
| @rule8 (updater and process-spawn surfaces) | | | |
| Core maintainer | | | |
| Runtime maintainer | | | |
