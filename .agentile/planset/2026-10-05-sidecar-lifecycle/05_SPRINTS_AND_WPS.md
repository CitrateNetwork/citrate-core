---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-1 draft)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core + citrate-agent-runtime (via federation sprint)
companions: 04_FEATURES_BDD.md, gates.yaml
---

# Sprints and Work Packages

Effort: S (1 day or less), M (2 to 4 days), L (1 to 2 weeks), XL (more than 2 weeks; split
before starting). Each WP goes red, then green, then close-with-proof (agentile red-green):
the failing test is written first, in the same PR as the fix. Repo tags: **core** =
citrate-core, **kit** = core's `kit/` crate, **rt** = citrate-agent-runtime, **fed** =
citrate-federation. Work outside citrate-core is tracked in the federation sprint
`citrate-federation/.agentile/sprints/active/2026-10-05-scl-sidecar-lifecycle.md`
(created in SCL-S9.0, before the first runtime PR).

Branches: `scl/s<N>-<slug>`, each WP a PR into `release/0.5.0-hermes-upskill`. Private
items land through the private remediation route in #298 and are merged by the owner.

"Data source" in an acceptance criterion names where the proof comes from (Rule 7).

## Sprint list

| Sprint | Goal (one line) | SPEC-001 | Effort |
|---|---|---|---|
| SCL-S0 | Pre-cut safety fixes with regression tests, before the v0.5.0 cut | (ours) | M (6 WPs: 4 S, 2 M) |
| SCL-S1 | Theory: extended supervisor model, exit coordinator model, fault harness, re-grounding | (gate) | L |
| SCL-S2 | Acknowledged controls, intent sequence, generation fencing, receipts | SC1 | L |
| SCL-S3 | Readiness separate from liveness; attested retry credit | SC2 | M |
| SCL-S4 | One probe permit per service; literal-only loopback adapter | SC3 | M |
| SCL-S5 | Byte-bounded capture, snapshots and crash sink | SC4 | M |
| SCL-S6 | Migrate node, node agent, memory, IPFS owners and consumers | SC5 | L |
| SCL-S7 | Migrate llama chat, llama embed, Comms, Cluster, Hermes owners and consumers | SC6 | L |
| SCL-S8 | Own core's bespoke spawns; replace the sweep with recorded ownership | (ours) | L |
| SCL-S9 | Shared containment crate, Unix backend (retained anchor) | SC8 | L |
| SCL-S10 | Windows Job object backend in the shared crate | (ours; parked in SPEC-001) | L |
| SCL-S11 | Hermes-sidecar children under the same contract | (ours) | XL, split in 5 WPs |
| SCL-S12 | App exit coordinator, capability and updater migration, frontend observer | SC7 | L |
| SCL-S13 | Standing lifecycle fault-injection suite as a required CI check | (ours) | M |
| SCL-S14 | Packaged native acceptance on macOS, Linux, Windows | (gate) | L |
| SCL-S15 | Red-team: planset before Stage-2, merged code before the cut | (gate) | L |

## Dependency table

| Sprint | Depends on | Can run in parallel with |
|---|---|---|
| SCL-S0 | (none; starts now) | S1 |
| SCL-S1 | (none) | S0 |
| SCL-S2 | S1 (gate0) | |
| SCL-S3 | S2 | |
| SCL-S4 | S3 | |
| SCL-S5 | S4 (shared file `kit/src/supervisor.rs`; owner may allow S5 earlier, SPEC-001 notes no semantic dependency) | |
| SCL-S6 | S5 | S7, S8, S9, S10 |
| SCL-S7 | S5; S7.2 also re-grounds on #180 | S6, S8, S9, S10 |
| SCL-S8 | S2 (receipt types); S8.3 also S9.1 and S10.1 (group and Job identity); S8.4 also S9 and S10 | S6, S7 |
| SCL-S9 | S5; S9.0 `[[drift]]` entry and federation sprint file first | S10 |
| SCL-S10 | S9.0 (crate scaffold) | S9 |
| SCL-S11 | S9, S10, S7.3 | S12 (until S12.5) |
| SCL-S12 | S6, S7, S8; S12.5 also S11 | |
| SCL-S13 | S1.3 (harness), grows with every sprint; S13.1 to S13.4 close after S11 and S12 | |
| SCL-S14 | S9, S10, S11, S12, S13 | |
| SCL-S15 | S15.1 after S1; S15.2 after S14; S15.3 after S15.2 | |

v0.5.0 is cut only when every SCL gate is met (D-2). SCL-S0 lands before the cut whatever
the state of the rest.

---

## SCL-S0: Pre-cut safety (details private, #298)

Sprint file: `.agentile/sprints/active/sprint-scl-s0-precut-safety/SCOPE.md`. Specifics of
S0.1, S0.3 and S0.4 are private; the public text here is deliberately generic.

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S0.1 | Narrow the startup orphan cleanup: match only the exact executable paths of this bundle's own sidecar binaries, never a directory substring; keep it as the one-release legacy path (O-17) | core | S | Red test: a decoy process whose command line contains the app directory survives; a real owned orphan is cleaned (`src-tauri` unit + fixture tests). Native check on each OS's installed package (private note on #298) |
| S0.2 | Windows update install stops every sidecar before the installer runs: wire the updater's before-exit hook (verified at the pinned tauri-plugin-updater 2.10.1) to the bounded shutdown path. Superseded by S12.2 | core | S | Unit test that the hook is installed and calls the shutdown path; Windows native run by the Windows team: after install, no sidecar from the old version is running (task list before and after, EVIDENCE) |
| S0.3 | Managed-browser cleanup on Hermes stop and at the next Hermes start or app launch (pre-cut safety fix tracked privately) | rt, core | M | Fixture: hard-kill the sidecar with a managed browser running; after the next start, no browser process from the earlier run and no leftover profile directory (runtime fixture + core test) |
| S0.4 | Runtime-side containment correction (pre-cut safety fix tracked privately) | rt | S | Red test in the runtime crate named on #298; green on Linux and macOS CI |
| S0.5 | Shutdown coverage check: embed stops with Hermes; runtime workers exit on stdin close; record per OS | core, rt | S | QA record in S0 EVIDENCE (process list after quit on macOS, Linux, Windows) |
| S0.6 | Genesis-change update check per OS: after a 0.4.x to 0.5.0 update, no old node holds the chain DB lock or ports; if one does, a startup check names it in the UI ("An older Citrate node is still running; quit it or restart") instead of a silent wedge | core | M | Packaged update run per OS (EVIDENCE); unit test for the startup check using an owned fixture listener on the RPC port |

## SCL-S1: Theory (gate0)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S1.1 | Extend `SidecarSupervisor.tla` per 03 §1, with configs and the three mutant configs; decide on `OwnedContainment.tla` | core | M | TLC: green configs report no error, each mutant reports its named invariant violated (`scripts/run-tlc.sh` output in EVIDENCE) |
| S1.2 | Write `AppExitCoordinator.tla` per 03 §2 with configs and mutants | core | M | TLC as above |
| S1.3 | Fault-injection harness: owned-process fixture helpers with marker-checked guards (kill and reap every recorded pid even on failure), monotonic timing helpers, a suite manifest file, a CI job stub; positive controls green | kit, core | M | `cargo test -p citrate-core-kit --test lifecycle_faults` green with positive controls; no leftover marker processes after the run (`pgrep` check in the job) |
| S1.4 | Re-ground SPEC-001's line references, the "61 manager commands" inventory and the SC5 to SC7 touch lists on `release/0.5.0-hermes-upskill`; produce the command-to-consumer map | core | M | `.agentile/sprints/active/sprint-scl-s1-theory/REGROUND.md` with commit pin |
| S1.5 | Baseline: `cargo test --workspace --locked` count, vitest count, kit supervisor tests (22 + 3 Windows), TLC state counts | core | S | `07`-style baseline section in S1 EVIDENCE |

## SCL-S2: Acknowledged controls (SC1)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S2.1 | Identity counters, latest-intent cell, wake bit, bounded descriptor (64 KiB) | kit | M | Unit tests on ordering and coalescing; counters serialize as decimal strings |
| S2.2 | Stop wins in every phase; repeated Stop joins the same receipt (bug a) | kit | M | Red first: `stop_in_off_never_respawns`, `repeated_stop_joins_receipt` (fault suite, fixed pid count over 2 s); INV-5, INV-6 TLC re-run |
| S2.3 | Spawn and publication tickets; a late spawn return after Stop goes straight to teardown; stale completions inert | kit | M | Fault fixtures with a delayed spawn barrier; INV-12, INV-14 |
| S2.4 | Receipts and the deadline publisher (one std thread, 10 records); `ObservationUnknown` on wake failure; native-only receipt reads | kit | L | Tests from SPEC-001 "GATE" essential proofs: atomic arming, supersession, expiry, delayed delivery, publisher creation failure leaves admission unavailable |

## SCL-S3: Readiness and retry credit (SC2)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S3.1 | Readiness field and per-service profiles (02 §3) | kit | M | Unit tests; liveness-only profiles report `NotAssessed` |
| S3.2 | Attested retry credit for probed profiles; grace never credits; monotonic clock only (bug b) | kit | M | Red first: grace longer than `healthy_after` with an always-false probe reaches `Failed` after `max_retries` (fault suite, count of distinct child pids); positive control: sustained fresh success resets; INV-7, INV-8 |
| S3.3 | Split counters (initial failures, attempts, respawns, budget); keep `restarts` as a documented legacy counter | kit | S | Unit tests; INV-1, INV-2, INV-9 re-run |

## SCL-S4: Bounded probes (SC3)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S4.1 | One probe permit per service across generations; timeout never frees; never-ending worker quarantines (bug c) | kit | M | Red first: a blocked probe held through 50 intervals and 3 replacements, at most 1 probe thread alive (thread count from the fixture); release frees the slot; INV-10, INV-11, INV-15 |
| S4.2 | Literal-only resolver and `TcpConnector` adapter; `ureq = "=3.3.0"` in kit and src-tauri; supply-chain review | kit, core | M | Adapter tests: no resolver thread spawned (thread count), wrong authority rejected, no proxy, no redirect, 8 KiB header limit; supply-chain review record |
| S4.3 | Per-platform IPC connect probe adapters (UDS, named pipe) with owned deadlines | kit, core | S | Unix and Windows CI tests |

## SCL-S5: Bounded diagnostics (SC4)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S5.1 | Streaming byte-bounded capture before allocation (bug d) | kit | M | Red first: a 1 MiB single line retains at most 16 KiB plus marker; 4 x 4 MiB lines keep the ring at or under 256 KiB (fault suite, ring byte count); no-newline, invalid UTF-8, alternating streams |
| S5.2 | Snapshot admission (2 app-wide), crash sink (64 x 8 KiB), rotation (2 x 16 MiB) | kit | M | Tests for slow consumer, full disk (owned temp volume or injected writer), blocked sink; counters visible |
| S5.3 | Contributor engagement point (O-8): SC4 offered to @mfarzanansari after gate0 | (process) | S | Invitation comment on #240 or #241 linked in S5 EVIDENCE |

## SCL-S6: Owners, group A (SC5)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S6.1 | Node owner: lifecycle cell, liveness-only profile, preserve the activation-height environment and storage key wiring, #243 reset runs only after the node owner reports `Complete`, watchdog consumes terminal teardown | core | L | Owned-fixture tests; #243 reset tests green; watchdog never restarts after a Stop error |
| S6.2 | Node agent and memory owners; generation-owned bearer and token files | core | M | Tests with a held cleanup barrier: an old generation's cleanup never removes the new generation's file |
| S6.3 | IPFS owner; `init` and `config` become owned bounded preflights; port-free check for the API and swarm ports | core | M | Tests: a stuck preflight owns the slot and a second cannot start; port-in-use is reported, not retried silently |
| S6.4 | Consumer migration for group A commands and `store.ts` mapping (including `mapNodeState`'s unknown-to-off fallback) | core | M | vitest + Rust command tests per command in the S1.4 map |

## SCL-S7: Owners, group B (SC6)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S7.1 | Llama chat and llama embed owners; fresh `Ready` gates the four local inference paths and embedding calls; 180 s grace earns no credit | core | L | Fixture llama server with a slow `/health`: requests wait or fail honestly while `Awaiting`; vitest for UI state |
| S7.2 | Comms and Cluster owners; generation-fenced seed and bearer files; Windows Comms foreground slots re-grounded on #180 (O-14) | core | L | Unix tests + Windows CI tests; Windows native slot release run (Windows team) |
| S7.3 | Hermes owner: control-plane readiness, per-generation bearer cleanup, embed as its own lifecycle cell owned through the Hermes manager | core | M | Tests; bearer file absent after a clean stop |
| S7.4 | Consumer migration for group B commands and the UI | core | M | vitest + Rust command tests per the S1.4 map |

## SCL-S8: Bespoke ownership and recorded cleanup (core 12, node 3)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S8.1 | Ownership registry (handle, deadline, capture, receipt); lint tripwire: no `Command::new` in `src-tauri/src` or `kit/src` outside the supervisor and the registry (allowlist file) | kit, core | M | Tripwire test fails on a planted `Command::new`; registry unit tests |
| S8.2 | Migrate the core bespoke kinds (07 §B): bounded deadlines for every `.output()`; the corpus import owned (it holds the memory store lock); the fork dry-run owned as a contained group | core | L | One test per kind: deadline enforced with a slow fixture binary; owner reports `Complete` |
| S8.3 | Ownership record and next-launch cleanup (02 §7); remove `sweep_orphan_sidecars` and its helper processes, keeping only the O-17 legacy path for 0.5.0; @rule8 review | kit, core | L | Fixture: record a process, kill core's owner abruptly, relaunch: the recorded process is cleaned; a pid-reuse fixture (same pid, different start time) is **not** signalled; a same-name process at another path is not signalled; native runs for update-moved paths (macOS) and deleted paths (Linux) |
| S8.4 | Node descendants contained by the node's profile; verify no escape on each OS | core | M | Native fixture with the real node binary compiling via its RPC: after node Stop, no compiler or engine child remains (process list, EVIDENCE); an escape becomes a federation item |

## SCL-S9: Shared containment, Unix (SC8)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S9.0 | `[[drift]]` entry in `citrate-federation/manifest.toml` (consumer citrate-core, dep_repo citrate-agent-runtime, file `kit/Cargo.toml`), federation sprint file, crate scaffold (name per O-9) | fed, rt | S | drift-check green; sprint file merged |
| S9.1 | Owned session via async-signal-safe `pre_exec` `setsid`; recorded SID/PGID; sole-reaper check; typed zeroed `waitid` WNOWAIT polling | rt | L | Linux and macOS tests: running-child polling, leader-first exit keeps the anchor, competing-reaper fixture makes the profile unsupported |
| S9.2 | Group TERM, graceful window, one group KILL before reap; close actuation before reap; signal-zero absence only; `Incomplete` paths (bug e) | rt | M | Red first: `sh -c 'sleep & sleep & wait'` descendants are gone after Stop (fault suite, marker pids); no signal after reap (syscall trace or wrapper counter); escape fixture reported as excluded |
| S9.3 | Native fixtures and measurement: inherited pipes, concurrent Stop, permission errors; `fork+exec` launch cost | rt, core | M | EVIDENCE with timings on macOS and Linux |
| S9.4 | Core kit adoption: every supervised profile spawns through the crate after its compatibility test (O-10) | kit | M | Per-profile compatibility test results; profiles that fail listed in gates.yaml note |

## SCL-S10: Windows Job objects

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S10.1 | Job backend: `CREATE_SUSPENDED`, assign, resume; kill-on-job-close; no breakaway; `windows-sys` features only | rt | L | Windows CI: a child that spawns a grandchild has both in the Job (accounting count); breakaway attempt fails |
| S10.2 | Completion by `ActiveProcesses == 0` inside the deadline; completion-port message only as a wake | rt | M | Windows CI: Stop with a stubborn grandchild ends with `Complete` after terminate; a held handle case ends `Incomplete` |
| S10.3 | Nested and pre-existing Job compatibility; detect and report | rt | M | Windows CI run under a parent Job |
| S10.4 | Native acceptance on a packaged build by the Windows team (@RDCTart69, @kurtatwork): quit, update install, core crash (kill-on-close) | core | M | EVIDENCE with task-list snapshots before and after |

## SCL-S11: Hermes-sidecar children (runtime)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S11.1 | Workers and toolchain runs spawn through the shared crate; worker session cleanup uses the retained anchor | rt | L | Runtime fault fixtures: worker hard-kill with a toolchain run in flight; no descendant left (Linux, macOS, Windows CI) |
| S11.2 | Managed browser and search service owned and contained, including their helper processes; profile and data directories removed on `Complete` | rt | L | Fixtures: after sidecar Stop and after sidecar hard kill plus restart, no browser or search process from the earlier run |
| S11.3 | `shell_run`, MCP stdio servers (including wrapper launchers), MCP probe, `git`, GPU sampler owned with deadlines; `shutdown_children` covers in-flight work | rt | L | One fixture per kind; wrapper launcher fixture: the wrapped server ends with the wrapper's owner |
| S11.4 | Runtime child-ownership status field (O-13) and ownership records core can read; drain fits inside core's close deadline (O-11); bounded shutdown of blocking turns | rt | M | Control API test; timing test of drain under a busy turn |
| S11.5 | Core side: Hermes receipt includes the child report; next Hermes start and app launch apply runtime records | core | M | Core tests with a fixture sidecar that reports `Incomplete` |

## SCL-S12: App exit coordinator (SC7)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S12.1 | Coordinator: Closing gate, Hermes-first then concurrent drains (O-11), one absolute deadline, native coordinated quit and restart; `ExitRequested` prevented and scheduled, never blocking the main thread | core | L | Tests from SPEC-001 SC7 list (held async workers, saturated blocking pool, stalled owner, repeat exit, late tickets); X-1 to X-9 TLC re-run |
| S12.2 | Remove `process:default` and `updater:default`; native update check, download and install commands; migrate `updater.ts` including the critical path; Windows install only after `Complete`; @rule8 review | core | L | Capability test: raw exit, restart and install are not invocable from the webview; Windows native install run; macOS and Linux restart run; supersedes S0.2 |
| S12.3 | Factory reset and the #243 reset route through owners' `Complete` | core | S | Tests; `local_data_delete` no longer exits on a timer |
| S12.4 | Frontend observer: one raw request in flight, epochs, numeric revisions, StrictMode; replace the "Supervised restarts" label source | core | M | vitest: StrictMode double setup does not duplicate starts or requests; BigInt revision ordering |
| S12.5 | Exit-path coverage matrix: every path in 07's exit-path table drains every owner, including Hermes children | core, rt | M | One test or recorded native run per path (`g3-exit-paths` evidence table) |

## SCL-S13: Standing lifecycle fault-injection suite

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S13.1 | Required CI job `lifecycle-faults` on ubuntu, macos and windows hosted runners (kit, src-tauri and runtime suites); added to the merge gate | core, rt | M | Workflow file; branch protection lists the check |
| S13.2 | Suite manifest tripwire: every fixture listed; CI fails if a listed fixture is missing or `#[ignore]`d without an ADR reference (Rule 2) | core, rt | S | Tripwire self-test with a planted removal |
| S13.3 | Mutation pass: re-introduce each of bugs (a) to (e) and each SCL fix's inverse; the suite must fail on every mutant | core, rt | M | Mutation table in EVIDENCE (agentile refactor-mutate) |
| S13.4 | TLC in CI for both modules, green configs and mutant configs (mutants must fail) | core | S | CI log |
| S13.5 | CODEOWNERS for the lifecycle contract surfaces (O-16) | core, rt | S | CODEOWNERS diff |

## SCL-S14: Packaged native acceptance

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S14.1 | macOS arm64, signed and notarized build: first launch, cold model load, noisy logs, Hermes with browser and toolchain, quit, restart, update install and restart, force quit then relaunch | core | M | Recorded run, process lists, versions (EVIDENCE) |
| S14.2 | Linux x64 on the DGX: AppImage and `.deb`, same scenarios | core | M | Recorded run (EVIDENCE) |
| S14.3 | Windows x64 by the Windows team: installer, same scenarios, update install path | core | M | Recorded run (EVIDENCE) |
| S14.4 | Budget measurements: diagnostic bytes, thread counts, `fork+exec` launch cost, close duration | core | S | Measurement table; budgets confirmed or re-gated |

## SCL-S15: Red-team

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S15.1 | Second-model adversarial pass on this planset before Stage-2 (O-6); public corrections section in 00; findings private | core | M | Corrections section; private findings table on #298 |
| S15.2 | Red-team on merged code: an adversarial review of the SCL diff on the integration branch and of the packaged build (signal targeting, exit-path bypass, pid reuse, Job breakaway, nested ownership) | core, rt | L | Private report; `g5-redteam` |
| S15.3 | Remediation of every High and the re-run of the suite | core, rt | M | No open High |

## Rule-12 drift pre-list

Add `[[drift]]` entries to `citrate-federation/manifest.toml` **before** the Cargo edits:

| Consumer | Dependency | WP |
|---|---|---|
| citrate-core (`kit/Cargo.toml`) | citrate-agent-runtime shared containment crate (O-9) | SCL-S9.0 |

No other cross-repo dependency is planned. The `ureq` exact pin and the `windows-sys`
feature flags are intra-repo changes and go through the supply-chain review skill.

## Totals

| Effort | Count of WPs |
|---|---|
| S | 14 |
| M | 39 |
| L | 14 |
| XL | 0 (S11 is split into five WPs) |
| **All** | **67 WPs in 16 sprints** |
