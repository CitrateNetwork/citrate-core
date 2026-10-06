---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: proposed (Stage-2, red-teamed; owner decisions O-18, O-19, O-20 and the red-team pass accepted 2026-10-05, then a second set the same day (release path v0.5.0 / v0.5.1 / v0.5.x, D-16 extended, HUP D-41 amended); ADR acceptance (g0-adr) still pending; changes nothing until accepted)
updated: 2026-10-05 (owner decisions: D-2 amended to a cut-blocking subset; O-18, O-19 locked; red-team pass accepted; second set: D-2 amended again, D-16 extended, HUP D-41 amended for one v0.5.1)
red_teamed: 2026-10-05 (adversarial pass, 29 findings, 3 blocking; corrections in 08_RED_TEAM.md supersede conflicting text)
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

*Red-team correction (2026-10-05, RT-21):* "are merged to core `main`" is not yet true. PR #241
was open and unmerged at head `01772a3` on 2026-10-05. Read it as "will be merged"; until then
the links resolve only in #241, and `g0-adr` cannot flip.

## Status

**Proposed, 2026-10-05.** The owner has decided D-1 to D-14 of the planset (scope, release,
home, base design and the deltas below). Acceptance of this ADR is gate criterion `g0-adr`.
Owner decisions O-1 to O-17 in the planset are recommended defaults, pending owner sign-off.

*Owner decision (2026-10-05):* the owner accepted O-20 (D-2 amended: the v0.5.0 cut is gated
on a cut-blocking subset), O-18 (locked as D-15) and O-19 (locked as D-16), and accepted the
single-model red-team pass as the pre-Stage-2 red-team (D-17, `g0-redteam` met). O-1 to O-17
remain recommended defaults pending sign-off. The ADR as a whole is still proposed until
`g0-adr` flips. See "Owner decisions (2026-10-05)" below.

*Owner decision (2026-10-05, second set):* D-2 amended again (v0.5.0 carries only SCL-S0 and
the S8.5 startup barrier; the previous cut-blocking subset gates v0.5.1), D-16 extended (ask
first when the local model is down), and the HUP D-41 cadence amended to allow one v0.5.1.
`g0-adr` is now a v0.5.1 criterion. See "Owner decisions (2026-10-05, second set)" below.

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
   *Owner decision (2026-10-05, O-20 accepted, D-2 amended):* superseded. The v0.5.0 cut is
   gated on the SCL criteria marked `blocks_cut: true`; the rest of SC1 to SC8 and the deltas
   finish on the 0.5.x line under the same planset, still gated (`blocks_cut: false`). The
   HUP `g5-scl` line points at the cut-blocking criteria.
   *Owner decision (2026-10-05, second set, D-2 amended again):* superseded in turn. v0.5.0
   ships after the 40204 reroll and its 2,000-block soak with only the SCL criteria tagged
   `release: v0.5.0` (SCL-S0, S8.5a, and the minimum they need, plus S7.5a). The previous
   cut-blocking subset is the v0.5.1 gate (`release: v0.5.1`); the rest is `release: v0.5.x`.
   The HUP `g5-scl` line points at the `release: v0.5.0` criteria.

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
| Δ-8 | Release | No release approval implied | Ships in v0.5.0; SCL gates gate the cut. *Owner decision (2026-10-05): amended; only the cut-blocking subset gates the cut, the rest ships on 0.5.x.* *Owner decision (2026-10-05, second set): amended again; v0.5.0 carries SCL-S0 and S8.5a, v0.5.1 carries the previous cut-blocking subset, the rest is 0.5.x* | Owner decision 2026-10-05 |
| Δ-9 | Keeping it fixed | Future acceptance tests | A **standing fault-injection suite** as a required CI check on three OSes, a manifest tripwire so fixtures cannot be dropped without an ADR, TLA+ mutant configs that must reproduce bugs (a) to (c), a named cross-cutting owner, and a red-team pass on merged code | The prior model and tests were green while all five defects were present |
| Δ-10 | Formal model | Not specified | Extend `SidecarSupervisor.tla` (intent and generation fencing, attested retry credit, probe permit) and add `AppExitCoordinator.tla` | Rule 9: extend, do not duplicate |
| Δ-11 | Thread budget | 35 (43 with Windows Comms slots) | 38 (46) for 9 cells, plus a bounded bespoke reader pool; ceilings to measure (O-5) | Embed counted as its own owner |
| Δ-12 | Validation split for another tool's ADR schema | Proposed as policy | Declined as federation policy (O-1, pending sign-off); Rule-5 frontmatter only | One validation contract for our docs |
| Δ-13 | Red-team | Contributor's three internal rounds | Inputs only; our own second-model pass before Stage-2 and on merged code (O-6) | Independence |

*Red-team corrections to this table (2026-10-05, see the planset's 08_RED_TEAM, which governs
where it conflicts):*

- **Δ-2 (RT-03, RT-05, RT-06):** the rationale "the Windows update path is where orphans
  matter most" does not hold for 0.5.0: the Windows bundle ships no updater artifacts, so
  Windows members install by hand, and the NSIS installer is where 0.5.0 can act (new S0.7).
  Windows Job capture on the stable toolchain also needs a mechanism `std` does not expose
  (S10.0).
- **Δ-5 (RT-07, RT-08, RT-09):** nested ownership needs a runtime Closing gate before the
  child report, a leader-first stop for the nested owner, and core must treat runtime records
  as claims, not authority.
- **Δ-6 (RT-26):** Hermes-first drain keeps a sub-deadline so a slow Hermes cannot consume the
  whole close deadline.
- **D-8 / SPEC-001 shutdown (RT-01, RT-02):** the coordinator also needs a synchronous drain
  mode for unpreventable macOS terminations, and Quit must always end (proposed O-18;
  *accepted by the owner 2026-10-05 as D-15*).
- **SC6 routing (RT-12):** fresh `Ready` must not move chat to the remote gateway by itself
  (proposed O-19; *accepted by the owner 2026-10-05 as D-16*).
- **Δ-13:** the pass recorded in 08 ran in a fresh context on the same model family that wrote
  Stage-1; the owner decides whether it satisfies "second model" (`g0-redteam`).
  *Owner decision (2026-10-05):* accepted as sufficient (D-17); `g0-redteam` is met.

Unchanged from SPEC-001 and adopted as written: no Tokio migration; no generic durable
operation registry; no new process-management crate; std monitor as sole process writer;
retry credit attested for probed services and liveness-only for unprobed ones (the owner
locked exactly this split); the status DTO with `stopping` and `quarantined` and decimal
string counters; the diagnostic budgets as starting values to be measured; removal of the
raw `process:default` and `updater:default` capabilities in favor of native coordinated
commands; force exit and crashes are never reported as `Complete`.

## Owner decisions (2026-10-05)

Recorded as locked decisions in the planset's
[00_OVERVIEW](../../.agentile/planset/2026-10-05-sidecar-lifecycle/00_OVERVIEW.md#locked-decisions).

| # | Decision | Replaces |
|---|---|---|
| D-2 (amended) | **Split the gates (O-20).** The v0.5.0 cut is gated on a cut-blocking subset: SCL-S0; the gate0 theory items it needs; SC1 to SC4 (S2 to S5); the exit, quit and update correctness for 0.5.0 (synchronous macOS quit fallback, Quit always ends, macOS updater coordination with `UpdateStaged`, raw process and updater capabilities removed, the Windows NSIS pre-install stop); the #243 startup barrier and database-lock check; recorded-ownership cleanup for core-spawned processes; provider routing and llama readiness; the standing fault suite for the subset; red-team of the merged subset; packaged macOS acceptance and a Linux smoke. The rest (remaining manager migrations, the Hermes-sidecar children beyond a minimum child report, shared containment and Unix group containment, Windows Job objects, full three-OS acceptance) finishes on 0.5.x under the same planset. Table and estimate: planset 00 and 05 | Decision 3 and Δ-8 as first written |
| D-15 | **Quit always ends (O-18).** Install and restart require `Complete`. Quit force-stops what it still owns and exits, reporting `Incomplete` | Safety gate 4 as first written; X-2, X-5 for Quit |
| D-16 | **No silent gateway fallback (O-19).** While the local model is cold-loading or its probe times out, chat waits or tells the member; it never routes the prompt to the remote gateway without the member's explicit choice. Blocks the cut | Implicit `LocalFallback` on readiness |
| D-17 | **Red-team gate.** The single-model red-team pass (planset 08, commit `2db312f`) is accepted as the pre-Stage-2 red-team | Δ-13's "second model" condition for `g0-redteam` |

Estimate for the cut-blocking subset: about 165 agent-days of effort, a critical path of about
51 to 70 agent-days, 7 to 10 calendar weeks with four to five lanes (cut about late November
to mid-December 2026). The 0.5.x remainder is about 135 agent-days.
*Owner decision (2026-10-05, second set): superseded by the estimate below.*

## Owner decisions (2026-10-05, second set)

Recorded as locked decisions in the planset's 00 (and D-41 in the HUP planset). They supersede
the rows above where they differ; the rows above stay as the record.

| # | Decision | Replaces |
|---|---|---|
| D-2 (amended again) | **Three releases.** v0.5.0 ships right after the 40204 reroll and its 2,000-block soak with only SCL-S0 pre-cut safety (S0.1, S0.3 to S0.7, including the Windows NSIS pre-install stop) and S8.5a (startup cleanup barrier before node admission, #243 database-lock check), plus S1.6a (the hosted Windows CI job S0.7's test needs; the only strict dependency) and S7.5a (below). The previous cut-blocking subset is the **v0.5.1** gate. Everything else is 0.5.x follow-up. gates.yaml tags every criterion `release: v0.5.0`, `v0.5.1` or `v0.5.x` in place of `blocks_cut` | D-2 (amended) above |
| D-16 (extended) | **Ask first, never silent.** In addition to D-16: when the local model is `Failed`, `Stopped` or `Quarantined`, chat asks the member first (restart the local model, or send this message to the gateway this time), per message. Today's code routes silently to the gateway when the local server process is not running, so a minimal ask-first on today's signals ships in v0.5.0 (S7.5a); the full rule on the readiness model ships in v0.5.1 (S7.5) | D-16 above, which named only cold load and probe timeout |
| D-41 (HUP, amended) | **One v0.5.1.** The HUP cadence allows exactly one v0.5.1 after v0.5.0, carrying the SCL v0.5.1 gate | D-41 as amended 2026-10-01 |

Estimate: v0.5.0 is 9 WPs, about 19 agent-days (16 without S7.5a), 9 to 14 working days from
2026-10-06, so about 1 to 2 weeks after the soak if the other HUP gate5 criteria and the
Windows team's S0.7 run are ready. v0.5.1 is 47 WPs, about 150 agent-days, critical path about
51 to 70 agent-days, about early to late December 2026. The remainder is about 135 agent-days.
Consequences: bugs (a) to (e) ship open in v0.5.0; the 0.5.0 to 0.5.1 macOS update runs
0.5.0's exit code; the O-17 legacy cleanup stays through v0.5.1; the remainder has no release
slot yet under D-41 as amended.

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
- v0.5.0 waits for the SCL gates. *Owner decision (2026-10-05): v0.5.0 waits for the
  cut-blocking SCL criteria only; the rest ships on 0.5.x. Bug (e) stays open for descendants
  of supervised services at the cut, until shared containment lands.*
  *Owner decision (2026-10-05, second set): v0.5.0 waits only for SCL-S0, S8.5a, S1.6a and
  S7.5a; v0.5.1 waits for the previous cut-blocking subset. All five reproduced defects ship
  open in v0.5.0.*

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
