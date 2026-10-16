---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed)
updated: 2026-10-05 (owner decisions: O-18 accepted as D-15, O-19 accepted as D-16; second set: D-16 extended to ask first, release tags v0.5.0 / v0.5.1; third set: S7.5a moves to v0.5.1, v0.5.x renamed v0.5.2)
red_teamed: 2026-10-05 (adversarial pass, 29 findings, 3 blocking; corrections in 08_RED_TEAM.md supersede conflicting text)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core + citrate-agent-runtime
companions: 02_ARCHITECTURE.md, 05_SPRINTS_AND_WPS.md, gates.yaml
---

# User Stories, Acceptance Criteria, BDD

## How to read this

- Stories are `US-<epic>.<n>`. Epic numbers follow the SCL sprint that delivers them.
- Every acceptance criterion names its **data source** (Rule 7).
- Every capability has Gherkin. The five reproduced supervisor bugs are regression scenarios
  tagged `@regression @bug-a` to `@bug-e`; they live in the standing fault-injection suite
  (SCL-S13) and may not be removed without an ADR.
- A story is done only when every AC is proven by a test or a recorded run named in
  gates.yaml. "Implemented" is not "native-accepted" is not "released".
- Personas: **Member** (an everyday user on one machine), **Windows Member**, **Builder** (a
  developer using Hermes with the browser and toolchain), **Maintainer** (someone changing
  core or the runtime).

---

## E0 Pre-cut safety (SCL-S0)

**US-0.1: Startup cleanup touches only what Citrate owns.** As a Member, launching Citrate
Core never signals a process that Citrate did not start.
- AC1: Startup cleanup matches exact owned executable paths only. *Source: S0.1 decoy
  fixture.*
- AC2: On each OS's installed package the check passes. *Source: native run note on #298.*

**US-0.2: Windows update leaves nothing running from the old version.**
- AC1: The installer starts only after every sidecar from the old version has stopped.
  *Source: Windows native run, task list before and after.*

**US-0.3: The managed browser does not outlive Hermes.**
- AC1: After a Hermes stop, and after a hard kill followed by the next start, no managed
  browser process or profile from the earlier run remains. *Source: S0.3 fixture.*

**US-0.4: An old node after a genesis-change update is named, not silent.**
- AC1: If an older node still holds the chain database or ports, the app says so and offers
  a way out. *Source: S0.6 packaged update run per OS + unit test.*

```gherkin
Feature: Pre-cut safety

  Scenario: A process that only resembles Citrate is left alone
    Given a process not started by Citrate that resembles a Citrate sidecar
    # Red-team correction (2026-10-05, RT-23): wording genericized; specifics on #298
    And an orphaned Citrate sidecar from a previous run at this bundle's exact binary path
    When Citrate Core starts
    Then the orphaned sidecar is stopped
    And the other process is still running

  Scenario: Windows update install drains first
    Given a Windows Member running Citrate Core with the node and Hermes started
    When they install an update
    Then every sidecar from the running version has exited before the installer starts
    # Red-team correction (2026-10-05, RT-03): not runnable in 0.5.0 (no Windows updater
    # feed). Kept as a code-path test for S0.2; the native proof is the scenario below.

  Scenario: Manual Windows install with the old version running
    Given a Windows Member running Citrate Core 0.4.2 with the node and Hermes started
    When they run the 0.5.0 installer by hand
    Then the installer stops only this installation's own sidecar binaries before copying files
    And no process from another location is signalled
    And after the install no sidecar from 0.4.2 is running

  Scenario: Managed browser cleaned after a hard kill
    Given Hermes is running with its managed browser open
    When the Hermes sidecar is killed without a graceful stop
    And Hermes starts again
    Then no browser process or profile directory from the earlier run remains

  Scenario: An older node left behind by an update
    Given an update from 0.4.x left the old node running
    When Citrate Core 0.5.0 starts
    Then the app shows "An older Citrate node is still running; quit it or restart"
    And it does not reset or open the chain database
```

---

## E1 Acknowledged controls (SCL-S2, SC1)

**US-1.1: Stop means stop.** As a Member, when I stop a service it stays stopped until I
start it again.
- AC1: After Stop is committed, no new child is spawned in any phase, including `Stopped`
  and `Backoff`. *Source: fault suite, child pid count over 2 s; TLC INV-5.*
- AC2: A repeated Stop joins the first one's receipt. *Source: fault suite; INV-6.*
- AC3: A spawn that was admitted before Stop and returns after it goes straight to
  teardown. *Source: delayed-spawn fixture; INV-12.*

**US-1.2: Controls report what happened, not what was asked.**
- AC1: Start and Stop return `Accepted`, `AlreadySatisfied`, `RejectedClosing`, `Busy`,
  `Quarantined` or `ObservationUnknown`; `Accepted` is never shown as done. *Source: kit
  receipt tests.*
- AC2: Stop finishes as `Complete(scope)` or `Incomplete(remaining, reason)`. *Source:
  receipt tests.*
- AC3: A replacement never lets the old generation's cleanup touch the new generation's
  files or status. *Source: held-cleanup fixture; INV-12, INV-14.*

```gherkin
Feature: Acknowledged controls

  @regression @bug-a
  Scenario: A second Stop on a stopped service does not respawn it
    Given a supervised service that has been stopped and is Off
    When Stop is called again on the same handle
    Then no new child process is created within 2 seconds
    And the service is still Stopped

  Scenario: Stop wins over a pending start
    Given a Start whose prerequisite validation is still running
    When the Member stops the service
    Then the validation result is discarded as superseded
    And no child process is spawned

  Scenario: A late spawn after Stop is torn down
    Given a spawn was admitted and the operating system call has not returned
    When Stop is committed
    And the spawn returns a child
    Then that child goes straight to teardown
    And the service never reports Ready for it

  Scenario: Stop reports incomplete cleanup honestly
    Given a service whose reader thread cannot finish
    When the Member stops it
    Then the receipt is Incomplete and names the reader
    And the service is Quarantined, not Stopped

  Scenario: Old cleanup cannot touch a new generation
    Given generation 3 is stopping and its cleanup is held
    When a replacement for generation 4 is requested
    Then generation 4 is not admitted until generation 3's cleanup completes
    And no file written for generation 4 is removed by generation 3's cleanup
```

---

## E2 Readiness and retry credit (SCL-S3, SC2)

**US-2.1: Ready means it answered.** As a Member, a service is shown ready only when it
answered its health check just now.
- AC1: `Ready` only from a fresh success of the current incarnation. *Source: kit tests;
  INV-13.*
- AC2: Liveness-only services show `NotAssessed`, never `Ready`. *Source: profile tests;
  INV-9.*

**US-2.2: A service that never becomes healthy stops retrying.**
- AC1: For probed services, the retry budget resets only after sustained fresh successes;
  grace and elapsed time never reset it. *Source: fault suite; INV-7, INV-8.*
- AC2: Liveness-only services keep today's reset rule. *Source: INV-1, INV-9.*

```gherkin
Feature: Readiness and retry credit

  @regression @bug-b
  Scenario: Grace longer than healthy_after does not reset the retry budget
    Given a probed service with grace 120 ms, healthy_after 40 ms and max_retries 1
    And its health probe always fails
    When it runs for 3 seconds
    Then it reaches Failed
    And it spawned at most 2 children

  Scenario: Sustained health earns credit
    Given a probed service that answers its health probe for longer than healthy_after
    When it crashes
    Then its consecutive failure count starts again from zero

  Scenario: Cold load is alive but not ready
    Given the local model server is loading a large model
    When the Member opens chat
    Then the model server shows Alive and Awaiting
    And chat waits or explains that the model is still loading

  Scenario: A liveness-only service is never called ready
    Given the IPFS service is running
    Then its readiness is NotAssessed
```

---

## E3 Bounded probes (SCL-S4, SC3)

**US-3.1: A stuck health check cannot pile up.**
- AC1: At most one probe worker per service, across timeouts and replacements. *Source:
  fault suite thread count; INV-10, INV-11.*
- AC2: A probe that never returns quarantines the service with a visible reason and a
  "Restart service" action (O-3). *Source: kit test + vitest.*

**US-3.2: Health checks use only loopback.**
- AC1: The health adapter resolves only the configured loopback literal, with no resolver
  thread, proxy or redirect. *Source: adapter tests.*

```gherkin
Feature: Bounded probes

  @regression @bug-c
  Scenario: Timed-out probes do not accumulate
    Given a service whose health probe blocks forever
    When 50 probe intervals pass and the service is replaced 3 times
    Then at most 1 probe worker for that service is alive
    And the service is Quarantined with reason "health probe did not return"

  Scenario: Releasing a stuck probe frees its slot
    Given a Quarantined service with one blocked probe
    When the blocked probe returns
    And the Member chooses "Restart service"
    Then a new probe may start

  Scenario: The health adapter never resolves names
    Given the probe URL authority is not the configured loopback address
    When a probe runs
    Then the request is rejected before any connection
```

---

## E4 Bounded diagnostics (SCL-S5, SC4)

**US-4.1: A noisy service cannot fill memory or disk.**
- AC1: Each retained line is at most 16 KiB plus a truncation marker; each ring at most
  256 KiB and 500 entries. *Source: fault suite byte counts.*
- AC2: Crash records are bounded (64 queued, 8 KiB each, 2 rotated files of 16 MiB);
  saturation drops records with a visible counter. *Source: sink tests.*
- AC3: Budgets are confirmed on packaged builds. *Source: S14.4 measurements.*

```gherkin
Feature: Bounded diagnostics

  @regression @bug-d
  Scenario: One very long line does not hold a megabyte
    Given a service that writes a single 1 MiB line with no newline
    When its log is captured
    Then the retained text for that line is at most 16 KiB plus a truncation marker
    And the dropped byte counter shows the rest

  Scenario: Many long lines stay inside the ring budget
    Given a service that writes four 4 MiB lines
    Then the retained ring is at most 256 KiB

  Scenario: A slow log viewer does not stall the service
    Given the log snapshot reader is slow
    When the service writes continuously
    Then the service's output pipes keep draining
    And Stop still completes inside its deadline

  Scenario: A full disk degrades diagnostics, not the service
    Given the crash record location cannot be written
    When the service crashes
    Then the crash is counted as a dropped diagnostic
    And the supervisor continues
```

---

## E5 and E6 Service owners (SCL-S6, S7; SC5, SC6)

**US-5.1: Every long-lived service has one owner.** As a Member, the node, node agent,
memory, IPFS, both model servers, Comms, Cluster and Hermes are each started, observed and
stopped by exactly one owner.
- AC1: Every manager uses the lifecycle cell; every command in the S1.4 map uses receipts or
  the compatibility layer (O-2). *Source: per-command tests.*
- AC2: Generation-owned files (bearer tokens, seed files, key files passed to children) are
  removed only by the generation that wrote them, and the #243 keep-list is never touched.
  *Source: held-cleanup fixtures.*
- AC3: The node's chain DB reset runs only after the node owner reports `Complete`.
  *Source: #243 tests extended.*
- AC4: IPFS preflights are owned and bounded. *Source: S6.3 tests.*

**US-6.1: Local inference waits for a ready model.**
- AC1: The four local inference paths and embedding calls require fresh `Ready`. *Source:
  S7.1 fixture.*

```gherkin
Feature: Service owners

  Scenario Outline: Each service stops cleanly through its owner
    Given the <service> service is running
    When the Member stops it
    Then the receipt is Complete for <service>
    And no process, reader or probe for <service> remains

    Examples:
      | service    |
      | node       |
      | node agent |
      | memory     |
      | llama chat |
      | llama embed|
      | ipfs       |
      | comms      |
      | cluster    |
      | hermes     |

  Scenario: Credential files belong to their generation
    Given the Comms service is replaced while the old generation's cleanup is held
    When the old cleanup finishes
    Then the new generation's seed and bearer files are still present

  Scenario: Chain reset waits for the node to be gone
    Given a genesis change requires a chain database reset
    When the node owner has not reported Complete
    Then the reset does not start

  Scenario: Stuck IPFS init owns its slot
    Given the IPFS init preflight is stuck
    When IPFS start is requested again
    Then the request is Busy
    And no second init process starts
```

---

## E7 Bespoke ownership and recorded cleanup (SCL-S8)

**US-7.1: One-shot tools have an owner and a deadline.**
- AC1: Every core `Command::new` outside the supervisor goes through the registry; the
  tripwire fails otherwise. *Source: S8.1 tripwire test.*
- AC2: Each bespoke kind has a deadline and an owned kill path. *Source: S8.2 per-kind
  tests.*

**US-7.2: Cleanup after a crash is precise.** As a Member, after Citrate crashes, the next
launch cleans up only what that run started.
- AC1: A recorded process is signalled only if pid, start time and exact path match.
  *Source: S8.3 fixtures (pid reuse, same name elsewhere).*
- AC2: Update-moved and deleted bundle paths are handled per OS. *Source: native runs.*

```gherkin
Feature: Bespoke ownership and recorded cleanup

  Scenario: An unbounded tool call gets a deadline
    Given a hardware probe tool that never exits
    When Citrate Core runs it
    Then the tool is stopped at its deadline
    And the owner reports Complete

  Scenario: A recorded orphan is cleaned at next launch
    Given Citrate Core crashed while a recorded sidecar was running
    When Citrate Core starts again
    Then that sidecar receives a graceful stop and then a kill if needed
    And its record is removed

  Scenario: A reused pid is not signalled
    Given a record for pid 4242 with start time T1
    And pid 4242 now belongs to a process with start time T2
    When Citrate Core starts
    Then pid 4242 is not signalled
    And the stale record is removed

  Scenario: A same-named process elsewhere is not signalled
    Given a process named like a Citrate sidecar running from another path
    When Citrate Core starts
    Then it is not signalled
```

---

## E8 Unix containment (SCL-S9, SC8)

**US-8.1: Stopping a service stops its ordinary descendants.**
- AC1: Ordinary descendants in the owned session are gone after Stop. *Source: fault suite
  marker pids, Linux and macOS.*
- AC2: No group signal after the leader is reaped. *Source: wrapper counter or trace.*
- AC3: A descendant that leaves the session is reported as excluded, not claimed cleaned.
  *Source: escape fixture.*

```gherkin
Feature: Unix descendant containment

  @regression @bug-e
  Scenario: Background descendants do not survive Stop
    Given a supervised child running "sh -c 'sleep 295 & sleep 296 & wait'"
    When the service is stopped
    Then neither sleep process is alive
    And the receipt is Complete

  Scenario: The leader exits first
    Given the session leader exits while a descendant keeps running
    When Stop runs
    Then the descendant is signalled through the retained group
    And the leader is reaped only after the final group signal

  Scenario: An escaped descendant is reported, not hidden
    Given a descendant that starts its own new session
    When Stop runs
    Then the receipt names the containment scope
    And the escape fixture records it as outside that scope

  Scenario: A competing reaper disables containment for that profile
    Given another component reaps child processes in the same app
    Then the containment profile reports unsupported
    And its gate criterion stays open
```

---

## E9 Windows Job containment (SCL-S10)

**US-9.1: On Windows, a service's whole process tree ends with it.**
- AC1: Children start suspended, are assigned to the owner's Job, then resume. *Source:
  Windows CI accounting count.*
- AC2: Breakaway is impossible. *Source: breakaway fixture.*
- AC3: Stop is `Complete` only at `ActiveProcesses == 0` inside the deadline. *Source:
  Windows CI.*
- AC4: If Citrate Core dies, kill-on-job-close ends the captured tree. *Source: native run
  by the Windows team.*

```gherkin
Feature: Windows Job containment

  Scenario: Grandchildren are inside the Job
    Given a Windows Member starts a service whose child starts a grandchild
    Then the Job accounting shows 2 active processes

  Scenario: Breakaway is refused
    Given a child that asks to create a process outside its Job
    Then the new process is still inside the Job

  Scenario: Stop completes only when the Job is empty
    Given a service with a grandchild that ignores the graceful step
    When the service is stopped
    Then the Job is terminated
    And the receipt is Complete only after ActiveProcesses is 0

  Scenario: A crash of Citrate Core ends the captured tree
    Given services with descendants are running on Windows
    When Citrate Core is terminated from Task Manager
    Then no process from those Jobs remains
```

---

## E10 Hermes-sidecar children (SCL-S11)

**US-10.1: Hermes's children have owners too.** As a Builder, when Hermes stops, its
workers, browser, search service, toolchain runs, commands, MCP servers and helpers stop
with it, or the app tells me which did not.
- AC1: Each of the 10 kinds is owned with a deadline in the runtime. *Source: runtime
  fixtures per kind.*
- AC2: Hermes Stop's receipt includes the runtime's child report. *Source: S11.5 tests.*
- AC3: After a hard kill of the sidecar, the next Hermes start or app launch cleans recorded
  children. *Source: S11.4, S11.5 fixtures.*
- AC4: `/health` stays control-plane scoped; child ownership is a separate status field
  (O-13). *Source: control API test.*

```gherkin
Feature: Hermes-sidecar children

  Scenario: Hermes stop drains its children
    Given Hermes is running a toolchain build, a shell command and the managed browser
    When the Member stops Hermes
    Then each child has exited or is listed as incomplete in the Hermes receipt

  Scenario: A wrapped MCP server ends with its wrapper
    Given an MCP stdio server started through a launcher wrapper
    When the MCP host closes it
    Then neither the wrapper nor the server is running

  Scenario: Recorded children are cleaned after a sidecar hard kill
    Given the Hermes sidecar was killed while a toolchain run was in progress
    When Hermes starts again
    Then the toolchain run from the earlier sidecar is gone

  Scenario: Health does not pretend children are fine
    Given a Hermes child failed to stop
    Then /health still reports the control plane
    And the runtime status reports one incomplete child
```

---

## E11 App exit coordinator (SCL-S12, SC7)

**US-11.1: Every way out drains first.**
- AC1: Every exit path in 07's table goes through the coordinator. *Source: exit-path
  matrix (S12.5); TLC X-1.*
- AC2: No exit, restart or installer before `Complete` inside the deadline. *Source: S12.1
  tests; X-2, X-5.*
  *Owner decision (2026-10-05, O-18 accepted, D-15):* for exit this AC is superseded by
  US-11.4. Restart and installer keep it; Quit always ends.
- AC3: The webview cannot call raw exit, restart or install. *Source: capability test.*
- AC4: The UI never freezes during close. *Source: frame-timing check in S14.*

**US-11.2: The status view is honest and cheap.**
- AC1: One raw snapshot request in flight; StrictMode does not duplicate starts. *Source:
  vitest.*
- AC2: Revisions compare numerically. *Source: vitest.*

```gherkin
Feature: App exit coordinator

  Scenario Outline: Each exit path drains every owner
    Given the node, memory, model server and Hermes with a browser are running
    When the Member <action>
    Then every owner reports Complete before <final>

    Examples:
      | action                               | final                        |
      | closes the last window               | the app exits                |
      | chooses Quit                         | the app exits                |
      | chooses Restart after an update      | the app restarts             |
      | installs an update on Windows        | the installer starts         |
      | installs an update on macOS or Linux | the restart is offered       |
      | runs a factory reset                 | data is deleted and app exits |

  Scenario: Hermes drains before the services it depends on
    When the app starts closing
    Then Hermes and its children drain before the node and memory begin to drain

  Scenario: An expired close does not exit late
    Given one owner cannot finish before the close deadline
    When the deadline passes
    Then the app reports incomplete cleanup and does not exit on its own
    And a later explicit Quit retries the close
    # Red-team correction (2026-10-05, RT-02, O-18): superseded for Quit by US-11.4.
    # It still holds for Restart and update install.

  Scenario: Raw exit is not available to the webview
    When web content calls the process exit or restart command
    Then the call is rejected by capability checks

  Scenario: Repeated quit joins the first
    Given the app is already closing
    When the Member chooses Quit again
    Then no second drain starts

  Scenario: Navigation does not restart services
    Given the status view is open
    When the Member navigates away and back in development StrictMode
    Then no service start is requested
    And at most one status request is in flight
```

---

## E12 Standing fault-injection suite (SCL-S13)

**US-12.1: This class of bug cannot quietly return.** As a Maintainer, a change that
reintroduces any of the known lifecycle defects fails CI.
- AC1: `lifecycle-faults` is a required check on ubuntu, macos and windows runners.
  *Source: workflow + branch protection.*
- AC2: The manifest tripwire fails on a removed or ignored fixture without an ADR
  reference. *Source: tripwire self-test.*
- AC3: Each reintroduced bug is caught. *Source: S13.3 mutation table.*
- AC4: TLC mutants still fail. *Source: S13.4 CI log.*

```gherkin
Feature: Standing fault-injection suite

  Scenario: Reintroducing a known bug fails CI
    Given a change that lets Stop in Off continue the spawn loop
    When CI runs
    Then the lifecycle-faults check fails on the bug-a regression scenario

  Scenario: A fixture cannot be silently dropped
    Given a change that deletes a listed fault fixture
    And the change cites no ADR
    When CI runs
    Then the manifest tripwire fails

  Scenario: The model still has teeth
    When the TLC job runs the mutant configs
    Then each mutant reports its named invariant violated
```

---

## E13 Packaged native acceptance (SCL-S14)

**US-13.1: It works on the machines members use.**
- AC1: The S14 scenario list passes on macOS arm64 (owner), Linux x64 (DGX) and Windows x64
  (Windows team) packaged builds. *Source: recorded runs.*
- AC2: Budgets measured and confirmed or re-gated. *Source: S14.4.*

```gherkin
Feature: Packaged native acceptance

  Scenario Outline: Full lifecycle on a packaged build
    Given a clean install of the packaged app on <os>
    When the tester runs the S14 scenario list from first launch to force quit and relaunch
    Then no Citrate-started process remains after each exit
    And every incomplete cleanup was shown to the tester

    Examples:
      | os            |
      | macOS arm64   |
      | Linux x64     |
      | Windows x64   |
```

---

## Red-team additions (2026-10-05)

Added by the red-team pass ([08_RED_TEAM](08_RED_TEAM.md)). They supersede conflicting text
above.

**US-6.2: Chat stays where the member put it.** (RT-12, O-19)
*Owner decision (2026-10-05, O-19 accepted, D-16, locked):* while the local model is
cold-loading or its probe times out, chat waits or tells the member; it never routes the
prompt to the remote gateway without the member's explicit choice. `g3-provider-routing`
blocks the cut. As a Member with a local model,
a cold load or a slow health check never sends my prompt to the remote gateway by itself.
- AC1: While the local owner is `Awaiting` or `Stale`, local requests wait boundedly or fail
  honestly; the route is not `LocalFallback`. *Source: S7.5 tests over `select_inference_state`
  with the new readiness inputs.*
- AC2: `LocalFallback` only when the local owner is `Failed`, `Stopped` or `Quarantined`.
  *Source: S7.5 tests.*
- AC3: Under generation load, a timed-out probe does not change the route. *Source: S14
  packaged run under load.*
- AC4 (owner decision 2026-10-05): the gateway is used for a waiting prompt only when the
  member explicitly chooses it for that prompt; the choice is never preselected or
  remembered silently. *Source: S7.5 vitest and command test.*
- AC5 (owner decision 2026-10-05, second set, D-16 extended): when the local model is
  `Failed`, `Stopped` or `Quarantined`, chat asks the member first: restart the local model,
  or send this message to the gateway this time. Nothing is sent before the member chooses,
  and the choice covers only that message. Never silent. *Source: S7.5a (v0.5.0, today's
  signals: the local server process is not running) and S7.5 (v0.5.1, readiness model) Rust
  and vitest tests; `g4-native-v050` macOS check.* *Owner decision (2026-10-05, third set): S7.5a
  ships in v0.5.1, not v0.5.0; its macOS check moves to `g4-native-macos-cut`.*

**US-7.3: Cleanup after a crash is a barrier.** (RT-10, RT-11)
*Owner decision (2026-10-05, second set):* AC1 and AC2 ship in v0.5.0 as S8.5a (over the S0.1
cleanup); AC3 needs S8.3's records and ships in v0.5.1 as S8.5b.
- AC1: No node spawn is admitted until startup cleanup has finished. *Source: S8.5 test.*
- AC2: The chain reset refuses while another process holds the chain database lock, even if
  nothing answers on the local RPC. *Source: S8.5 owned lock-holder fixture.*
- AC3: A record whose boot identity differs from this boot is removed and nothing is
  signalled. *Source: S8.5 unit test.*

**US-10.2: The sidecar cannot direct core to signal arbitrary processes.** (RT-09)
- AC1: Core signals a recorded runtime child only when it is tied to a Hermes incarnation core
  itself recorded. *Source: S11.7 tests; details private.*
- AC2: Hermes's child report is taken only after the runtime closed its own admission.
  *Source: S11.6 test with an MCP reconnect racing the report.*

**US-11.3: Quit from the Dock or at logout still drains.** (RT-01)
- AC1: Dock Quit and logout run a synchronous bounded drain inside `Exit` over the same
  owners and deadline. *Source: S12.6 test + macOS native run.*
- AC2: The app menu's Quit takes the async coordinated path. *Source: S12.6 test.*

**US-11.4: Quit always ends.** (RT-02, O-18)
*Owner decision (2026-10-05, O-18 accepted, D-15, locked):* install and restart require
`Complete`; Quit force-stops what it still owns and exits, reporting `Incomplete`.
- AC1: When an owner is still `Incomplete` at the deadline, Quit performs a final actuation of
  every OS-process scope still held, records `Incomplete` and exits. *Source: S12.7 test with a
  never-returning probe worker.*
- AC2: Restart and update install still require `Complete`. *Source: S12.7 test.*
- AC3: The next launch says cleanup was incomplete when it was. *Source: S12.7 test.*
- AC4 (owner decision 2026-10-05): an expired Quit is never reported `Complete`, and the
  final actuation happens before the process exits. *Source: S12.7 test; TLC X-11, X-12.*

**US-11.5: Edges of closing.** (RT-18, RT-20, RT-22)
- AC1: After a macOS update is installed and restart is postponed, no bundle binary is spawned
  until restart. *Source: S12.8 test.*
- AC2: A second launch during `Closing` is not lost. *Source: S12.8 test.*
- AC3: Pending approval and budget requests are rejected with a recorded reason on `Closing`
  (A-7). *Source: S12.1 test.*

```gherkin
Feature: Red-team additions

  Scenario: A cold load does not move chat off the device
    Given a verified local model and a configured gateway key
    And the local model server is still loading
    When the Member sends a chat message
    Then the message is not sent to the gateway
    And the app says the local model is still loading

  Scenario: Dock Quit drains synchronously
    Given the node, memory and Hermes are running on macOS
    When the Member chooses Quit from the Dock
    Then every owner is drained inside the close deadline before the process ends

  Scenario: A stuck worker cannot keep the app open
    Given a model-server health check that never returns
    When the Member chooses Quit
    And the close deadline passes
    Then the app stops every process it still owns and exits
    And the next launch reports that cleanup was incomplete

  Scenario: Hermes cannot start children after reporting them stopped
    Given Hermes is stopping and an MCP server reconnect is due
    When the runtime takes its child report
    Then no child is started after the report
    And core reports Complete only if no Hermes child is running

  Scenario: An orphan that holds the database blocks the reset
    Given a process from an earlier run holds the chain database lock and does not answer RPC
    And the address book names a new genesis
    When Citrate Core starts
    Then the chain database is not reset
    And the app names the problem instead of waiting silently

  Scenario: Postponed restart after an update
    Given a macOS update is installed and the Member chose Later
    When the model server exits and would be retried
    Then it is not restarted from the new bundle
    And the app asks the Member to restart to finish the update
```

## Owner decisions (2026-10-05)

Scenarios for the locked decisions D-15 (O-18) and D-16 (O-19). They govern where they
conflict with scenarios above; both are in the v0.5.0 cut-blocking subset (D-2 amended).

```gherkin
Feature: Quit always ends (D-15)

  Scenario: Quit with a stuck owner ends and reports Incomplete
    Given the node, memory and the model server are running
    And the model-server health check never returns
    When the Member chooses Quit
    And the close deadline passes
    Then the app force-stops every process it still owns
    And the app exits
    And the cleanup is recorded as Incomplete, never Complete
    And the next launch tells the Member that cleanup was incomplete

  Scenario: Restart waits for Complete
    Given an update is installed and one owner cannot finish before the close deadline
    When the Member chooses Restart
    Then the app does not restart
    And the app says which service did not stop
    And Quit is still available and ends the app

  Scenario: Install waits for Complete
    Given the Hermes runtime reports a child it cannot observe
    When the Member chooses to install an update
    Then the installer does not start
    And the app says Hermes cleanup is not complete

  Scenario: Factory reset still ends the app
    Given one owner cannot finish before the close deadline
    When the Member runs a factory reset
    Then the app force-stops every process it still owns, deletes the data and exits
    And the cleanup is recorded as Incomplete

Feature: No silent gateway fallback (D-16)

  Scenario: A probe timeout under load keeps chat local
    Given a verified local model and a configured gateway key
    And the local model server is generating and its health check times out
    When the Member sends a chat message
    Then the message is not sent to the gateway
    And the message waits for the local model or the app tells the Member it is busy

  Scenario: The Member chooses the gateway explicitly
    Given a verified local model and a configured gateway key
    And the local model server is still loading
    When the Member sends a chat message
    And the app tells the Member the local model is still loading
    And the Member explicitly chooses to send this message to the gateway
    Then only that message is sent to the gateway
    And the next message is not sent to the gateway without a new choice
```

### Owner decision (2026-10-05, second set): ask first, never silent (D-16 extended)

These govern where they conflict with the scenarios above. Release: v0.5.0 for the stopped
case on today's signals (S7.5a); v0.5.1 for the readiness model, including `Failed` and
`Quarantined` (S7.5).
*Owner decision (2026-10-05, third set):* both ship in v0.5.1; S7.5a moved out of v0.5.0.

```gherkin
Feature: Ask first when the local model is down (D-16 extended)

  Scenario: A stopped local model asks before using the gateway
    Given a verified local model and a configured gateway key
    And the local model server is not running
    When the Member sends a chat message
    Then the message is not sent anywhere yet
    And the app asks the Member to restart the local model or send this message to the gateway this time

  Scenario: The Member restarts the local model
    Given the app is asking because the local model server is not running
    When the Member chooses to restart the local model
    Then the local model server is started
    And the message is answered by the local model once it is ready
    And nothing is sent to the gateway

  Scenario: The Member sends one message to the gateway
    Given the app is asking because the local model server is not running
    When the Member chooses to send this message to the gateway
    Then only that message is sent to the gateway
    And the next message asks again while the local model is still down

  Scenario: Launch does not ask or route while the app is starting the local model
    Given a verified local model and a configured gateway key
    And the app has just launched and is starting the local model server
    When the Member sends a chat message
    Then the message waits for the local model, within a bound, and the app says it is starting
    And the message is not sent to the gateway

  Scenario: A failed or quarantined local model asks first (v0.5.1)
    Given a verified local model and a configured gateway key
    And the local model owner is Failed or Quarantined
    When the Member sends a chat message
    Then the app asks the Member to restart the local model or send this message to the gateway this time
    And nothing is sent before the Member chooses
```
