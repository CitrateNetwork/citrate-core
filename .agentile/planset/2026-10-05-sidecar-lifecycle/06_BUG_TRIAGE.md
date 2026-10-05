---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed)
red_teamed: 2026-10-05 (adversarial pass, 29 findings, 3 blocking; corrections in 08_RED_TEAM.md supersede conflicting text)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core
companions: 04_FEATURES_BDD.md, 05_SPRINTS_AND_WPS.md
---

# Bug Triage: supervisor defects reported in #240 / #241

The five observations are @mfarzanansari's (SPEC-001 "Acceptance and empirical evidence",
run on Linux at `main` `30c789e`). We re-ran them on the integration branch.

**Target:** `origin/release/0.5.0-hermes-upskill` at `d16f194`. `kit/src/supervisor.rs` has
SHA-256 `14d72aec…2d1c`, identical to `30c789e` and to the contributor's recorded hash;
`supervisor_tests.rs` and `supervisor_windows_tests.rs` are also byte-identical. Every
primitive-level finding therefore applies to the v0.5.0 line unchanged.

**Method:** a temporary integration test against the public kit API on macOS 15.6.1 arm64,
rustc 1.96.0, run twice (5 of 5 reproduced both times). Each fixture recorded every pid it
created and a guard killed and reaped all of them; both runs ended with no leftover marked
process. The test was not committed; it becomes the seed of the SCL-S13 suite. Line numbers
below are on `d16f194`.

| # | Defect | Verdict on `d16f194` | Observed (run 1; run 2 in brackets) |
|---|---|---|---|
| a | A second Stop on one handle respawns the child | **Reproduced** | After the first Stop the state is Off with no child. A second `stop()` returns at once; within 1 s a new child is Running and alive at OS level, with `restarts` still 0. A third Stop returns it to Off |
| b | The retry budget resets on elapsed Running time alone | **Reproduced** | grace 120 ms, healthy_after 40 ms, max_retries 1, always-false probe: never reaches Failed in 3 s; 18 [17] recorded failures, 19 [18] distinct child pids, 0 probe successes; every crash record has `restart_attempt` 0. Control (grace 0, healthy_after 10 s): Failed after about 100 ms |
| c | Timed-out probe threads overlap without a bound | **Reproduced, worse than reported** | Inside grace: 22 [22] probe threads blocked at once within 1 s. Outside grace: 20 [21] concurrent blocked probes and 19 [20] restarts. The contributor stopped counting at 4; the count grows by one per interval |
| d | The 500-line log cap does not bound bytes | **Reproduced** | One 1,048,576-byte line: 1 line and 1,048,576 bytes retained. Four 4 MiB lines: 16,777,216 bytes retained |
| e | A descendant survives Stop of its direct parent | **Reproduced on macOS** (contributor: Linux) | `sh -c '/bin/sleep 295 & /bin/sleep 296 & wait'`: after Stop the shell is gone and both sleeps are alive, reparented to launchd |

## Mechanisms (release branch)

- **(a)** In `kit/src/supervisor.rs:1000-1003`, the `Stop` arm of the Off wait `continue`s
  the outer spawn loop. `stop()` (`:635-646`) returns at once when already Off. The doc
  comment at `:634` says "Idempotent", which is not true. The backoff-path equivalent
  (`:1335-1339`) terminates instead, so the two paths disagree.
- **(b)** `refresh_health!` (`:1182-1190`) sets `sustained_healthy` from elapsed time only;
  `:985-986` then resets `consecutive_failures`. A grace window longer than `healthy_after`
  always earns the reset before the first counted failure.
- **(c)** The `ProbeRunner` is dropped on timeout, inside grace (`:1266-1270`) and outside it
  (`:1270-1274`); a new probe spawns at `:1284`; nothing bounds the detached threads.
- **(d)** `BufRead::lines()` (`:733`, `:748`) has no per-line byte bound; the ring (`:428`)
  evicts by count only.
- **(e)** `terminate_child` signals only the child pid (`:877`, `:892`); `to_command`
  (`:320`) sets no process group, session or Job.

## Reachability (what we can and cannot claim)

- **(a)** Not reached through today's managers: all nine take the handle out of their slot
  before calling `stop()`, so a repeated public Stop finds nothing. It is a primitive defect
  that any new caller, or a refactor that keeps the handle, would hit. Severity: Medium,
  fixed in SCL-S2.2.
- **(b)** Production profiles with grace longer than `healthy_after`: llama chat (180 s /
  60 s, `serve.rs:75`, `:64`) and llama embed, new on this line (60 s / 30 s,
  `embed_serve.rs:50-51`). Comms, Cluster and Hermes (20 s / 30 s) and the node (90 s) are
  not in that ordering. A model server that never becomes healthy would retry without the
  intended limit. No live incident was measured. `SupervisorStatus.restarts` is not shown
  anywhere in the UI (the Node surface's "Supervised restarts" label reads a list that only
  demo seeds fill), so the over-retry is invisible apart from crash-record files.
  Severity: Medium, fixed in SCL-S3.2.
- **(c)** Real probes have a 3 s HTTP deadline on a 5 s interval, so a production hang was
  not demonstrated. The primitive has no bound. Severity: Medium, fixed in SCL-S4.1.
- **(d)** Any sidecar that writes long lines without newlines retains them in full.
  Severity: Low to Medium, fixed in SCL-S5.1.
- **(e)** The Hermes sidecar and the node both start descendants. Whether a given real
  descendant survives depends on its parent's own cleanup and on the exit path; the
  per-kind picture is in 07_PROCESS_INVENTORY. Severity: depends on the kind; some cases are
  pre-cut safety items tracked privately (SCL-S0). Fixed for all kinds in SCL-S9, S10, S11.

## Related findings from the same research

- **Startup cleanup by name and path.** `sweep_orphan_sidecars` matches processes by path
  and executable name, not by recorded ownership; it does nothing on Windows. Replaced by
  recorded ownership (D-7, SCL-S8.3). A pre-cut narrowing is tracked privately (SCL-S0.1).
- **Update install exit path.** The pinned updater's Windows install calls its before-exit
  hook and then exits the process directly, so the app's `RunEvent` teardown does not run.
  Minimal fix pre-cut (SCL-S0.2), full fix in the coordinator (SCL-S12.2). On macOS and
  Linux the install swaps the bundle while sidecars keep running until the restart.
  *Red-team correction (2026-10-05, RT-03):* in 0.5.0 the in-app updater is live on macOS
  only; the Windows and Linux bundles set `createUpdaterArtifacts: false`, so the Windows
  install path above is reachable only once a Windows feed exists. The pre-cut Windows work is
  the manual installer path (SCL-S0.7); S0.2 is conditional.
- **Raw capabilities.** The main window holds `process:default` (exit, restart) and
  `updater:default` (check, download, install). Removed in SCL-S12.2.
- **Factory reset.** `local_data_delete` stops sidecars, then exits on a 2.5 s timer
  without the `RunEvent` handlers. Routed through the coordinator in SCL-S12.3.
- **Teardown order.** `shutdown_all_sidecars` stops services one after another on the main
  thread, Hermes last, after the node and memory it depends on. Changed by O-11 and
  SCL-S12.1.

## What we did not reproduce

- The contributor's Linux namespace-isolated baseline (539 app tests, 212 kit tests) and
  their Linux-only descendant and replay fixtures. Our macOS run substitutes `ps` for
  `/proc` and cannot reap launchd-adopted descendants itself; the guard killed them.
- No packaged app was launched and no live incident was measured for any of (a) to (e).
