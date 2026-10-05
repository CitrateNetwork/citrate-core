---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-1 draft)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (primary); citrate-agent-runtime (Hermes-sidecar children, shared containment crate, via federation sprint)
tier: T1
adopts: docs/adr/0001-owned-sidecar-lifecycle.md (ADR-0001) and docs/specs/001-owned-sidecar-lifecycle.md (SPEC-001), by reference
adr: docs/adr/ADR-2026-10-05-owned-sidecar-lifecycle.md
contributor: "@mfarzanansari (Farzan Ansari): issue #240, PR #241"
tracking: citrate-core #244
companions: 01_SCOPE_OF_WORK.md, 02_ARCHITECTURE.md, 03_TLA_SPECS.md, 04_FEATURES_BDD.md, 05_SPRINTS_AND_WPS.md, 06_BUG_TRIAGE.md, 07_PROCESS_INVENTORY.md, gates.yaml
---

# Owned Sidecar Lifecycle (SCL): Planset Overview

## Credit

The design this planset adopts is **@mfarzanansari's** (Farzan Ansari): the proposal in
[issue #240](https://github.com/CitrateNetwork/citrate-core/issues/240), and ADR-0001 plus
SPEC-001 with stories SC1 to SC8 in
[PR #241](https://github.com/CitrateNetwork/citrate-core/pull/241) (head `01772a3`). The
reproduction fixtures, the story breakdown, the numeric budgets and most of the normative
contract are theirs. This planset adds our verification on the release branch, widens the
scope to every process the app starts, and sequences the work into SCL sprints. Where we
differ from their text, the difference is recorded as a delta in our
[adoption ADR](../../../docs/adr/ADR-2026-10-05-owned-sidecar-lifecycle.md).

## Why this exists

Citrate Core starts many processes. On the v0.5.0 line there are **34 kinds**: 9 long-lived
sidecars run through `kit/src/supervisor.rs` (including a second `llama-server` for BGE
embeddings), and 25 started some other way (12 by core directly, 10 by the Hermes sidecar,
3 by the node). The full list is in [07_PROCESS_INVENTORY](07_PROCESS_INVENTORY.md).

Today ownership of those processes is uneven:

- The supervisor primitive has five reproduced defects (repeated Stop can respawn, the
  retry budget resets on elapsed time alone, timed-out probe threads accumulate without a
  bound, the log ring caps lines but not bytes, and a descendant survives the stop of its
  direct parent). All five reproduce on `release/0.5.0-hermes-upskill` at `d16f194`, where
  `kit/src/supervisor.rs` is byte-identical to the contributor's audit pin `30c789e`. See
  [06_BUG_TRIAGE](06_BUG_TRIAGE.md).
- The 25 bespoke spawns each manage their own lifetime, several with no deadline, and none
  is tracked by the app's exit path.
- Exit paths differ. Window close and Cmd+Q run the sidecar shutdown; the Windows updater
  install path exits without it; force quit and crashes rely on a startup sweep that
  matches processes by name and path.

The fix is one ownership contract for every process kind, enforced by tests that stay in CI.

## Core invariant

> **Every process that Citrate Core or its Hermes sidecar starts has exactly one owner that
> can stop it, observes its readiness separately from its liveness, bounds its work and
> diagnostics, and acknowledges its cleanup before the app exits or updates; and nothing is
> reported as stopped, ready or complete unless that owner observed it.**

## Locked decisions

Locked 2026-10-05 by the owner. Reversing any of these requires a superseding ADR.

| # | Decision | Choice | Date |
|---|---|---|---|
| D-1 | Base design | **Adopt ADR-0001 / SPEC-001** (lifecycle cell per service, intent sequence and generation fencing, acknowledged controls, bounded probes, byte-bounded diagnostics, exit coordinator, retained-anchor Unix containment) by reference. Our deltas live in the adoption ADR | 2026-10-05 |
| D-2 | Release | **All of SC1 to SC8 ship in v0.5.0.** The 0.5.0 cut is gated on the SCL gates in [gates.yaml](gates.yaml). The 40204 reroll proceeds independently and is not gated on SCL | 2026-10-05 |
| D-3 | Home | Separate planset at `.agentile/planset/2026-10-05-sidecar-lifecycle/` (code **SCL**) in citrate-core, plus the adoption ADR `docs/adr/ADR-2026-10-05-owned-sidecar-lifecycle.md`. The contributor's `docs/adr/0001-owned-sidecar-lifecycle.md` and `docs/specs/001-owned-sidecar-lifecycle.md` merge to core `main` from #241 and reach the release line by forward merge. We never edit them (they carry a `content_sha256` lock) | 2026-10-05 |
| D-4 | Runtime model | Keep the std monitor thread per service. **No Tokio migration**, no generic durable operation registry, no new process-management crate | 2026-10-05 |
| D-5 | Scope | **All 34 process kinds.** The 9 supervised kinds use the lifecycle cell. The 25 bespoke kinds use a registered ownership handle: 12 in core, 3 node descendants through the node's containment, 10 Hermes-sidecar children through the same contract in citrate-agent-runtime | 2026-10-05 |
| D-6 | Retry credit | **Attested health resets the retry budget for probed services** (llama chat, llama embed, Hermes, the Comms and Cluster connect probes). **Liveness-only reset for unprobed services** (node, node agent, memory, IPFS), which report readiness `NotAssessed`, never `Ready` | 2026-10-05 |
| D-7 | Orphan cleanup | **Replace** (not just remove) the startup name/path orphan sweep with recorded-ownership cleanup: a process is signalled only when its pid, its start time and its exact binary path all match a record this app wrote. A pre-cut narrowing hotfix lands first (SCL-S0) | 2026-10-05 |
| D-8 | Exit paths | One **app exit coordinator** for quit, restart, update install and factory reset. Remove the raw `process:default` and `updater:default` capabilities from the main window; expose native coordinated commands instead | 2026-10-05 |
| D-9 | Unix containment | Owned session/process group created before exec; the unreaped leader is kept as the identity anchor through the final group signal; no group signal after the leader is reaped; never signal by executable name or by a reaped numeric id | 2026-10-05 |
| D-10 | Windows containment | **Job objects, in scope** (not parked): create suspended, assign to an owned Job, resume; no breakaway; kill-on-job-close; completion is `ActiveProcesses == 0` from Job accounting. Native acceptance run by the Windows team | 2026-10-05 |
| D-11 | One implementation | **One shared containment implementation** used by both core's kit and the Hermes sidecar in citrate-agent-runtime. Location: see O-9 (recommended default). If it crosses repos, the `[[drift]]` entry lands first (Rule 12) | 2026-10-05 |
| D-12 | Status DTO | Phase adds `stopping` and `quarantined`; readiness is a separate field (`NotAssessed`, `Awaiting`, `Ready`, `Stale`, `Unavailable`); counters cross to JavaScript as decimal strings and are compared numerically; `Accepted` never means `Complete` | 2026-10-05 |
| D-13 | Diagnostics | Byte-bounded capture and crash sink, using the contributor's numbers (16 KiB line, 256 KiB and 500 entries per ring, 64 KiB snapshot, 2 concurrent snapshots, 8 KiB crash record, 64 queued, 2 x 16 MiB rotated files) as **starting values**, measured on packaged builds before the gate flips | 2026-10-05 |
| D-14 | Keeping it fixed | A **standing lifecycle fault-injection suite** is a required CI check; a named cross-cutting owner for the supervisor contract; a red-team pass on merged code, not only on plans (see "How this was missed") | 2026-10-05 |

## Owner decisions (recommended default, pending owner sign-off)

These came out of the reconciliation research. Each row is the **recommended default,
pending owner sign-off**. The planset is written to these defaults; a different owner answer
changes the named WPs. Items the owner already decided (planset home, release target, base
design, retry policy, sweep replacement, exit coordinator, shared containment, Windows in
scope) are locked above as D-1 to D-14.

| # | Question | Recommended default, pending owner sign-off | Affects |
|---|---|---|---|
| O-1 | Adopt the contributor's tool-specific validation split (strip `created/branch/author` to check against another tool's ADR schema)? | Decline as federation policy. Our docs are checked against Rule-5 frontmatter only; contributors may run any private tooling | docs only |
| O-2 | How do legacy commands behave while consumers migrate? | A compatibility layer: legacy Start/Stop keep today's return types; legacy Stop waits for `Complete` of its named scope or returns a structured cleanup-pending error, never `Ok` on `Accepted` | SCL-S2, S6, S7, S12 |
| O-3 | Quarantine UX when a probe or cleanup worker never returns | Quarantine with no silent auto-restart. Show the reason and a user-initiated "Restart service" button (HIC-0: acting on the app's own process, not an approval card) | SCL-S4, S12 |
| O-4 | Exact `ureq` pin plus a literal-only resolver and connector for health probes | Accept: `=3.3.0` on both direct constraints, supply-chain review now, a re-gate on every `ureq` bump | SCL-S4 |
| O-5 | Thread budget (35 supervisor-owned threads, 43 with the Windows Comms slots) | Accept as a ceiling, re-measured once bespoke handles and runtime children are in scope | SCL-S5, S14 |
| O-6 | Do the contributor's three internal review rounds count as our red-team? | No. They are inputs. We run our own second-model pass on this planset before Stage-2 and again on merged code | SCL-S15 |
| O-7 | Severity and handling of the pre-cut safety items | Treat as High until native checks say otherwise; fix privately (federation #298); ship in 0.5.0 | SCL-S0 |
| O-8 | Contributor engagement | Invite @mfarzanansari to implement a story after gate0 (SC4 is the smallest and most separable). Require at least a DCO sign-off on outside code; confirm with counsel alongside the open licence question | SCL-S5 |
| O-9 | Where does the shared containment crate live? | A new keyless crate in **citrate-agent-runtime** (working name `proc-owner`; the owner picks the final name), consumed by core's `kit` through a git rev pin. Reasons: the runtime already carries `libc` and per-child process-group code, it has no core dependency, and core already builds the Hermes binary from it. Needs a `[[drift]]` entry (consumer citrate-core, dep_repo citrate-agent-runtime, file `kit/Cargo.toml`) before the Cargo line, and a federation sprint file | SCL-S9, S10, S11 |
| O-10 | Containment on by default? | On for every profile that passes its per-profile compatibility test. A profile that fails blocks its gate criterion; it does not ship silently uncontained | SCL-S9, S10 |
| O-11 | Drain order at app close | Hermes and its children drain first (they depend on node and memory), then the other owners concurrently, all inside one absolute app deadline (15 s proposed, 5 s graceful child stop inside it). The contributor's text drains all owners concurrently | SCL-S12 |
| O-12 | Force exit, crash, OS logout | Never reported as `Complete`. Captured trees on Windows die with the Job handle; on Unix the next launch's recorded-ownership cleanup handles them. The UI says cleanup was incomplete when it was | SCL-S8, S10, S12 |
| O-13 | Should Hermes `/health` reflect its children? | No. `/health` stays scoped to the sidecar's control plane. Child ownership (counts, incomplete cleanup) is reported in a separate runtime status field | SCL-S11 |
| O-14 | Windows Comms foreground slots (8) and payload caps (4 MiB per direction per slot) | Starting values, re-grounded against the Windows IPC timeout work already merged (#180) before SCL-S7.2 starts | SCL-S7 |
| O-15 | Frontend observation | Keep polling: one raw snapshot request in flight across observer epochs, 5 s cadence plus a coalesced refresh after each control. No Tauri 2.12 upgrade inside this program | SCL-S12 |
| O-16 | Who owns the cross-cutting lifecycle contract? | The owner (Larry Klosowski) until delegated; CODEOWNERS entries for the supervisor, the ownership registry, the containment crate, capabilities and the updater bridge | SCL-S13 |
| O-17 | Leftovers from 0.4.x, which wrote no ownership records | 0.5.0 keeps the narrowed cleanup from SCL-S0.1 as a one-release legacy path: only processes whose exact binary path equals one of this bundle's own sidecar binaries and that are already orphaned. Anything else is named in the UI, never signalled. The legacy path is removed in the release after 0.5.0 | SCL-S0, S8 |

## Architecture at a glance

```
┌──────────────────────────── citrate-core (Tauri) ─────────────────────────────┐
│ Webview (view only): cached status snapshot, one raw request in flight        │
│        │ native coordinated commands: quit / restart / update / reset         │
│        ▼                                                                       │
│ AppExitCoordinator ── Closing gate ── one absolute deadline ── final action    │
│        │ drains                                                                │
│        ▼                                                                       │
│ Lifecycle cells (kit/src/supervisor.rs, one std monitor each)                  │
│   node · node-agent · mem-mcp · llama chat · llama embed · ipfs ·              │
│   comms · cluster · hermes                                                     │
│   intent seq / generation / incarnation · readiness ≠ liveness ·               │
│   1 probe permit · byte-bounded logs · receipts (Accepted ≠ Complete)          │
│ Ownership registry: 12 bespoke core spawns (deadline, handle, record)          │
│ Ownership record (pid + start time + exact path) ──► next-launch cleanup       │
│        │ every spawn goes through                                              │
│        ▼                                                                       │
│ Shared containment (proc-owner): Unix owned session + retained anchor;         │
│                                  Windows owned Job (no breakaway, kill-on-close)│
└────────┼───────────────────────────────────────────────────────────────────────┘
         │ hermes (supervised)
┌────────▼──────────── Hermes sidecar (citrate-agent-runtime) ──────────────────┐
│ Same contract for its 10 child kinds: workers, managed browser, search,       │
│ toolchain runs, shell_run, MCP stdio servers, MCP probe, git, GPU sampler      │
│ Uses the same shared containment; reports child ownership separately from     │
│ /health; drains inside core's close deadline                                   │
└────────────────────────────────────────────────────────────────────────────────┘
```

## Surfaces we consume (reuse map)

Nothing on this list gets rebuilt.

| Need | Existing surface | Where |
|---|---|---|
| Per-service monitor, backoff, crash records, log ring | `Supervisor`, `SidecarSpec`, `BackoffPolicy`, `HealthCheck` | core `kit/src/supervisor.rs` (+ `supervisor_tests.rs`, 22 tests; `supervisor_windows_tests.rs`, 3) |
| Formal model of the supervisor | `SidecarSupervisor.tla` (INV-1 to INV-4, fork-bomb config) | core `src-tauri/formal/` |
| Service managers and their prerequisites | `node.rs`, `agent.rs`, `memory.rs`, `serve.rs`, `embed_serve.rs`, `ipfs.rs`, `comms.rs`, `cluster.rs`, `hermes.rs` | core `src-tauri/src/` |
| App teardown hook | `shutdown_all_sidecars`, `RunEvent::ExitRequested` / `Exit` handler | core `src-tauri/src/lib.rs` |
| Hermes sidecar graceful shutdown | SIGTERM / Ctrl-C handler, `shutdown_children` order (sessions, workers, browser, search, MCP) | runtime `agent-sidecar/src/main.rs`, `sessions.rs` |
| Worker supervision inside the sidecar | `agent-workers` (restart policy, ping health, stdin-EOF exit) | runtime `agent-workers/` |
| OS sandbox for agent commands | Seatbelt (macOS), bubblewrap (Linux), per-command process group | runtime `agent-shell/` |
| App updater | tauri-plugin-updater 2.10.1 (signature, feed, platform policy), `src/shell/updater.ts` | core |
| Chain DB reset on a genesis change | #243 (refuses while an old node answers on local RPC; keep-list of identity files) | core `node.rs` |
| Windows named-pipe deadline helper | the Comms foreground helper and `sprint-windows-ipc-timeouts` | core `comms.rs` (#180) |
| Windows API bindings | `windows-sys` 0.61 (already in the tree; Job objects need feature flags only, no new crate) | core `src-tauri/Cargo.toml` |
| Unix signalling | `libc` 0.2 (already a direct dependency of kit and src-tauri) | core |
| TLC runner | `scripts/run-tlc.sh` | core |
| Methodology | Agentile red-green, refactor-mutate, test-plan, retro, journal | agentile-skills |

## Scope

**In:** every process kind in [07_PROCESS_INVENTORY](07_PROCESS_INVENTORY.md); every exit
path in its exit-path table; SC1 to SC8 as written in SPEC-001 plus the deltas in the
adoption ADR; Windows Job containment; Hermes-sidecar children; recorded-ownership cleanup;
a standing fault-injection suite; packaged native acceptance on macOS, Linux and Windows;
a red-team pass.

**Out:** durable operation replay or a generic operation registry; group-message
retention (separately parked in SPEC-001); agent-session cancellation semantics (owned by
the Hermes loop ADR); updater signing, feed or channel policy; any change to ceremony,
custody or budget rules; processes started by the user's own tools outside the app (for
example a member's own anvil or Chrome); a universal "no orphan, ever" guarantee for
descendants that deliberately leave their session or Job.

## Safety gates

The app **refuses** an action unless all of these hold:

1. **Signal only what we own.** A signal goes to a process (or group, or Job) only when
   this app holds a live handle to it, or a recorded-ownership entry matches its pid, its
   start time and its exact binary path. Never by name, path substring or reaped id.
2. **Stop wins.** After a Stop or app Closing is committed, no new spawn, probe or
   credential publication is admitted for that owner. Work admitted earlier is still
   owned and goes straight to teardown.
3. **No invented state.** `Ready` only from a fresh probe of the current incarnation.
   `Complete` only from observed teardown of the named scope. Anything unobserved is
   `Incomplete`, `ObservationUnknown` or `Quarantined`, and the UI says so.
4. **No final action before Complete.** Exit, restart and installer launch run only after
   the coordinator reports `Complete` inside its deadline. An expired deadline never
   authorizes a late final action.
5. **Custody unchanged.** Generation-owned cleanup never deletes a key or credential that
   a newer generation uses, and preserves the #243 keep-list. Nothing in SCL signs.
6. **@rule8 review** for the WPs that change process-spawn-sensitive code (SCL-S8.3, S9,
   S10) and the app updater path and capability grants (SCL-S0.2, S12.2), per core
   CLAUDE.md rule 7.

## Release coupling

D-2 makes the SCL gates part of the v0.5.0 cut. The HUP release gates carry one added line
(`g5-scl` in [HUP gates.yaml](../2026-09-30-hermes-upskill/gates.yaml)) that points here.
Nothing else in the HUP planset changes. The SCL program's own gates are in
[gates.yaml](gates.yaml). The 40204 reroll is not gated on SCL.

Two timing facts shape SCL-S0 (from the reconciliation research): the 0.4.x to 0.5.0
update runs **0.4.x's** updater and exit code, so 0.5.0 can only defend at its own startup;
and the 0.5.0 to 0.5.x update runs **0.5.0's** code, so whatever 0.5.0 ships for the update
exit path is what every member's next update uses.

## How this was missed

The supervisor had an independent Rule-8 review at birth, a TLA+ model, and 25 tests. All
five defects still shipped. The honest reasons:

1. **Reviews were lane-scoped.** Each manager, sprint and fan-out lane was reviewed against
   its own scope. The supervisor contract crosses all nine services, the Hermes sidecar and
   every exit path, and no lane owned it. The HUP process split (S1.9) added new children
   inside the sidecar without anyone re-checking the contract the core side relies on.
2. **No owner for the cross-cutting contract.** Nobody was responsible for asking "who stops
   this process on every exit path?" when a new spawn site landed. The Hermes sidecar's
   children and the core bespoke spawns grew to 25 kinds without that question.
3. **The model checked an abstraction, not the code.** `SidecarSupervisor.tla` models Stop
   only from live states (so Stop-while-Off, bug a, was outside it), treats "healthy" as an
   abstract flag (so elapsed time standing in for health, bug b, could not show up), and has
   no probe workers, bytes or descendants (bugs c, d, e).
4. **No fault-injection tests.** The example-based tests used cooperative children. Nothing
   held a probe blocked, wrote a 1 MiB line, called Stop twice, or spawned a grandchild.

**Fix, built into this program:** a standing fault-injection suite that is a required CI
check and cannot shrink without an ADR (SCL-S13); a named cross-cutting owner with
CODEOWNERS on the contract surfaces (O-16); TLA+ mutant configs that must reproduce each
reproduced bug as a counterexample (SCL-S1); and a red-team pass on merged code, not only on
plans (SCL-S15).

## Private matters

citrate-core is a public repository. Some pre-cut safety items have security impact. Their
specifics (affected platforms, packaging, exact mechanisms) are tracked privately on
federation #298 and are described here only generically as "pre-cut safety fixes tracked
privately". Security-sensitive findings discovered while executing this planset are
remediated privately, per owner policy, and are not described in public PR or issue text.

## Home of this planset

citrate-core, following the HUP precedent (an exception to the core
`.agentile/planset/README.md` pointer and CLAUDE.md rule 6, recorded here as HUP recorded
it). The runtime and manifest work is cross-repo, so a federation sprint file
(`citrate-federation/.agentile/sprints/active/2026-10-05-scl-sidecar-lifecycle.md`) is
created before the first runtime PR (SCL-S9.0).
