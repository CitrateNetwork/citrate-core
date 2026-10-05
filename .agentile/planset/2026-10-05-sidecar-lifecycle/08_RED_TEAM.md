---
created: 2026-10-05T23:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5 (adversarial red-team pass)
status: planset (Stage-2, red-teamed)
red_teamed: 2026-10-05 (adversarial pass in a fresh context; 29 findings, 3 blocking)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core + citrate-agent-runtime
companions: 00_OVERVIEW.md, 05_SPRINTS_AND_WPS.md, gates.yaml
---

# Red-team findings (2026-10-05)

This is SCL-S15.1: an adversarial pass over the Stage-1 planset, the adoption ADR and the
contributor's ADR-0001 / SPEC-001 (PR #241 head `01772a3`). The brief was to break the plan,
not polish it. Every claim was checked against code, not docs:

- citrate-core `origin/release/0.5.0-hermes-upskill` at `d16f194`;
- citrate-agent-runtime `origin/main` at `397a6b1`;
- the locked dependency sources in the cargo registry (tao 0.35.3, tauri 2.11.5,
  tauri-runtime-wry 2.11.4, tauri-plugin-updater 2.10.1, tauri-plugin-single-instance 2.4.3);
- Rust `std` stability on the stable channel (both repos pin `channel = "stable"`).

**These findings supersede the plan text they correct.** The affected documents carry a
"Red-team correction (2026-10-05)" note at the point of conflict; the original text is kept
so the change is visible. Where a finding needs new work, the WP is added in
[05 §Red-team additions](05_SPRINTS_AND_WPS.md#red-team-additions-2026-10-05) with a new id
(no existing id is renumbered).

Independence note: this pass ran in a fresh context with an adversarial brief, but on the
same model family that wrote the Stage-1 text. The contributor's design was written by a
different model. `g0-redteam` stays `met: false` until the owner either accepts this pass or a
cross-model pass is added (O-6).

Security-relevant specifics are kept out of this public file (owner policy). Where a finding
has a security angle it is described generically and its specifics are tracked privately.

## Verdict scale

- **holds**: the plan is right as written (evidence given so the claim is reviewable).
- **revise**: the plan is wrong or incomplete; the correction below governs.
- **blocking**: the plan as written cannot reach its gate, or would ship a regression; must be
  resolved before Stage-2 is accepted.

## Findings

| Id | Angle | Finding | Evidence | Severity | Verdict | Correction |
|---|---|---|---|---|---|---|
| RT-01 | 2, 4 Exit paths | On macOS, Cmd+Q, the Dock's Quit and logout reach the app only as `RunEvent::Exit` from `applicationWillTerminate`. That event cannot be prevented. 02 §8 says "`ExitRequested` is prevented and the coordinator is scheduled ... never blocking the main thread". Applied to these paths, nothing drains before the process ends: every Cmd+Q would orphan every sidecar. Today the `Exit` arm drains synchronously, so this is a regression the plan would introduce | tao 0.35.3 `platform_impl/macos/app_delegate.rs:131-135` (only `applicationWillTerminate`, no `applicationShouldTerminate`); core `src-tauri/src/lib.rs:1029-1039` | High | **blocking** | The coordinator gets two entry modes over the same owners and the same absolute deadline: **async** for preventable requests (`ExitRequested`, native coordinated commands) and **synchronous bounded drain** inside the `Exit` callback for unpreventable terminations (Dock Quit, logout, OS shutdown). The app menu's predefined Quit item is replaced by a custom item that calls coordinated quit, so Cmd+Q takes the async path. New WP S12.6, TLA X-10 `UnpreventableExitDrains`, BDD US-11.3 |
| RT-02 | 1 Concurrency | A non-returning worker makes Quit impossible. Safety gate 4, X-2 and X-5 allow a final action only when every owner is `Complete` before the deadline, and SPEC-001 defines `Complete` to include joined probe workers and readers (in-process threads). If one probe worker never returns (the quarantine case), Quit expires, the final action stays disabled until an explicit retry, and the member can leave only by force quit. In-process threads end with the process, so they are not a reason to keep it alive | SPEC-001 "Ownership" (Complete requires actual joins) and "Shutdown" (exits only after Complete); 00 "Safety gates" item 4; 03 X-2, X-5, L-1 (`reported = Incomplete` is an allowed terminal state with the app still open) | High | **blocking** | Proposed **O-18**: split final actions by what they put at risk. **Install and restart** still require `Complete` (unchanged: they replace binaries or start a second instance). **Quit and the factory-reset exit** proceed at the deadline with `Incomplete` recorded: first a final actuation of every OS-process scope still held (group KILL on Unix while the anchor is retained; `TerminateJobObject` on Windows), then exit; in-process threads never block exit; ownership records of anything not observed absent stay for next-launch cleanup; the next launch tells the member cleanup was incomplete. X-2 and X-5 are restated for `Restart` and `Installer` only; new X-11 `QuitAlwaysTerminates`. New WP S12.7 |
| RT-03 | 3, 4 Updater scope | The in-app updater is live on **macOS only**. The Windows and Linux bundles set `createUpdaterArtifacts: false`, and the frontend treats "no entry for this platform" as up to date. Windows and Linux members update by running the installer or package by hand. Δ-2's rationale ("the Windows update path is where orphans matter most"), S0.2, S10.4, S12.2 ("Windows install only after Complete"), S14.2 and S14.3 ("update install path"), the Windows rows of `g3-exit-paths` and `g4-native-windows` all require native runs of an install path that does not exist in 0.5.0. Enabling it is out of scope (updater feed and signing policy are Out). As written those criteria cannot be met | core `src-tauri/tauri.bundle-windows.conf.json:6`, `src-tauri/tauri.bundle-linux.conf.json:6` (`createUpdaterArtifacts: false`); `src/shell/updater.ts` `isNoPlatformInFeed` and its use in `check()` | High | **blocking** | Re-scope: the in-app install path on Windows and Linux is covered by code-path tests only and labelled "not live in 0.5.0"; its native rows are replaced by the **manual installer path** (RT-04). S0.2 becomes conditional: it lands with whatever release first publishes a Windows feed, and is not a 0.5.0 gate item. `g3-exit-paths`, `g4-native-windows`, `g1-precut` notes amended |
| RT-04 | 4 Regression, Windows install | The Windows NSIS installer is 0.5.0 code that runs **before** 0.5.0 starts, during every manual 0.4.x to 0.5.0 install. The 07 and 00 timing fact "0.5.0 can only defend at its own startup" misses this. Tauri's NSIS template checks only the main binary for "app is running"; sidecar `.exe` files that are still running hold their image files open, so overwriting them can fail mid-install or leave mixed versions | core `src-tauri/tauri.bundle-windows.conf.json` (`targets: ["nsis"]`, `installMode: currentUser`, no installer hooks); 07 §D Windows row; B-inventory exit-path table (locked `.exe` files) | High | revise | New **S0.7**: an NSIS pre-install hook stops only this installation's own sidecar binaries, matched by exact image path equality with the binaries in the install directory (graceful step, then terminate), before files are copied; Windows team native run of a real 0.4.2 to 0.5.0 manual install with the old app and Hermes running. This is the Windows form of the O-17 legacy path and inherits its rules |
| RT-05 | 2 Windows Job | Stable Rust cannot do "create suspended, assign, resume" through `std::process::Command` as the plan assumes. `creation_flags(CREATE_SUSPENDED)` is stable, but the primary thread handle (`ChildExt::main_thread_handle`, rust#96723) and `PROC_THREAD_ATTRIBUTE_JOB_LIST` (`CommandExt::spawn_with_attributes`, rust#114854) are nightly-only. "`windows-sys` feature flags only" is not enough. Two failure paths are also unspecified: (1) assignment fails after a suspended create, (2) the Job handle leaks into a child (std spawns with handle inheritance on), which stops kill-on-job-close from firing when core dies | doc.rust-lang.org stable `std::os::windows::process::ChildExt` and `CommandExt` (checked 2026-10-05); both repos' `rust-toolchain.toml` = stable; 02 §6.2 | High | revise | New **S10.0** spike before S10.1: choose and record (ADR addendum) one stable mechanism, either thread enumeration plus `ResumeThread` (adds the `Win32_System_Diagnostics_ToolHelp` feature) or a raw `CreateProcessW` backend that owns its pipes; undocumented `NtResumeProcess` is rejected. Required tests: assignment failure terminates the still-suspended child and never resumes it uncontained; the Job handle is created non-inheritable and no child holds a duplicate; core crash ends the captured tree |
| RT-06 | 2 Windows children | No per-OS child matrix. The runtime has no Windows containment today (`stop_session` is a no-op off Unix); SearXNG does not officially support Windows; whether the managed Chromium launches all its helpers inside a Job that forbids breakaway is unverified (the expected failure is a helper that fails to launch, not an escape). MCP launchers on Windows (`npx.cmd` through `cmd.exe`) add a level the plan does not name | runtime `agent-workers/src/lib.rs:609-610` (`#[cfg(not(unix))] fn stop_session(_sid: u32) {}`); 07 §C has no OS column | Med | revise | 07 gains a per-OS column (correction note there). S10 and S11 fixtures run the **real** managed browser and a real `npx`-launched MCP server inside a no-breakaway Job on Windows; a kind that does not exist on an OS is marked n/a, not "contained" |
| RT-07 | 7 Contributor vs delta | SPEC-001's containment stop sends group TERM, waits, then one group KILL. For Hermes this collides with our Δ-5 / Δ-6: Chromium, SearXNG and MCP servers share the Hermes process group, so they get TERM at the same instant as Hermes, before the runtime's ordered `shutdown_children` (sessions, workers, browser, search, MCP) runs. Sessions then fail mid-drain on transport errors | runtime `agent-sidecar/src/main.rs:121-141`, `sessions.rs:1298-1311` (ordered drain); B-inventory H5 to H7 (no own group); SPEC-001 "Containment scope" | Med | revise | A `nested_owner` stop profile: TERM to the leader only, wait for the runtime's child report inside the Hermes sub-deadline (RT-26), then group TERM, then at most one group KILL, all while the anchor is retained. Test in S11.6 |
| RT-08 | 1 Concurrency, nested | Sequence that violates the core invariant: (1) core stops Hermes; (2) the runtime reports its children `Complete`; (3) work admitted earlier (an MCP reconnect, which the host retries every 2 s, or a turn that reaches `shell_run`) spawns a child; (4) Hermes exits; (5) core reports Hermes `Complete` with a live, unrecorded child. `AppExitCoordinator.tla` cannot find this: `hermesChildren` is one variable that never goes back to `Running`, and runtime admission is not modelled. This is the "model checked an abstraction" failure 00 describes | 03 §2 (X-7 over a single `hermesChildren` value); runtime `agent-mcp-host/src/host.rs:29-30` (reconnect loop); 02 §6.3 step 2 | High | revise | The runtime gets its own Closing gate, closed **before** it takes the child report: no new child spawn, MCP reconnect, or lazy browser or search start after it. TLA: model runtime admission (`RuntimeAdmit` disabled after `RuntimeClosing`) and add a mutant where a reconnect is admitted after the report; it must violate X-7. New WPs S1.7 (model) and S11.6 (code) |
| RT-09 | 5 Security | Ownership records written by the Hermes sidecar cross a trust boundary. The sidecar runs agent-directed and member-added programs. 02 §6.3 step 3 and S11.5 have core act on records the sidecar writes. A pid, start time and path identify a process; they do not authorize signalling it. Specifics are tracked privately | 02 §6.3, §7; 05 S11.5 | Med | revise | Core treats runtime records as **claims, not authority**: it signals a recorded process only when it can tie it to a Hermes incarnation core itself recorded (core-recorded session or Job identity), and record files live outside any location a sandboxed or member-added program can write. New WP S11.7 with @rule8 review; BDD US-10.2. Specifics: private (#298) |
| RT-10 | 1 Records | (1) On Linux, process start time is counted in clock ticks since boot, so pid plus start time is not unique across a reboot; a deterministic start at login can repeat both. (2) D-7 says pid, start time and exact path must **all** match; R-8 says "pid plus start time first, path second" for moved or deleted bundles. The two contradict. (3) There is an unavoidable crash window between `spawn()` returning and the record being durable; a child spawned in that window is unrecorded. Std `spawn()` waits for exec, so the child cannot be held until the record is written. O-17 removes the exact-path fallback "in the release after 0.5.0" regardless | 02 §7; 01 R-8; 00 D-7, O-17 | Med | revise | Record a boot identity (Linux `boot_id`, macOS `kern.boottime`; Windows creation time is absolute). Identity = pid + start time + boot identity; the path rule is a per-OS defined equality, including the documented moved and deleted forms (no "first, second"). State the crash window as a residual. Removing the exact-path fallback for bundle binaries after 0.5.0 becomes conditional on S8.3 measuring it. In S8.5 |
| RT-11 | 4 Regression, #243 | The #243 reset refuses only while a node **answers on the local RPC**. A starting or wedged orphan that holds the chain database lock but does not answer passes that check. Today the startup sweep runs earlier in `setup`, so this rarely matters. S8.3 removes the sweep; S6.1 ("reset runs only after the node owner reports `Complete`") covers this run's node, not an orphan from a crash or from 0.4.x | core `src-tauri/src/node.rs:333-341` (`reconcile_genesis(..., || self.head_height().is_some())`); `lib.rs:324` (sweep in `setup`) | High | revise | Startup cleanup (recorded and legacy) becomes a **barrier**: no node spawn ticket is admitted until it finishes. The reset additionally requires that the chain database lock can be taken, not only RPC silence. Test: an owned fixture that holds the lock and never answers RPC must block the reset. S8.5 |
| RT-12 | 4, 5 Privacy regression | S7.1 replaces `server_healthy: serve.0.is_running()` with fresh `Ready`. `select_inference_state` routes "model ready, server not healthy, gateway key configured" to `LocalFallback`, which is the remote gateway. `Ready` is `Awaiting` through a cold load (grace up to 180 s) and `Stale` or `Unavailable` after one timed-out probe. So prompts that today stay on the device (waiting on a loading local server) would go to the remote gateway, and the route would flap under load. That is a silent change in where a member's prompt is processed | core `src-tauri/src/serve.rs:768`; `src-tauri/src/ai.rs:262-271` | High | revise | Proposed **O-19**: readiness gates admission to the local server but never switches provider by itself. `LocalFallback` only when the local owner is `Failed`, `Stopped` or `Quarantined`, never during `Awaiting` or `Stale`; requests wait boundedly or fail honestly. Tests: a cold load never routes to the gateway; a probe timeout under generation load does not flip the route. New WP S7.5; S14 adds "probe under generation load" |
| RT-13 | 3 Dependency graph | The table makes S9 depend on S5 and S10 on S9.0. The shared containment crate is new code in the runtime with no semantic dependency on the supervisor cell; only kit adoption (S9.4) needs S5. SPEC-001 itself says SC4 (our S5) has no semantic dependency on SC3. As written the graph adds roughly three to four weeks to the critical path | 05 dependency table; SPEC-001 "Stories" (SC8 depends on SC4 only for shared-file reasons) | Med | revise | S9.0 to S9.3 and S10.0 to S10.3 start right after gate0; S9.4 waits for S5. Recommend the owner allow S5 beside S3 and S4 |
| RT-14 | 3 CI reality | Core and runtime CI run on Ubuntu only. The three existing `supervisor_windows_tests` never run in CI. Acceptance for S4.3, S7.2, S10.1 to S10.3 and S11.x says "Windows CI" or "macOS CI", which does not exist | core `.github/workflows/ci.yml:18,66`, `merge-gate.yml:30,77` (ubuntu-latest; only `release.yml` uses macos-14); runtime `.github/workflows/ci.yml:18,82` | High | revise | New **S1.6**: Windows and macOS CI lanes for kit, the src-tauri lifecycle tests and the runtime lifecycle crates, before S2. Both repos are public, so hosted runner minutes are not the constraint; build time is. `g0-ci` added |
| RT-15 | 3 Scope and estimate | 67 WPs at the stated sizes (S about 1 day, M about 3, L about 7) are about **229 nominal agent-days**. Several are undersized: S11.3 covers five child kinds including MCP wrapper launchers (XL, split); S13.1 stands up two new OS lanes for a Tauri app plus the runtime (L); S0.6 assumed an update path that is not live (RT-03). R-1 rates the program "High / High" but gives no estimate | 05 WP tables and totals | Med | revise | Estimate and critical path recorded in 05 (see "Critical path" below). S11.3 split into S11.3a and S11.3b (ids kept). S13.1 resized to L |
| RT-16 | 1 Deadline publisher | S2.4 tests creation failure of the deadline publisher, but not its unexpected termination. SPEC-001 requires that termination also leaves admission unavailable with no replacement actor, and that receipt reads and final-action admission compare the monotonic deadline directly | 05 S2.4; SPEC-001 "GATE" and "Ownership" | Low | revise | S2.4 acceptance adds: a publisher that panics or exits leaves admission unavailable and creates no replacement; a receipt read after the deadline reports expiry even if the publisher never ran |
| RT-17 | 1 TLA vacuity | A mutant config "fails as intended" vacuously if the mutated action is unreachable or if TLC reports a different invariant first. The current model has no Stop transition from `Off` at all, so a careless `MutStopOff` can pass for the wrong reason | 03 §1 (current Stop only from live states); 03 §1 mutant table | Low | revise | Each mutant gets a reachability witness (an invariant `~Reached_<action>` that TLC must violate on the green config) and must fail on its **named** invariant. S1.7 |
| RT-18 | 4 Single instance | A relaunch during the async drain (up to 15 s, window possibly already gone) is forwarded to the closing instance, which only focuses `main`; the new process exits and the member's launch is lost | core `src-tauri/src/lib.rs:290-297` | Low | revise | During `Closing`, a second-instance request is recorded and the app relaunches after a completed quit, or tells the member it is finishing shutdown. S12.8, BDD |
| RT-19 | 4 Node watchdog | The stall watchdog ignores the Stop result and sets the node to "off" before `startNode()`. Under O-2 a cleanup-pending Stop error would make it spin and the UI would show an invented state | core `src/shell/store.ts:3486-3493` | Low | holds, with a gap | S6.1 already requires "watchdog never restarts after a Stop error". S6.4 must also remove the invented `setState({ node: "off" })` and map the real receipt |
| RT-20 | 4 Updater window (macOS) | After "Download & install" the app keeps running on a swapped bundle until the member clicks Restart, which may be never. Supervised retries and lazy starts in that window execute the **new** bundle's binaries next to the old app. The plan drains at restart but does not close admission after install | B-inventory exit-path table ("A respawn in this window ... starts the new binary next to the old app"); 07 §D | Med | revise | After a successful install the coordinator enters `UpdateStaged`: new spawns and retries of bundle binaries are refused with "Restart to finish the update"; running services continue. S12.8, BDD |
| RT-21 | 6 Claim honesty | The ADR's Credit section says ADR-0001 and SPEC-001 "are merged to core `main` from #241". PR #241 is open and unmerged (head `01772a3`, checked 2026-10-05). The relative links in the ADR and 00 resolve on no branch yet | `gh pr view 241`: `state: OPEN`, `mergedAt: null` | Low | revise | Wording corrected by a note in the ADR: "will be merged"; links resolve in #241 until then; `g0-adr` already depends on it |
| RT-22 | 6 Agentile | (1) 12 of 21 gate criteria name no evidence path. (2) No BDD for unpreventable exits, record trust, provider routing or update staging. (3) Pending approvals and budget requests at shutdown are not covered: the reconciliation research flagged that Closing must reject them (Rule-3 amendment A-7), never approve them and never leave them half-admitted; the planset only says "no change to ceremony" | gates.yaml; 04; research C §2 (SC7 row) | Med | revise | Planned evidence paths added; stories US-6.2, US-7.3, US-10.2, US-11.3, US-11.4 added in 04; S12.1 acceptance adds the A-7 rule |
| RT-23 | 5 Public text | Read together, four public lines narrow a privately tracked pre-cut item to its platform and packaging: the S0.1 fix wording, its decoy acceptance, the matching Gherkin, and the 01 native-evidence row. The code is public, but owner policy is that public text does not narrow private findings | 05 S0.1; 04 E0 Gherkin; 01 "What needs native evidence"; gates.yaml `g1-precut` note | Med | revise | Those lines are reworded generically in place (each marked) |
| RT-24 | 2 Sole reaper | Holds. The Hermes sidecar links no `tokio::process` (the runtime's `agent-code` uses it but is not a sidecar dependency), installs no SIGCHLD handler and calls no `waitpid(-1)`; core uses no shell plugin and grants no `shell:*`. Std `Child::try_wait` and `wait` reap that pid, and dropping a `Child` neither kills nor reaps it, so the anchor survives only while the adapter keeps the `Child` and never calls those. The existing monitor's `try_wait` must go. The planned "sole-reaper check at startup" can only inspect SIGCHLD disposition and `SA_NOCLDWAIT`; it cannot see a library that calls `waitpid(-1)` later | runtime `agent-sidecar/Cargo.toml` deps; `agent-workers/src/lib.rs:500-514`; core `src-tauri/capabilities/` (no `shell:*`) | Low | holds, with a guard | Add a CI guard in S13.2: deny the `process` feature path in sidecar crates and grep the dependency sources for wildcard `waitpid`; treat `ECHILD` on an anchor as lost ownership (SPEC-001 already says so) |
| RT-25 | 3 Native-run scheduling | Six gate criteria need the Windows team, three need the DGX, three need the owner's Mac. No capacity or calendar is recorded, and native findings arrive only at S14, the end of the chain | 01 "What needs native evidence"; gates.yaml g1, g3, g4 | Med | revise | Two native windows: a dry run after S9 and S10 (containment only) and the final S14 run. New S14.5 records the calendar and owners |
| RT-26 | 1 Drain order | O-11 / X-9 (no other owner drains until Hermes is `Complete` or `Incomplete`) lets a slow Hermes spend the whole 15 s, so every other owner ends `Incomplete` | 00 O-11; 03 X-9 | Med | revise | Hermes first with a sub-deadline (proposed 7 s of the 15 s); the other owners start at Hermes completion or at the sub-deadline, whichever comes first. X-9 restated with the sub-deadline |
| RT-27 | 4 Cold loads | Holds for retry credit: only llama chat and embed have grace longer than `healthy_after`; attested credit consumes budget only on counted failures, so a slow cold load is not penalized. The routing side effect is RT-12 | 06 "(b)"; 02 §3 | Low | holds | None beyond RT-12 |
| RT-28 | 4 Hermes session reattach | SPEC-001 requires that a shared-service Stop never becomes a session Stop and that snapshot polling never calls Hermes control endpoints. S7.3's acceptance names neither, so a webview reload with Hermes running (which reattaches sessions today) is untested | SPEC-001 "Compatibility" (Hermes row); 05 S7.3 | Low | revise | S7.3 acceptance adds: webview reload with Hermes running reattaches sessions; the status snapshot performs no Hermes HTTP call |
| RT-29 | 7, 3 Contributor right, delta risk | The contributor parked Windows Job ownership and universal descendant containment because native evidence did not exist. Δ-1 and Δ-2 bring both into a release gate. That is a locked owner decision (D-2, D-5, D-10), not an error, but it is the largest single schedule risk in the program | SPEC-001 "GATE" (PARK list); ADR Δ-1, Δ-2 | Med | holds (owner decision) | Raised as **O-20** for the owner, not changed here: optionally split the gates into cut-blocking (S0, S1 to S9, S12, S13, Unix native acceptance) and 0.5.0-line follow-up (S10 Windows Job, S11 beyond S0.3), keeping the HUP `g5-scl` line pointed at the cut-blocking set |

## Blocking items

1. **RT-01**: Cmd+Q, Dock Quit and logout cannot be prevented on macOS; the async coordinator
   alone would orphan everything on every quit. Needs the synchronous `Exit` mode.
2. **RT-02**: Quit can never complete when any in-process worker is stuck. Needs O-18.
3. **RT-03**: the Windows and Linux in-app update install paths are not live in 0.5.0, so the
   native gate rows that require them cannot be met. Needs re-scoping to the manual installer
   path (with RT-04).

## Critical path (honest estimate)

Sizes as 05 defines them, using midpoints: S = 1 agent-day, M = 3, L = 7. "Agent-day" is one
lane of agent implementation plus its review; native runs, @rule8 reviews and owner
sign-offs do not compress with more agents.

**Total effort:** 67 Stage-1 WPs are about 229 agent-days (14 S, 39 M, 14 L). The red-team
additions (12 new WPs, one split and one resize) add about 50, for **about 280 agent-days**
(80 WPs: 15 S, 48 M, 17 L).

**Critical path with the corrected graph (RT-13):**

| Step | Work | Agent-days |
|---|---|---|
| 1 | gate0: S1.1, S1.2, S1.3, S1.4, S1.6, S1.7 in parallel, plus owner sign-off on the ADR and O-1 to O-20 | 7 to 9 |
| 2 | S2 (S2.1, S2.2, S2.3 in sequence on one file, then S2.4) | 12 to 16 |
| 3 | S3 | 6 to 7 |
| 4 | S4 (S5 beside it if the owner allows) | 4 to 6 |
| 5 | S6 and S7 in parallel (S7.1, S7.2 are L; S7.4 after the owners) | 10 to 13 |
| 6 | S12.1, S12.2, S12.6, S12.7 (S11 and the containment lanes ran in parallel since gate0) | 12 to 16 |
| 7 | S12.5 exit matrix, S13 close | 3 to 5 |
| 8 | S14 native acceptance including one fix loop | 6 to 10 |
| 9 | S15.2 red-team on merged code, S15.3 remediation | 10 |
| | **Critical path** | **about 70 to 92 agent-days** |

With four to five parallel lanes and agent fan-outs compressing implementation WPs by roughly
1.5 to 2x (review, native runs and sign-offs do not compress), the critical path is about
**50 to 65 working days**, which is **10 to 14 calendar weeks**. Starting 2026-10-06, that puts
a v0.5.0 cut that waits on every SCL gate at about **mid-December 2026 to mid-January 2027**,
assuming the Windows team and the DGX are available in both native windows (RT-25). The prior
plan shipped v0.5.0 shortly after the reroll soak. The owner's D-2 therefore moves the cut by
roughly two to three months. O-20 is the lever if that is not acceptable.

Gates that need hardware the program may not control:

- Windows native runs (S0.7, S7.2, S10.4, S12.5 rows, S14.3, `g4-windows`,
  `g4-native-windows`): need the Windows team. Hosted Windows runners cover CI, not packaged
  acceptance.
- Linux packaged runs (S9.3, S14.2): need the DGX.
- No gate needs a Windows **in-app update** run any more (RT-03).

## Where the contributor's design was right and our deltas strained it

- **Concurrent drain** (SPEC-001) versus Hermes-first (Δ-6): strict ordering wastes the shared
  deadline (RT-26). Kept Hermes-first with a sub-deadline.
- **Group TERM** (SPEC-001) is right for leaf services and wrong for a nested owner (RT-07).
- **Windows PARKED** (SPEC-001): the contributor's caution was justified by RT-05 and RT-06;
  the owner's scope decision stands (RT-29, O-20).
- **"Remove broad orphan killing"** (SPEC-001) versus "replace" (Δ-3): replacing is right
  (RT-11 shows #243 depends on cleanup), but only as a barrier, with a boot identity (RT-10).

## Where our deltas fixed real gaps in the contributor's design

- The BGE embed server and the Hermes-sidecar children (Δ-1) were missing from SPEC-001's
  eight services; the nested-ownership race (RT-08) exists only because Δ-5 named them.
- Factory reset and the #243 reset as exit paths (Δ-7) are real; SPEC-001 did not cover them.
- A standing fault suite and mutant configs (Δ-9) address the "green while broken" failure,
  provided the mutants are non-vacuous (RT-17).

## Proposed owner decisions added by this pass

| # | Question | Recommended default, pending owner sign-off |
|---|---|---|
| O-18 | What happens when Quit's deadline expires with an owner `Incomplete`? | Quit and the factory-reset exit proceed after a final actuation of every OS-process scope still held, record `Incomplete`, and report it at next launch. Install and restart still require `Complete` (RT-02) |
| O-19 | Can readiness changes switch chat from the local model to the remote gateway? | No. Only a `Failed`, `Stopped` or `Quarantined` local owner allows `LocalFallback`; `Awaiting` and `Stale` wait or fail honestly (RT-12) |
| O-20 | Keep every SCL gate cut-blocking, or split cut-blocking from 0.5.0-line follow-up? | Owner's call (D-2 stands until changed). The split in RT-29 is offered as the schedule lever |
