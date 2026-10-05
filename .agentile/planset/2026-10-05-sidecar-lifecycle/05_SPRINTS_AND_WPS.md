---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed)
red_teamed: 2026-10-05 (adversarial pass, 29 findings, 3 blocking; corrections in 08_RED_TEAM.md supersede conflicting text)
updated: 2026-10-05 (owner decisions: O-20 accepted, D-2 amended to a cut-blocking subset; O-18, O-19 accepted)
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
| *Red-team correction (RT-13)* | **S9.0 to S9.3 and S10.0 to S10.3 depend only on gate0**; only S9.4 (kit adoption) waits for S5. S5 may run beside S3 and S4 (SPEC-001: no semantic dependency); recommended | |
| SCL-S10 | S9.0 (crate scaffold) | S9 |
| SCL-S11 | S9, S10, S7.3 | S12 (until S12.5) |
| SCL-S12 | S6, S7, S8; S12.5 also S11 | |
| SCL-S13 | S1.3 (harness), grows with every sprint; S13.1 to S13.4 close after S11 and S12 | |
| SCL-S14 | S9, S10, S11, S12, S13 | |
| SCL-S15 | S15.1 after S1; S15.2 after S14; S15.3 after S15.2 | |

v0.5.0 is cut only when every SCL gate is met (D-2). SCL-S0 lands before the cut whatever
the state of the rest.

*Owner decision (2026-10-05, O-20 accepted, D-2 amended):* the sentence above is superseded.
v0.5.0 is cut when every SCL criterion marked `blocks_cut: true` in [gates.yaml](gates.yaml)
is met. The rest of SCL finishes on the 0.5.x line under this planset. The split, the
dependency graph for the cut and the new estimate are in
[Owner decision (2026-10-05): cut-blocking subset](#owner-decision-2026-10-05-cut-blocking-subset).

---

## SCL-S0: Pre-cut safety (details private, #298)

Sprint file: `.agentile/sprints/active/sprint-scl-s0-precut-safety/SCOPE.md`. Specifics of
S0.1, S0.3 and S0.4 are private; the public text here is deliberately generic.

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S0.1 | Narrow the startup orphan cleanup to the exact executable paths of this bundle's own sidecar binaries; keep it as the one-release legacy path (O-17). *(Wording genericized by red-team correction RT-23.)* | core | S | Red test: a decoy process that resembles a sidecar but was not started by Citrate survives; a real owned orphan is cleaned (`src-tauri` unit + fixture tests). Native check on each OS's installed build (private note on #298) |
| S0.2 | Windows update install stops every sidecar before the installer runs: wire the updater's before-exit hook (verified at the pinned tauri-plugin-updater 2.10.1) to the bounded shutdown path. Superseded by S12.2 | core | S | Unit test that the hook is installed and calls the shutdown path; Windows native run by the Windows team: after install, no sidecar from the old version is running (task list before and after, EVIDENCE) |
| S0.3 | Managed-browser cleanup on Hermes stop and at the next Hermes start or app launch (pre-cut safety fix tracked privately) | rt, core | M | Fixture: hard-kill the sidecar with a managed browser running; after the next start, no browser process from the earlier run and no leftover profile directory (runtime fixture + core test) |
| S0.4 | Runtime-side containment correction (pre-cut safety fix tracked privately) | rt | S | Red test in the runtime crate named on #298; green on Linux and macOS CI |
| S0.5 | Shutdown coverage check: embed stops with Hermes; runtime workers exit on stdin close; record per OS | core, rt | S | QA record in S0 EVIDENCE (process list after quit on macOS, Linux, Windows) |
| S0.6 | Genesis-change update check per OS: after a 0.4.x to 0.5.0 update, no old node holds the chain DB lock or ports; if one does, a startup check names it in the UI ("An older Citrate node is still running; quit it or restart") instead of a silent wedge | core | M | Packaged update run per OS (EVIDENCE); unit test for the startup check using an owned fixture listener on the RPC port |

*Red-team corrections (2026-10-05):* **S0.2 (RT-03)** is conditional: the Windows bundle ships
no updater artifacts in 0.5.0, so the native run it names cannot happen. It lands with the
first release that publishes a Windows feed and is not a `g1-precut` item; S0.7 covers the
path Windows members actually use. **S0.6 (RT-03, RT-11)**: "packaged update run per OS" means
the macOS in-app update and the manual Linux and Windows installs; its unit test also uses an
owned fixture that holds the chain database lock without answering RPC (the lock check lands
in S8.5; S0.6 names the problem in the UI).

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
| *RT-16* | S2.4 acceptance adds: a publisher that panics or exits leaves admission unavailable and creates no replacement; a receipt read after the deadline reports expiry even if the publisher never ran | | | |

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
| *RT-19* | S6.4 also removes the stall watchdog's invented `setState({ node: "off" })` after an unchecked Stop (`store.ts:3486-3493`) and maps the real receipt | | | |

## SCL-S7: Owners, group B (SC6)

| WP | Work | Repo | Effort | Acceptance (data source) |
|---|---|---|---|---|
| S7.1 | Llama chat and llama embed owners; fresh `Ready` gates the four local inference paths and embedding calls; 180 s grace earns no credit | core | L | Fixture llama server with a slow `/health`: requests wait or fail honestly while `Awaiting`; vitest for UI state |
| S7.2 | Comms and Cluster owners; generation-fenced seed and bearer files; Windows Comms foreground slots re-grounded on #180 (O-14) | core | L | Unix tests + Windows CI tests; Windows native slot release run (Windows team) |
| S7.3 | Hermes owner: control-plane readiness, per-generation bearer cleanup, embed as its own lifecycle cell owned through the Hermes manager | core | M | Tests; bearer file absent after a clean stop |
| *RT-28* | S7.3 acceptance adds: a webview reload with Hermes running reattaches sessions; a shared-service Stop is not a session Stop; the status snapshot makes no Hermes HTTP call | | | |
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
| *RT-15* | S11.3 is XL: split into **S11.3a** (`shell_run`, sandbox probe, `git`, GPU sampler; L) and **S11.3b** (MCP stdio servers including wrapper launchers, MCP probe; L). The ids S11.3a and S11.3b replace S11.3 in tracking | | | |
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
| *RT-14, RT-15* | S13.1 is resized to L and builds on the lanes S1.6 creates; S13.2's tripwire also bans reaping code from the sidecar dependency tree (RT-24) | | | |
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
| *RT-03, RT-12* | S14.2 and S14.3: "update install" means the manual package or installer run (no in-app updater on Linux or Windows in 0.5.0). S14.1 to S14.3 add a chat under generation load with the probe timing out, checking that the route stays local | | | |
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

*Red-team correction (2026-10-05, RT-15):* with the additions below and the S11.3 split, the
program is **80 WPs** (14 S + 1, 39 M + 9, 14 L + 3 net of resizing; S13.1 moves M to L,
S11.3 becomes two L). Total effort is about 280 agent-days; see "Critical path" below.

## Red-team additions (2026-10-05)

New WPs from [08_RED_TEAM](08_RED_TEAM.md). New ids only; nothing is renumbered.

| WP | Work | Repo | Effort | Acceptance (data source) | Finding |
|---|---|---|---|---|---|
| S0.7 | Windows installer pre-install step: an NSIS hook stops only this installation's own sidecar binaries (exact image path equality with the install directory's binaries; graceful step, then terminate) before files are copied | core | M | NSIS hook test on a hosted Windows runner; Windows team native run of a manual 0.4.2 to 0.5.0 install with the old app and Hermes running: task list before and after, no old sidecar left, no other process signalled (EVIDENCE) | RT-04 |
| S1.6 | Windows and macOS CI lanes for kit, the src-tauri lifecycle tests and the runtime lifecycle crates (the three existing Windows supervisor tests run for the first time) | core, rt | L | Workflow files; first green run linked in S1 EVIDENCE | RT-14 |
| S1.7 | TLA+ corrections: runtime admission and `RuntimeClosing` in `AppExitCoordinator.tla`; X-10, X-11; X-2, X-5 restated for restart and installer; X-9 with the Hermes sub-deadline; reachability witnesses for every mutant; new mutants (reconnect after report, Quit waits forever, Terminate without drain). *Owner decision (2026-10-05, D-15):* adds X-12 `QuitActuatesHeldScopes` and its mutant (03) | core | M | TLC: green configs pass, every mutant fails on its named invariant, every witness is violated on the green config | RT-01, RT-02, RT-08, RT-17, RT-26 |
| S7.5 | Provider routing under readiness (O-19, accepted by the owner 2026-10-05 as D-16; blocks the cut): `LocalFallback` only for a `Failed`, `Stopped` or `Quarantined` local owner; bounded wait or honest failure while `Awaiting` or `Stale` | core | M | Unit tests over `select_inference_state` with the new inputs; vitest for the UI state; S14 run under load | RT-12 |
| S8.5 | Startup cleanup barrier before node admission; boot identity in records; per-OS path rule; #243 reset also requires the chain database lock to be free; stated crash-window residual | core | M | Owned lock-holder fixture blocks the reset; spawn admitted only after cleanup; boot-mismatch record removed without a signal | RT-10, RT-11 |
| S10.0 | Windows spawn-mechanism spike on stable Rust (thread enumeration plus `ResumeThread`, or a raw `CreateProcessW` backend); ADR addendum with the choice | rt | M | Hosted Windows CI: suspended child assigned then resumed; assignment failure terminates the suspended child; Job handle non-inheritable (no duplicate in any child); kill-on-close fires when the parent is killed | RT-05 |
| S11.6 | Runtime Closing gate before the child report (no new child, MCP reconnect, lazy browser or search start); `nested_owner` stop profile in the shared crate (leader TERM, child report, group TERM, one group KILL) | rt, kit | M | Race fixture: an MCP reconnect due during the report never starts a child; the ordered drain runs before any group signal | RT-07, RT-08 |
| S11.7 | Core validation of runtime ownership records: records are claims, tied to a core-recorded Hermes incarnation before any signal; record location not writable by sandboxed or member-added programs; @rule8 review | core, rt | M | Tests per the private criteria on #298; @rule8 review recorded | RT-09 |
| S12.6 | Unpreventable exits: synchronous bounded drain inside `RunEvent::Exit` over the same owners and absolute deadline; custom app-menu Quit item that calls coordinated quit | core | M | Test of the `Exit` drain with stalled owners (bounded); macOS native run of Dock Quit and logout with the process list after (EVIDENCE) | RT-01 |
| S12.7 | Quit with `Incomplete` (O-18, accepted by the owner 2026-10-05 as D-15): final actuation of every OS-process scope still held, then exit; records kept; next-launch report; restart and install still require `Complete` | core | M | Test with a never-returning probe worker: Quit exits inside deadline plus a bound; restart in the same state refuses | RT-02 |
| S12.8 | Edges of closing: `UpdateStaged` admission after a macOS install; a second launch during `Closing` is not lost; pending approval and budget requests rejected on `Closing` (A-7) | core | M | Tests for each; macOS native run of "install, Later, model server exit" | RT-18, RT-20, RT-22 |
| S14.5 | Native-run calendar: a containment dry run after S9 and S10 and the final S14 run, with named people and dates for the owner Mac, the DGX and the Windows team | (process) | S | Calendar in the S14 sprint file, agreed by each team | RT-25 |

Dependencies of the additions: S1.6 and S1.7 are part of gate0. S0.7 belongs to SCL-S0 and
`g1-precut`. S10.0 precedes S10.1. S11.6 precedes S11.4 and S11.5; S11.7 precedes S11.5. S7.5
lands with S7.1. S8.5 lands with S8.3 (the barrier must exist before the sweep is removed).
S12.6 and S12.7 land with S12.1; S12.8 after S12.2.

## Critical path (red-team estimate, 2026-10-05)

Midpoint sizes: S = 1 agent-day, M = 3, L = 7. Corrected graph per RT-13.

| Step | Work | Agent-days |
|---|---|---|
| 1 | gate0 (S1.1 to S1.4, S1.6, S1.7 in parallel) plus owner sign-off | 7 to 9 |
| 2 | S2 | 12 to 16 |
| 3 | S3 | 6 to 7 |
| 4 | S4 (S5 beside it if allowed) | 4 to 6 |
| 5 | S6 and S7 in parallel | 10 to 13 |
| 6 | S12.1, S12.2, S12.6, S12.7 (S8 to S11 in parallel lanes since gate0) | 12 to 16 |
| 7 | S12.5, S13 close | 3 to 5 |
| 8 | S14 with one fix loop | 6 to 10 |
| 9 | S15.2, S15.3 | 10 |
| | **Critical path** | **about 70 to 92 agent-days** |

With four to five lanes and agent fan-outs compressing implementation by roughly 1.5 to 2x
(reviews, native runs and sign-offs do not compress): about 50 to 65 working days, 10 to 14
calendar weeks. Starting 2026-10-06, a cut that waits on every SCL gate lands about
mid-December 2026 to mid-January 2027. O-20 (08) is the lever if that is not acceptable.
*Owner decision (2026-10-05):* O-20 accepted; see the next section for the cut estimate.

## Owner decision (2026-10-05): cut-blocking subset

**O-20 accepted; D-2 amended** (see [00 Locked decisions](00_OVERVIEW.md#locked-decisions)).
The v0.5.0 cut is gated on the cut-blocking subset below. Everything else in SCL finishes on
the 0.5.x line under this same planset: still owned, still gated in
[gates.yaml](gates.yaml) (`blocks_cut: false`), just not blocking the 0.5.0 cut. The
critical-path estimate above stays as the record of the single-cut plan; this section
governs where they differ.

### Cut-blocking vs 0.5.x

| Cut-blocking (v0.5.0) | 0.5.x follow-up (same planset, still gated) |
|---|---|
| **Pre-cut safety:** S0.1, S0.3, S0.4, S0.5, S0.6, S0.7 (Windows NSIS pre-install stop of this installation's own sidecars, RT-04) | **Pre-cut safety:** S0.2 (conditional on a Windows updater feed; none in 0.5.0, RT-03) |
| **Theory (gate0):** S1.1 supervisor TLA extension, S1.2 `AppExitCoordinator.tla`, S1.3 fault harness, S1.4 re-grounding *(moved in)*, S1.5 baseline *(moved in)*, S1.6 Windows and macOS CI lanes, S1.7 TLA corrections | **Theory:** `OwnedContainment.tla` if S1.1 decides it is needed (used by S9.2, S10.2) |
| **Primitive (SC1 to SC4):** S2 acknowledged controls, S3 readiness and retry credit, S4 bounded probes, S5 bounded diagnostics (all WPs) | (none) |
| **Supervised owners:** S7.1 llama chat and embed owners with fresh `Ready`; S7.5 provider routing (O-19, D-16); S6.4a stall watchdog maps the real Stop receipt *(moved in, RT-19)*. All 9 supervised services run on the new cell through the O-2 compatibility layer | **Supervised owners:** S6.1 node, S6.2 node agent and memory, S6.3 IPFS, S6.4b group A consumers, S7.2 Comms and Cluster (incl. Windows slots), S7.3 Hermes owner, S7.4 group B consumers |
| **Core spawns and cleanup:** S8.1 ownership registry and `Command::new` tripwire *(moved in)*, S8.3 recorded-ownership cleanup replaces the name/path sweep for core-spawned processes (O-17 legacy path), S8.5 startup barrier and database-lock check for #243 | **Core spawns:** S8.2 bespoke kinds onto the registry with deadlines, S8.4 node descendants contained |
| **Hermes children (minimum):** S11.4 child report and bounded drain *(moved in, resized L)*, S11.5a Hermes receipt includes the report *(moved in)*, S11.6 runtime Closing gate before the report *(moved in)* | **Hermes children:** S11.1 workers and toolchain, S11.2 browser and search, S11.3a, S11.3b, S11.5b core applies runtime records at next start, S11.7 records are claims (@rule8) |
| **Containment:** none. Quit's final actuation (D-15) reaches direct children only | **Containment:** S9.0 to S9.4 shared crate and Unix backend (with the `[[drift]]` entry), S10.0 to S10.4 Windows Job objects |
| **Exit, quit, update:** S12.1 coordinator, S12.2 raw `process:default` and `updater:default` removed with native update commands (@rule8), S12.3 factory reset and #243 reset through `Complete` *(moved in)*, S12.5a exit-path matrix for the cut, S12.6 synchronous macOS quit fallback (RT-01), S12.7 Quit always ends (O-18, D-15), S12.8 `UpdateStaged` (RT-20) with its second-launch and A-7 edges | **Exit:** S12.4 frontend observer, S12.5b exit-matrix rows for Hermes children and Windows Job |
| **Keeping it fixed:** S13.1 to S13.5 as a required CI check on three OSes over every fixture of the cut (S13.5 CODEOWNERS *moved in*) | **Keeping it fixed:** each 0.5.x WP adds its fixtures to the same required check |
| **Native acceptance:** S14.1 cut scenarios on the packaged macOS build, S14.6 Linux smoke on the DGX (new), S14.4 macOS budget slice *(moved in)*, S14.5 native-run calendar *(moved in)*; Windows native for the cut is the S0.7 run only | **Native acceptance:** S14.1 full, S14.2 full Linux, S14.3 Windows, S14.4 Linux and Windows budgets |
| **Red-team:** S15.1 (done, accepted by the owner as D-17), S15.2 and S15.3 on the merged cut code and the packaged macOS build | **Red-team:** S15.2 and S15.3 on the 0.5.x remainder |

### What moved between columns, and why

Into the cut (not on the owner's list, forced by a dependency):

- **S1.4, S1.5.** S2's compatibility layer (O-2) is written against the command-to-consumer
  map and re-grounded line references from S1.4; S1.5 is the Rule-2 baseline every cut PR
  records against.
- **S6.4a (RT-19 slice of S6.4).** Once S2 lands, an incomplete legacy Stop returns a
  structured cleanup-pending error instead of `Ok`. The node stall watchdog ignores the Stop
  result and sets the node to "off" before restarting it, so it would spin and show an invented
  state. Only the watchdog fix moves; the rest of S6.4 stays 0.5.x as S6.4b.
- **S8.1.** S8.3 writes ownership records through the registry, and the `Command::new`
  tripwire stops new unowned spawn sites from landing while S8.2 is on 0.5.x.
- **S11.4, S11.5a, S11.6.** D-15 keeps Restart and install requiring `Complete`, and Hermes
  is `Complete` only when its children are (X-7). Without the runtime's child report core can
  never observe Hermes `Complete`, so the macOS update restart would be refused whenever Hermes
  runs. S11.6 is what makes that report race-free (RT-08). For the cut, S11.4 reports every
  child kind through the handles the runtime already holds (sessions, workers, browser,
  search, MCP; per-command process groups for shell and toolchain runs); a kind it cannot
  observe is reported `ObservationUnknown`, and Hermes is then not `Complete` (Restart and
  install refuse and say why; Quit still ends under D-15). The `nested_owner` stop in S11.6 is
  leader TERM, child report, leader KILL until S9 adds the group steps. Applying runtime
  records at next launch (S11.5b) needs S11.7 and stays 0.5.x.
- **S12.1.** S12.6 and S12.7 land with it.
- **S12.3.** D-15 names the factory-reset exit; once S12.2 removes raw process exit, the
  factory reset and the #243 reset must route through the coordinator.
- **S12.8 in full.** The owner named `UpdateStaged` (RT-20). The other two edges, a second
  launch during `Closing` (RT-18) and pending approvals rejected on `Closing` (A-7, RT-22),
  belong to the same Closing gate S12.1 introduces; leaving them out would ship a gate that can
  lose a launch or leave an approval half-admitted at quit.
- **S13.5.** CODEOWNERS for the contract surfaces is the "named owner" fix from "How this was
  missed"; size S.
- **S14.4 macOS slice.** D-13 requires the diagnostic budgets to be measured on a packaged
  build before the gate flips, and S5 is in the cut.
- **S14.5.** The cut still needs three machines (owner Mac, DGX, Windows team for S0.7).

Out of the owner's list:

- **S0.2.** It targets the Windows in-app update path, which is not live in 0.5.0 (RT-03). It
  stays in SCL-S0 as conditional and has nothing to gate at the cut; S0.7 is the Windows
  defense that ships.

Residuals the cut ships with (stated, not hidden):

- **Bug (e) stays open** for descendants of supervised services until S9, S10 and S11
  (`g2-bugs`, `blocks_cut: false`). Known cases are narrowed by S0.3, S0.4 and S0.5.
- Quit's final actuation under D-15 reaches direct children only; group KILL on Unix and
  `TerminateJobObject` on Windows arrive with S9 and S10.
- Core's bespoke spawns keep today's behavior until S8.2; the tripwire stops new ones.
- Ownership records in the cut carry pid, start time, boot identity and exact path of the
  direct child; group and Job identity join them on 0.5.x.

### New and split WP ids

Nothing is renumbered. The ids below replace their parent in tracking.

| WP | Work | Repo | Effort | Acceptance (data source) | Column |
|---|---|---|---|---|---|
| S6.4a | Node stall watchdog: consume the real Stop receipt; remove the invented `setState({ node: "off" })`; never restart after a Stop error (RT-19) | core | S | vitest with a cleanup-pending Stop: no restart, no "off" state | cut |
| S6.4b | The rest of S6.4: group A command consumers and `store.ts` mapping | core | M | As S6.4 | 0.5.x |
| S11.4 (cut) | S11.4 resized M to L for the cut: report every child kind through existing handles; `ObservationUnknown` for any kind not observable; drain inside core's close deadline (O-11); bounded shutdown of blocking turns | rt | L | Control API test per kind; timing test of drain under a busy turn | cut |
| S11.5a | Core side: Hermes receipt includes the child report; Hermes `Complete` only when the report is `Complete` | core | M | Core tests with a fixture sidecar that reports `Incomplete` and `ObservationUnknown` | cut |
| S11.5b | Core side: next Hermes start and app launch apply runtime records (needs S11.7) | core | M | As S11.5 | 0.5.x |
| S12.5a | Exit-path matrix for the cut's owners and paths (`g3-exit-paths-cut`) | core | M | One test or recorded native run per row | cut |
| S12.5b | Exit-path matrix rows for Hermes children under the shared crate and the Windows Job | core, rt | S | As S12.5 | 0.5.x |
| S14.6 | Linux x64 smoke on the DGX: AppImage and `.deb`; launch, quit, manual package update over 0.4.x, relaunch; no Citrate-started process left | core | S | Recorded run (EVIDENCE), `g4-linux-smoke-cut` | cut |

### Dependencies for the cut

S12.1 waits for S2.4 (receipts) and S11.5a (Hermes row), not for S6, S7.2 to S7.4 or S8.2.
S7.1 and S7.5 wait for S5. S8.1, S8.3 and S8.5 wait for S2. S11.4 and S11.6 wait for gate0
(S1.7 models the runtime Closing gate). S13 closes after S12.5a. S14 cut runs after S13 closes.
S15.2 cut slice after S14 cut.

### Estimate (owner decision 2026-10-05)

Same sizing as above: S = 1 agent-day, M = 3, L = 7; reviews, native runs, @rule8 reviews and
owner sign-offs do not compress.

**Effort.** Cut-blocking subset: 53 WPs, about **165 agent-days**. 0.5.x remainder: 34 WPs,
about **135 agent-days**. Total about 300, roughly 20 more than the single-cut plan, from the
second native round, the second red-team round, the S11.4 resize and the split WPs.

**Critical path for the cut:**

| Step | Work | Agent-days |
|---|---|---|
| 1 | gate0 (S1.1 to S1.7 in parallel) plus owner sign-off on the ADR and O-1 to O-17 | 7 to 9 |
| 2 | S2 | 12 to 16 |
| 3 | S3 | 6 to 7 |
| 4 | S4 (S5 beside it) | 4 to 6 |
| 5 | S7.1 with S7.5 (in parallel lanes since S2: S12.1, S12.2, S12.3, S12.6 to S12.8; S8.1, S8.3, S8.5; S11.4, S11.5a, S11.6; S6.4a) | 8 to 10 |
| 6 | S12.5a exit matrix, S13 close | 3 to 5 |
| 7 | S14 cut (macOS packaged, Linux smoke) with one fix loop | 4 to 7 |
| 8 | S15.2, S15.3 on the cut code | 7 to 10 |
| | **Critical path, cut** | **about 51 to 70 agent-days** |

With four to five lanes and the same 1.5 to 2x compression on implementation: about **36 to
50 working days, 7 to 10 calendar weeks**. Starting 2026-10-06, the v0.5.0 cut lands about
**late November to mid-December 2026**, against mid-December 2026 to mid-January 2027 for the
single-cut plan. The saving is three to four weeks, not more, because the longest chain
(gate0, S2, S3, S4, the owners, the coordinator, native, red-team) is shared by both plans.
The larger gain is risk: the cut no longer waits on the Windows Job mechanism (RT-05, RT-29)
or on a full Windows native window; the only Windows native run the cut needs is S0.7.

**0.5.x remainder.** Critical path S10.0, S10.1, S10.2 (13), S11.1 to S11.3b in two or three
lanes (about 14), S11.7 and S11.5b (3 to 6), S12.5b (1), full three-OS S14 with a fix loop (6
to 10), S15 remainder (10): about **47 to 54 agent-days**, **7 to 9 calendar weeks** after the
cut if the containment lanes start then, or about **4 to 6 weeks** if S9.0 to S9.3 and S10.0 to
S10.3 run in spare lanes before the cut (they depend only on gate0). That puts the full SCL
program at about mid-January to mid-February 2027.
