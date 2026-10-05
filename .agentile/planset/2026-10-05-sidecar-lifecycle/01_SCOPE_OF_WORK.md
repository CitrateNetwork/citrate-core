---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed)
red_teamed: 2026-10-05 (adversarial pass, 29 findings, 3 blocking; corrections in 08_RED_TEAM.md supersede conflicting text)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (primary); citrate-agent-runtime
companions: 00_OVERVIEW.md, 05_SPRINTS_AND_WPS.md, gates.yaml
---

# Scope of Work

Scoped by work, not time. A phase closes when its gate in [gates.yaml](gates.yaml) is `met`
with an evidence path and a note. It never closes on a date. Story ids SC1 to SC8 are the
contributor's (SPEC-001) and are kept so their traceability survives.

## Phases

| Phase | Name | Outcome | Sprints | SPEC-001 stories |
|---|---|---|---|---|
| P0 | **Pre-cut safety** | The highest-risk lifecycle items are fixed before the v0.5.0 cut with regression tests; details private | SCL-S0 | (none; ours) |
| P1 | **Theory** | The supervisor model is extended, the exit coordinator is modelled, mutant configs reproduce the known bugs, the fault-injection harness exists, the SPEC is re-grounded on the release branch | SCL-S1 | (gate) |
| P2 | **Primitive** | The supervisor has acknowledged controls, generation fencing, attested readiness, one probe permit per service, byte-bounded diagnostics | SCL-S2 to S5 | SC1, SC2, SC3, SC4 |
| P3 | **Owners** | All nine supervised services and all 25 bespoke kinds have one owner each; the startup sweep is replaced by recorded ownership | SCL-S6, S7, S8, S11 | SC5, SC6 (+ ours) |
| P4 | **Containment** | Descendants of owned processes are contained and cleaned on Unix (session anchor) and Windows (Job objects) through one shared implementation | SCL-S9, S10 | SC8 (+ Windows, ours) |
| P5 | **Exit** | Every app exit path goes through the coordinator and drains before the final action; the webview observes cached state | SCL-S12 | SC7 |
| P6 | **Prove it** | Fault-injection suite required in CI; packaged acceptance on three OSes; red-team on merged code clean | SCL-S13, S14, S15 | (gate) |

## Deliverables

- An extended `SidecarSupervisor.tla` and a new `AppExitCoordinator.tla`, TLC-green, each
  with mutant configs that fail as intended.
- A lifecycle cell in `kit/src/supervisor.rs` with intent sequence, generation,
  incarnation and revision; acknowledged receipts; one deadline publisher.
- Readiness separate from liveness, with per-service profiles.
- One probe permit per service, held across timeouts and generations; a literal-only
  loopback health adapter.
- Byte-bounded capture, snapshots and crash sink.
- Migrated managers: node, node agent, memory, IPFS, llama chat, llama embed, Comms,
  Cluster, Hermes, with their command and frontend consumers.
- An ownership registry for core's bespoke spawns with deadlines, and an ownership record
  used for next-launch cleanup in place of the name/path sweep.
- A shared containment crate (Unix owned session with retained anchor; Windows owned Job)
  used by core and the Hermes sidecar.
- The Hermes sidecar's ten child kinds under the same contract.
- An app exit coordinator with native Quit, Restart, Update and Reset commands; the raw
  `process:default` and `updater:default` capabilities removed.
- A standing fault-injection suite that is a required CI check, with a manifest tripwire.
- Packaged acceptance records for macOS arm64, Linux x64 and Windows x64.
- A red-team report (private) and a public corrections section.

## In and out

**In:** everything in D-1 to D-14 and the scope list in [00_OVERVIEW](00_OVERVIEW.md#scope).

**Out:** see [00_OVERVIEW](00_OVERVIEW.md#scope). In addition, these SPEC-001 "parked"
items stay parked: durable operation replay, group-message retention, and a full total-RSS
ceiling. Windows Job containment is **not** parked here (D-10), which is a deliberate
change from SPEC-001.

## What needs native evidence before a gate flips

These claims cannot be proven by unit tests or by a Linux or macOS developer machine alone.
The gate criterion stays `met: false` until the named run is recorded.

| Claim | Who runs it | Where recorded |
|---|---|---|
| Windows Job capture, no breakaway, kill-on-close, `ActiveProcesses == 0` completion | Windows team (@RDCTart69, @kurtatwork) | SCL-S10 EVIDENCE, `g4-windows` |
| Windows update install drains every owner before the installer runs | Windows team | SCL-S0 and S12 EVIDENCE, `g1-precut`, `g3-exit-paths` |
| *Red-team correction (2026-10-05, RT-03, RT-04):* the row above is not runnable in 0.5.0 (no Windows updater artifacts). It is replaced by: a manual 0.4.2 to 0.5.0 NSIS install with the old app and Hermes running stops this installation's own sidecars before files are copied, and no old sidecar is left running | Windows team | SCL-S0.7 EVIDENCE, `g1-precut`, `g3-exit-paths` |
| Windows Comms foreground slots release only after real I/O completion | Windows team | SCL-S7 EVIDENCE |
| Linux owned-session containment (leader-first exit, no post-reap actuation, absence observation) on the packaged AppImage and `.deb` | DGX team | SCL-S9 and S14 EVIDENCE, `g4-unix`, `g4-native-linux` |
| Pre-cut safety checks on each OS's installed build (wording genericized by red-team correction RT-23) | owner Mac, DGX team, Windows team | private #298, then `g1-precut` note |
| macOS owned-session containment and update-moved bundle paths | Owner Mac | SCL-S9 and S14 EVIDENCE, `g4-native-macos` |
| Diagnostic and thread budgets under noisy output and cold model load | each OS above | SCL-S14 EVIDENCE, `g3-diagnostics` |
| `fork+exec` cold-launch cost of the `pre_exec` setup on the real app | macOS and Linux | SCL-S9 EVIDENCE |

## Risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-1 | The program is large and gates the 0.5.0 cut (D-2) | High | High | Scoped by work; SCL-S0 lands first so the most important fixes do not wait for the rest; sprints are ordered so P2 unblocks parallel lanes (S6, S7, S8, S9, S10) |
| R-2 | The DTO change touches every manager command and `store.ts`; regressions in unrelated UI | High | Med | Compatibility layer (O-2); consumers migrated per manager group with the fault suite and the existing vitest suite green at every PR |
| R-3 | Changing the formally modelled retry reset (F-1) breaks the "recovering daemon never permanently fails" property | Med | High | Extend `SidecarSupervisor.tla` first (SCL-S1); keep INV-1 for liveness-only profiles and add an attested variant for probed ones; negative controls for both |
| R-4 | Nested ownership: the Hermes sidecar starts children in their own sessions or groups, so a group signal to Hermes does not reach them | High | High | The runtime keeps its own anchors for its sub-groups and reports them; core's Hermes stop waits for the runtime's acknowledgement; on Windows, nested Jobs inside the Hermes Job die with it; on Unix, runtime ownership records feed next-launch cleanup (02 §6) |
| R-5 | `pre_exec` forces `fork+exec` instead of `posix_spawn` in a multithreaded host: latency, memory, resource-limit failures | Med | Med | Async-signal-safe callback only (`setsid`), measured on packaged builds before enablement (SCL-S9.3) |
| R-6 | Another reaper in the process (a library calling `waitpid(-1)`, `SIGCHLD = SIG_IGN`) breaks the retained-anchor assumption | Low | High | Sole-reaper check at startup; a profile is unsupported (and its gate criterion open) if the assumption fails; fixture that installs a competing reaper |
| R-7 | Core itself runs inside a Job it did not create (some launchers and terminals), affecting nested Jobs or breakaway | Med | Med | Nested Jobs are supported on Windows 8 and later; detect and report; fixture under a pre-existing Job (SCL-S10.3) |
| R-8 | Recorded-ownership matching misses processes running from an updater-moved or deleted bundle path | Med | Med | Match on pid plus start time first, path second, with per-OS handling of moved and deleted paths, verified natively (SCL-S8.3) |
| R-9 | The updater migration (D-8) is an @rule8 surface; a mistake can break updates for every member | Med | High | @rule8 review; keep plugin signature and feed policy unchanged; Windows install and macOS/Linux restart tested natively; critical-update path tested |
| R-10 | Quarantine without auto-restart reduces availability | Med | Med | Deliberate (SPEC-001); visible reason and a one-click user restart (O-3); budgets measured |
| R-11 | Fault-injection tests are flaky (timing, PID reuse on CI runners) | Med | Med | Owned fixtures with marker-checked guards, monotonic timing with generous bounds, no fixed sleeps as oracles; flake budget tracked; a flaky fixture is fixed, never deleted (manifest tripwire) |
| R-12 | Cross-repo drift between core's kit and the runtime's use of the shared crate | Med | Med | One crate, `[[drift]]` entry, pin-bump through the manifest (Rule 11, 12) |
| R-13 | Old clients (0.4.x) perform the 0.4.x to 0.5.0 update with their own exit code | High | Med | 0.5.0 defends at its own startup. Recorded-ownership cleanup cannot see 0.4.x processes (0.4.x wrote no records), so 0.5.0 keeps the narrowed exact-path cleanup from SCL-S0.1 as a one-release legacy path (O-17) and SCL-S0.6 adds a startup check that names a leftover old node in the UI |

*Red-team corrections to this register (2026-10-05, 08_RED_TEAM):*

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-1 (amended) | Program size: about 280 agent-days total, critical path about 70 to 92 agent-days, 10 to 14 calendar weeks with parallel lanes (RT-15) | High | High | Corrected dependency graph (RT-13); two native-run windows (RT-25); O-20 offers a split of cut-blocking from 0.5.0-line gates |
| R-14 | macOS terminations that cannot be prevented (Dock Quit, logout) bypass an async-only coordinator (RT-01) | High | High | Synchronous bounded drain in `Exit` plus a custom Quit menu item (SCL-S12.6) |
| R-15 | Quit blocked forever by a stuck in-process worker (RT-02) | Med | High | O-18 (SCL-S12.7) |
| R-16 | Readiness changes silently move chat from the local model to the remote gateway (RT-12) | High | High | O-19 (SCL-S7.5) |
| R-17 | Runtime ownership records are trusted as authority to signal (RT-09) | Med | Med | Records are claims; core ties them to its own recorded Hermes incarnation (SCL-S11.7, @rule8) |
| R-18 | Windows Job capture on stable Rust needs a mechanism `std` does not expose (RT-05) | High | Med | SCL-S10.0 spike and ADR addendum before S10.1 |
| R-19 | Hosted CI has no Windows or macOS lane today, so "Windows CI" acceptance has nowhere to run (RT-14) | High | Med | SCL-S1.6 before S2 |
