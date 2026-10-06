---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed)
red_teamed: 2026-10-05 (adversarial pass, 29 findings, 3 blocking; corrections in 08_RED_TEAM.md supersede conflicting text)
updated: 2026-10-05 (owner decisions: O-20 accepted, D-2 amended to a cut-blocking subset; O-18 and O-19 accepted as D-15 and D-16; red-team pass accepted as D-17; second set: D-2 amended again to v0.5.0 / v0.5.1 / v0.5.x, D-16 extended to ask first for a stopped local model, HUP D-41 amended for one v0.5.1; third set: minimum viable v0.5.0 (S7.5a moves to v0.5.1), v0.5.x renamed v0.5.2, HUP D-41 amended for one v0.5.1 and one v0.5.2, owners assigned)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (primary); citrate-agent-runtime (Hermes-sidecar children, shared containment crate, via federation sprint)
tier: T1
adopts: docs/adr/0001-owned-sidecar-lifecycle.md (ADR-0001) and docs/specs/001-owned-sidecar-lifecycle.md (SPEC-001), by reference
adr: docs/adr/ADR-2026-10-05-owned-sidecar-lifecycle.md
contributor: "@mfarzanansari (Farzan Ansari): issue #240, PR #241"
tracking: citrate-core #244
companions: 01_SCOPE_OF_WORK.md, 02_ARCHITECTURE.md, 03_TLA_SPECS.md, 04_FEATURES_BDD.md, 05_SPRINTS_AND_WPS.md, 06_BUG_TRIAGE.md, 07_PROCESS_INVENTORY.md, 08_RED_TEAM.md, gates.yaml
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
| D-2 | Release | **All of SC1 to SC8 ship in v0.5.0.** The 0.5.0 cut is gated on the SCL gates in [gates.yaml](gates.yaml). The 40204 reroll proceeds independently and is not gated on SCL. *Owner decision (2026-10-05): superseded by D-2 (amended) below* | 2026-10-05 |
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
| D-2 (amended) | Release (O-20 accepted) | **Owner decision (2026-10-05).** The v0.5.0 cut is gated on a **cut-blocking subset** of SCL: every criterion marked `blocks_cut: true` in [gates.yaml](gates.yaml). Everything else in SCL finishes on the 0.5.x line under this same planset (still owned, still gated, not blocking the cut). The subset is in [Cut-blocking subset](#owner-decision-2026-10-05-cut-blocking-subset). The 40204 reroll stays independent of SCL. *Owner decision (2026-10-05, second set): superseded by D-2 (amended again) below* | 2026-10-05 |
| D-15 | Quit always ends (O-18 accepted) | **Owner decision (2026-10-05).** Install and restart require `Complete`. Quit (and the factory-reset exit) force-stops every OS-process scope it still owns at the deadline and exits, reporting `Incomplete`; ownership records of anything not observed absent stay for next-launch cleanup, and the next launch tells the member. In-process threads never keep the app alive | 2026-10-05 |
| D-16 | No silent gateway fallback (O-19 accepted) | **Owner decision (2026-10-05).** While the local model is cold-loading or its probe times out, chat waits or tells the member; it never routes the prompt to the remote gateway without the member's explicit choice. Readiness gates admission to the local server and never switches provider by itself. Criterion `g3-provider-routing` blocks the cut. *Owner decision (2026-10-05, second set): extended by D-16 (extended) below* | 2026-10-05 |
| D-17 | Red-team gate | **Owner decision (2026-10-05).** The single-model adversarial pass in [08_RED_TEAM](08_RED_TEAM.md) (commit `2db312f`) is accepted as the pre-Stage-2 red-team; `g0-redteam` is met. The red-team on merged code (SCL-S15.2) is unchanged | 2026-10-05 |
| D-2 (amended again) | Release path | **Owner decision (2026-10-05, second set).** v0.5.0 ships right after the 40204 reroll and its 2,000-block soak with **only** SCL-S0 pre-cut safety (S0.1, S0.3 to S0.7, including the Windows NSIS pre-install stop S0.7) and the v0.5.0 slice of S8.5 (S8.5a: startup barrier before node admission, #243 database-lock check), plus the minimum they strictly depend on (S1.6a, a hosted Windows CI job for the S0.7 hook test) and the minimal ask-first S7.5a (D-16 extended). The previous cut-blocking subset becomes the **v0.5.1** release gate. Everything else stays 0.5.x follow-up. gates.yaml carries `release: v0.5.0 / v0.5.1 / v0.5.x` per criterion in place of `blocks_cut`. Table: [Three releases](#owner-decision-2026-10-05-second-set-three-releases). *Owner decision (2026-10-05, third set): superseded by D-2 (amended a third time) below; S7.5a moves to v0.5.1 and v0.5.x becomes v0.5.2* | 2026-10-05 |
| D-16 (extended) | Ask first, never silent | **Owner decision (2026-10-05, second set).** In addition to D-16 (cold load or probe timeout never routes to the gateway silently): when the local model is `Failed`, `Stopped` or `Quarantined`, chat asks the member first: restart the local model, or send this message to the gateway this time. The choice is per message, never preselected or remembered. Never silent. v0.5.0 ships the stopped case on today's signals (S7.5a, `g3-provider-routing-v050`); v0.5.1 ships the full rule on the readiness model (S7.5, `g3-provider-routing`). *Owner decision (2026-10-05, third set): S7.5a moves to v0.5.1 with S7.5 (`g3-provider-routing-v050` keeps its id, `release: v0.5.1`); v0.5.0 ships today's silent route for a stopped local server as a known issue* | 2026-10-05 |
| D-41 (HUP, amended) | One v0.5.1 | **Owner decision (2026-10-05, second set).** The HUP release cadence D-41 is amended to allow exactly one v0.5.1 after v0.5.0, carrying the SCL v0.5.1 gate. Recorded in the HUP planset (00 D-41, 10_RELEASE_PLAN, `g5-scl`). *Owner decision (2026-10-05, third set): superseded by D-41 (HUP, amended again) below* | 2026-10-05 |
| D-2 (amended a third time) | Minimum viable v0.5.0 | **Owner decision (2026-10-05, third set).** v0.5.0 is the minimum viable release: SCL-S0 only (S0.1, S0.3, S0.4, S0.5, S0.6, S0.7, S1.6a, S8.5a). **S7.5a moves to v0.5.1**: the owner asked for the minimum viable 0.5.0, and S7.5a is not a safety fix. **v0.5.1** is the previous v0.5.1 set plus S7.5a. The previous **v0.5.x** remainder becomes **v0.5.2**. gates.yaml: every `release: v0.5.x` becomes `release: v0.5.2`, and `g3-provider-routing-v050` becomes `release: v0.5.1` (ids kept). Table: [Third set](#owner-decision-2026-10-05-third-set-v050-v051-v052) | 2026-10-05 |
| D-41 (HUP, amended again) | One v0.5.1 and one v0.5.2 | **Owner decision (2026-10-05, third set).** The HUP release cadence D-41 allows exactly one v0.5.1 **and** exactly one v0.5.2 after v0.5.0, carrying the SCL v0.5.1 and v0.5.2 gates respectively. Recorded in the HUP planset (00 D-41, 10_RELEASE_PLAN, `g5-scl`). This answers the second set's open question ("the v0.5.x remainder has no release slot") | 2026-10-05 |
| D-18 | Owners | **Owner decision (2026-10-05, third set).** Each release's work has a named owner, tracked on one issue per lane (#247 to #253). Table: [Owners](#owners-owner-decision-2026-10-05-third-set); WP-level detail in [05](05_SPRINTS_AND_WPS.md#owners-third-set). Assignments marked *proposed* wait on the contributor's acceptance | 2026-10-05 |

## Owner decisions (recommended default, pending owner sign-off)

*Owner decision (2026-10-05):* O-1 to O-17 below remain recommended defaults, pending owner
sign-off. O-18, O-19 and O-20 (raised in 08) are decided and locked as D-15, D-16 and D-2
(amended). *Owner decision (2026-10-05, second set):* O-19 is extended (D-16 extended) and D-2
is amended again; see Locked decisions. O-17's "removed in the release after 0.5.0" no longer
fits: 0.5.0 writes no ownership records (S8.3 is v0.5.1), so the legacy path must stay in
v0.5.1 too. The recommended default becomes "removed no earlier than the release after
v0.5.1", still pending sign-off.

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

## Red-team findings (2026-10-05, supersede the text above and below)

An adversarial pass (SCL-S15.1) checked this planset against code on the release branch, the
runtime and the locked dependency sources. The full table, evidence and the critical-path
estimate are in [08_RED_TEAM](08_RED_TEAM.md). Where these corrections conflict with any text
in this planset or the adoption ADR, the corrections govern. Affected passages carry a
"Red-team correction (2026-10-05)" note so the original text stays visible.

**Blocking (must be resolved before Stage-2 is accepted):**

1. **RT-01. Unpreventable exits.** On macOS, Cmd+Q, Dock Quit and logout arrive only as
   `RunEvent::Exit`, which cannot be prevented. The coordinator gets a synchronous bounded
   drain mode inside `Exit` in addition to its async mode, and the app menu's Quit item calls
   the coordinated command (SCL-S12.6).
2. **RT-02. Quit must always end.** Proposed O-18: install and restart still need `Complete`;
   Quit and the factory-reset exit proceed at the deadline after a final actuation of every
   OS-process scope still held, record `Incomplete`, and report it at next launch. In-process
   threads never keep the app alive (SCL-S12.7).
3. **RT-03. The in-app updater is live on macOS only.** Windows and Linux bundles ship no
   updater artifacts. Native gate rows that need a Windows or Linux in-app install are
   replaced by the manual installer path, and S0.2 becomes conditional on a Windows feed
   (with RT-04, new SCL-S0.7).

**Other corrections:** a stable-Rust mechanism for the Windows Job backend (RT-05, S10.0);
per-OS child matrix (RT-06); leader-first stop for nested owners (RT-07); a runtime Closing
gate before the child report, modelled in TLA+ (RT-08, S1.7, S11.6); runtime ownership
records are claims, not authority (RT-09, S11.7); boot identity in records and a defined path
rule (RT-10); startup cleanup is a barrier before node admission and the #243 reset also
checks the database lock (RT-11, S8.5); readiness never switches chat to the remote gateway
by itself (RT-12, proposed O-19, S7.5); containment lanes start at gate0 (RT-13); Windows and
macOS CI lanes first (RT-14, S1.6); a recorded estimate (RT-15); update staging, relaunch
during Closing and pending approvals rejected on Closing (RT-18, RT-20, RT-22, S12.8); two
native-run windows (RT-25, S14.5); Hermes-first drain with a sub-deadline (RT-26).

**Owner decisions added (recommended defaults, pending sign-off):** O-18 (quit with
`Incomplete`), O-19 (no silent local-to-gateway switch), O-20 (optionally split cut-blocking
gates from 0.5.0-line follow-up; D-2 stands until the owner changes it). Text in 08.
*Owner decision (2026-10-05):* all three accepted, locked as D-15 (O-18), D-16 (O-19) and D-2
(amended) (O-20). The owner also accepted this pass as the pre-Stage-2 red-team (D-17).

**Estimate:** about 280 agent-days of total effort; a critical path of about 70 to 92
agent-days, which is roughly 10 to 14 calendar weeks with parallel lanes. A cut that waits on
every SCL gate lands about mid-December 2026 to mid-January 2027 (08 "Critical path").
*Owner decision (2026-10-05):* superseded for the cut by the cut-blocking estimate below.

## Owner decision (2026-10-05): cut-blocking subset

O-20 accepted, D-2 amended. The v0.5.0 cut waits only for the left column. The right column
finishes on the 0.5.x line under this planset, gated by the `blocks_cut: false` criteria in
[gates.yaml](gates.yaml). WP-level detail, the reasons for every move between columns and
the dependency graph are in
[05 "Owner decision (2026-10-05)"](05_SPRINTS_AND_WPS.md#owner-decision-2026-10-05-cut-blocking-subset).

| Cut-blocking (v0.5.0) | 0.5.x follow-up (same planset, still gated) |
|---|---|
| **Pre-cut safety:** all of SCL-S0 that applies to 0.5.0 (S0.1, S0.3 to S0.7), including the Windows NSIS pre-install stop of this installation's own sidecars (S0.7, RT-04) | S0.2, conditional on a Windows updater feed (none in 0.5.0, RT-03) |
| **Theory (gate0):** supervisor TLA extension, `AppExitCoordinator.tla`, fault harness, re-grounding and baseline, Windows and macOS CI lanes (S1.6), TLA corrections (S1.7) | `OwnedContainment.tla`, if needed |
| **Primitive:** SC1 acknowledged controls (S2), SC2 readiness and retry credit (S3), SC3 bounded probes (S4), SC4 bounded diagnostics (S5) | (none) |
| **Supervised owners:** llama chat and embed readiness (S7.1), provider routing under D-16 (S7.5), node watchdog maps the real Stop receipt (S6.4a); all 9 services on the new cell through the O-2 compatibility layer | Node, node agent, memory, IPFS, Comms, Cluster and Hermes manager migrations and their consumers (S6.1 to S6.3, S6.4b, S7.2 to S7.4) |
| **Core cleanup:** ownership registry and spawn tripwire (S8.1), recorded-ownership cleanup replaces the name/path sweep for core-spawned processes (S8.3), startup barrier and database-lock check for #243 (S8.5) | Bespoke kinds onto the registry (S8.2), node descendants contained (S8.4) |
| **Hermes children (minimum):** runtime Closing gate and child report so Hermes can be observed `Complete` (S11.4, S11.5a, S11.6) | All 25 bespoke and Hermes-sidecar children under the full contract (S11.1 to S11.3b, S11.5b, S11.7) |
| **Containment:** none; Quit's final actuation reaches direct children | Shared containment crate and Unix group containment (S9), Windows Job objects (S10) |
| **Exit, quit, update:** coordinator (S12.1), raw `process:default` and `updater:default` removed (S12.2), factory reset and #243 reset through `Complete` (S12.3), cut exit-path matrix (S12.5a), synchronous macOS quit fallback (S12.6, RT-01), Quit always ends (S12.7, D-15), `UpdateStaged` and the other Closing edges (S12.8, RT-20) | Frontend observer (S12.4), exit-matrix rows for Hermes children and Windows Job (S12.5b) |
| **Keeping it fixed:** standing fault-injection suite as a required CI check on three OSes for every fixture of the cut, tripwire, mutation pass, TLC in CI, CODEOWNERS (S13) | New fixtures join the same check as 0.5.x WPs land |
| **Native:** packaged macOS acceptance of the subset (S14.1 cut, S14.4 macOS budgets), Linux smoke on the DGX (S14.6), Windows S0.7 run, native-run calendar (S14.5) | Full three-OS native acceptance (S14.1 to S14.4) |
| **Red-team:** pre-Stage-2 pass accepted (D-17); red-team of the merged cut code (S15.2, S15.3 slice) | Red-team of the 0.5.x remainder |

**Moved by dependency** (not on the owner's list): S1.4 and S1.5, S6.4a, S8.1, S11.4, S11.5a
and S11.6, S12.1, S12.3, all of S12.8, S13.5, the S14.4 macOS slice and S14.5. The one that
matters most: D-15 keeps Restart and install requiring `Complete`, and Hermes is `Complete`
only when its children are (X-7), so without the runtime's child report the macOS update
restart would be refused whenever Hermes runs. **Moved out:** S0.2 (no Windows feed in
0.5.0). **Residual at the cut:** bug (e) stays open for descendants of supervised services
until S9 to S11.

**Estimate** (S = 1, M = 3, L = 7 agent-days; reviews, native runs and sign-offs do not
compress):

| | Cut-blocking subset | 0.5.x remainder |
|---|---|---|
| Work packages | 53 | 34 |
| Total effort | about 165 agent-days | about 135 agent-days |
| Critical path | about 51 to 70 agent-days | about 47 to 54 agent-days |
| Calendar, 4 to 5 parallel lanes | 7 to 10 weeks: cut about late November to mid-December 2026 | 7 to 9 weeks after the cut (4 to 6 if containment lanes start before it): about mid-January to mid-February 2027 |

The single-cut plan landed the cut about mid-December 2026 to mid-January 2027. The split
saves three to four weeks on the cut, because the longest chain (gate0, S2 to S4, owners,
coordinator, native, red-team) is shared. The larger gain is risk: the cut no longer waits on
the Windows Job mechanism or a full Windows native window.

## Owner decision (2026-10-05, second set): three releases

D-2 amended again; this section supersedes "cut-blocking subset" above where they differ (the
section above is kept as the record; its left column, less S0 and S8.5a, is now v0.5.1).
WP-level detail, the dependency reasoning and the O-19 call are in
[05 "Owner decision (2026-10-05, second set)"](05_SPRINTS_AND_WPS.md#owner-decision-2026-10-05-second-set-v050-v051-v05x).

| v0.5.0 (right after the reroll and 2,000-block soak) | v0.5.1 (the one release D-41 now allows) | v0.5.x (follow-up, same planset) |
|---|---|---|
| SCL-S0: S0.1, S0.3, S0.4, S0.5, S0.6, S0.7 (Windows NSIS pre-install stop of this installation's own sidecars) | (none) | S0.2, conditional on a Windows updater feed |
| S8.5a: startup cleanup barrier before node admission (over the S0.1 cleanup); #243 reset also requires the chain database lock | S8.1 registry and tripwire, S8.3 recorded-ownership cleanup, S8.5b boot identity and path rule | S8.2 bespoke kinds, S8.4 node descendants |
| S1.6a: hosted Windows CI job for the S0.7 hook test (the only strict dependency S0 and S8.5a have; no Windows CI lane exists today) | gate0: S1.1 to S1.5, the rest of S1.6, S1.7 | `OwnedContainment.tla` if needed |
| S7.5a: ask first when the local server is not running, on today's signals (D-16 extended) | SC1 to SC4 (S2 to S5); S7.1, S7.5 (full D-16 on readiness), S6.4a; all 9 services on the new cell through the O-2 layer | S6.1 to S6.3, S6.4b, S7.2 to S7.4 |
| | Hermes child report minimum: S11.4, S11.5a, S11.6 | S11.1, S11.2, S11.3a, S11.3b, S11.5b, S11.7 |
| | Exit, quit, update: S12.1 to S12.3, S12.5a, S12.6 to S12.8 | S12.4, S12.5b |
| | (no containment) | S9 Unix containment, S10 Windows Job objects |
| | S13 fault suite over every v0.5.0 and v0.5.1 fixture | New fixtures join the same check |
| Native: macOS 0.4.2 to 0.5.0 update on the rerolled chain, DGX Linux manual update, Windows S0.7 run | Native: S14.1 cut scenarios plus the 0.5.0 to 0.5.1 update, S14.4 macOS slice, S14.5, S14.6 | Full three-OS acceptance |
| Red-team: covered by HUP `g5-redteam`; S0 private route (#298) | S15.2, S15.3 on the v0.5.1 code | S15 on the remainder |

| | v0.5.0 | v0.5.1 | v0.5.x remainder |
|---|---|---|---|
| Work packages | 9 | 47 | 34 |
| Effort | about 19 agent-days (16 without S7.5a) | about 150 agent-days | about 135 agent-days |
| Critical path | about 6 agent-days of code, then native runs and the ceremony | about 51 to 70 agent-days | about 47 to 54 agent-days |
| Calendar | 9 to 14 working days from 2026-10-06; ships about 2026-10-19 to 2026-10-23 or 1 to 2 weeks after the soak, whichever is later | 8 to 11 weeks from 2026-10-06: about early to late December 2026 | 7 to 9 weeks after v0.5.1 (4 to 6 with early containment lanes): about late January to late February 2027 |

The 1 to 2 week target after the soak is realistic for the SCL work if it starts on
2026-10-06. What can break it is outside SCL code: the Windows team's S0.7 run (needs a 0.5.0
Windows installer and a machine running 0.4.2), the other HUP gate5 criteria and QA pass
(unchanged by this decision), and the owner's private-route merges. Residuals v0.5.0 ships with:
bugs (a) to (e) stay open (SC1 to SC4 are v0.5.1); the 0.5.0 to 0.5.1 macOS update runs 0.5.0's
exit code; the v0.5.x remainder has no release slot yet under D-41 as amended (owner question).
*Owner decision (2026-10-05, third set):* answered; the remainder is v0.5.2 (D-41 amended again), and
S7.5a left v0.5.0 for v0.5.1. See the third-set section below.

## Owner decision (2026-10-05, third set): v0.5.0, v0.5.1, v0.5.2

D-2 amended a third time and HUP D-41 amended again. This section supersedes "Owner decision
(2026-10-05, second set): three releases" above where they differ; that section stays as the
record. Two changes only: **S7.5a moves from v0.5.0 to v0.5.1** (the owner asked for the minimum
viable 0.5.0; S7.5a is not a safety fix), and the **v0.5.x remainder is now v0.5.2**, the one
v0.5.2 that D-41 now allows. Detail and the re-estimate are in
[05 "Owner decision (2026-10-05, third set)"](05_SPRINTS_AND_WPS.md#owner-decision-2026-10-05-third-set-v050-v051-v052).

| v0.5.0 (minimum viable, after the reroll and 2,000-block soak) | v0.5.1 | v0.5.2 |
|---|---|---|
| SCL-S0: S0.1, S0.3, S0.4, S0.5, S0.6, S0.7 | (none) | S0.2, conditional on a Windows updater feed |
| S8.5a startup barrier and #243 database-lock check | S8.1, S8.3, S8.5b | S8.2, S8.4 |
| S1.6a hosted Windows CI job for the S0.7 hook test | gate0: S1.1 to S1.5, the rest of S1.6, S1.7 | `OwnedContainment.tla` if needed |
| (none; S7.5a moved out) | **S7.5a** (moved from v0.5.0) and S7.5; SC1 to SC4 (S2 to S5); S7.1, S6.4a; all 9 services on the new cell through the O-2 layer | S6.1 to S6.3, S6.4b, S7.2 to S7.4 |
| | Hermes child report minimum: S11.4, S11.5a, S11.6 | S11.1, S11.2, S11.3a, S11.3b, S11.5b, S11.7 |
| | Exit, quit, update: S12.1 to S12.3, S12.5a, S12.6 to S12.8 | S12.4, S12.5b |
| | (no containment) | S9 Unix containment, S10 Windows Job objects |
| | S13 fault suite over every v0.5.0 and v0.5.1 fixture | New fixtures join the same check |
| Native: macOS 0.4.2 to 0.5.0 update on the rerolled chain, DGX Linux manual update, Windows S0.7 run | Native: S14.1 cut scenarios (now including the S7.5a stopped-model check) plus the 0.5.0 to 0.5.1 update, S14.4 macOS slice, S14.5, S14.6 | Full three-OS acceptance |
| Red-team: HUP `g5-redteam`; S0 private route (#298) | S15.2, S15.3 on the v0.5.1 code | S15 on the remainder |

**Known issue v0.5.0 ships with (owner accepted):** today's silent route to the gateway when the
local model is verified, a gateway key is set and the local server process is not running
(05, "O-19 extended"). v0.5.0 release notes should list it and must not claim ask-first;
v0.5.1 fixes it (S7.5a, then S7.5).

| | v0.5.0 | v0.5.1 | v0.5.2 |
|---|---|---|---|
| Work packages | 8 | 48 | 34 |
| Effort | about 16 agent-days | about 153 agent-days | about 135 agent-days |
| Critical path | about 6 agent-days of code (S0.6 then S8.5a), then native runs and the ceremony | about 51 to 70 agent-days at agent pace; SC2 to SC4 and the CI lanes now sit with human contributors | about 47 to 54 agent-days at agent pace; S9, S10 and S11 now sit with human contributors |
| Calendar | 8 to 13 working days from 2026-10-06: about 2026-10-16 to 2026-10-22, or 1 to 2 weeks after the soak, whichever is later | best case early to late December 2026; expected mid-December 2026 to mid-January 2027 (assumptions in 05) | best case 4 to 6 weeks after v0.5.1 (late January to late February 2027); expected 10 to 14 weeks after v0.5.1 (about late March to late April 2027) |

### Owners (owner decision 2026-10-05, third set)

"Mac lane" is the owner's Mac with agents (@SaulBuilds). "Proposed" means the contributor has
not yet accepted on the issue.

| Release | Work | Owner | Issue |
|---|---|---|---|
| v0.5.0 | SCL-S0 build (S0.1, S0.3 to S0.7, S1.6a, S8.5a) | Mac lane (agents) | #249 |
| v0.5.0 | Windows native S0.7 run (and review of the S0.7 and S1.6a PR) | @kurtatwork, with @RDCTart69 | #249 |
| v0.5.1 | Supervisor contract SC2 to SC4 (SCL-S3, S4, S5) and their fault fixtures | @mfarzanansari, **proposed, pending his acceptance** | #247 |
| v0.5.1 | Windows and macOS CI lanes (S1.6), Windows exit paths (Windows halves of S12), Windows Comms ownership | @kurtatwork | #250 |
| v0.5.1 | gate0, SC1 (S2), readiness and O-19 including S7.5a (S7.1, S7.5a, S7.5), S6.4a, S8.1, S8.3, S8.5b, macOS exit coordinator (S12), Hermes child report (S11.4, S11.5a, S11.6), fault suite (S13), macOS acceptance (S14), red-team (S15) | Mac lane | #251 |
| v0.5.2 | Windows Job objects (S10) | @kurtatwork | #252 |
| v0.5.2 | Unix containment (S9), the shared containment crate, Hermes children (S11 remainder) | @mfarzanansari, **proposed, pending his acceptance** | #248 |
| v0.5.2 | Remaining managers (S6.1 to S6.3, S6.4b, S7.2 to S7.4), bespoke processes (S8.2, S8.4), S12.4, S12.5b, remaining S14 and S15 | Mac lane | #253 |

The cross-cutting owner of the lifecycle contract stays the owner (O-16) whatever the lane
split. If a proposed assignment is not accepted, the Mac lane holds that work (recommended
default in 05, pending owner sign-off).

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
   *Red-team correction (2026-10-05, RT-02, proposed O-18):* this holds for **restart and
   installer launch**. Quit and the factory-reset exit proceed at the deadline after a final
   actuation of every OS-process scope still held, and are reported `Incomplete`, never
   `Complete`.
   *Owner decision (2026-10-05):* O-18 accepted (D-15); this correction is the locked rule.
5. **Custody unchanged.** Generation-owned cleanup never deletes a key or credential that
   a newer generation uses, and preserves the #243 keep-list. Nothing in SCL signs.
6. **@rule8 review** for the WPs that change process-spawn-sensitive code (SCL-S8.3, S9,
   S10) and the app updater path and capability grants (SCL-S0.2, S12.2), per core
   CLAUDE.md rule 7.

## Release coupling

D-2 makes the SCL gates part of the v0.5.0 cut. *Owner decision (2026-10-05): D-2 amended;
only the criteria marked `blocks_cut: true` are part of the cut, and `g5-scl` says so.*
*Owner decision (2026-10-05, second set): superseded; v0.5.0 needs only the criteria with
`release: v0.5.0`, `g5-scl` says so, and the `release: v0.5.1` criteria gate v0.5.1 (HUP D-41
amended).* *Owner decision (2026-10-05, third set): the v0.5.0 criteria no longer include
`g3-provider-routing-v050` (now `release: v0.5.1`); the `release: v0.5.2` criteria gate the one
v0.5.2 that D-41, amended again, allows.* The HUP release gates carry one added line
(`g5-scl` in [HUP gates.yaml](../2026-09-30-hermes-upskill/gates.yaml)) that points here.
Nothing else in the HUP planset changes. The SCL program's own gates are in
[gates.yaml](gates.yaml). The 40204 reroll is not gated on SCL.

Two timing facts shape SCL-S0 (from the reconciliation research): the 0.4.x to 0.5.0
update runs **0.4.x's** updater and exit code, so 0.5.0 can only defend at its own startup;
and the 0.5.0 to 0.5.x update runs **0.5.0's** code, so whatever 0.5.0 ships for the update
exit path is what every member's next update uses.

*Red-team correction (2026-10-05, RT-03, RT-04):* both facts hold for the **macOS** in-app
updater only. Windows and Linux bundles ship no updater artifacts in 0.5.0, so members there
update by running the installer or package by hand. On Windows that installer is 0.5.0 code
that runs **before** 0.5.0 starts, so 0.5.0 can also defend at install time (SCL-S0.7).

*Owner decision (2026-10-05, second set):* the second fact now applies to the 0.5.0 to 0.5.1
update. 0.5.0 ships without the exit coordinator, so that macOS update runs today's exit path;
v0.5.1 defends at its own startup with the S8.5a barrier (shipped in 0.5.0) and the narrowed
legacy cleanup, which therefore stays in v0.5.1 (O-17 note above).

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
