---
created: 2026-10-05T18:30:00Z
branch: docs/scl-planset
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-1 draft)
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core (primary); citrate-agent-runtime
companions: 00_OVERVIEW.md, 03_TLA_SPECS.md, 07_PROCESS_INVENTORY.md
---

# Architecture

**Normative source.** SPEC-001 ("Normative contract", in
`docs/specs/001-owned-sidecar-lifecycle.md` once forward-merged from `main`) is the detailed
contract for the supervised services, and this planset does not copy it (Rule 9). This
document records how SCL structures the work, where it extends SPEC-001 to the full 34-kind
scope, and the deltas the adoption ADR locks. Where this document and SPEC-001 disagree, the
adoption ADR's delta table decides.

## 1. Owners

Every process kind has exactly one owner. There are three owner shapes, all implementing
one contract (stop, observe, bound, acknowledge):

| Owner shape | Used for | Lives in | Stop scope |
|---|---|---|---|
| **Lifecycle cell** | The 9 supervised long-lived services | core `kit/src/supervisor.rs`, held by each manager | child + its contained group/Job + 2 readers + probe worker + preflights + generation credential files + admitted tickets |
| **Ownership handle** | Core's 12 bespoke spawns (one-shot tools, probes, import, fork dry-run) | core ownership registry (new module in kit) | child + its contained group/Job + capture + deadline |
| **Runtime owner** | The Hermes sidecar's 10 child kinds | citrate-agent-runtime (`agent-sidecar`, `agent-workers`, `agent-shell`, `agent-browser`, `agent-search`, `agent-mcp-host`, `agent-checkpoints`) | same contract, reported to core through the sidecar's status and acknowledged on Hermes stop |

The node's 3 descendant kinds (compiler, model probe, GGUF engine) are owned by the node
process itself and are **contained** by the node's lifecycle cell (its group or Job). SCL
adds no chain code unless a native run shows one of them leaving the node's group or Job;
that would become a federation item.

Rules shared by all three shapes:

- **Single owner.** A process is registered with exactly one owner at spawn admission. No
  second owner may signal it; a lint tripwire forbids `Command::new` in `src-tauri/src` and
  `kit/src` outside the supervisor and the registry (allowlist file, SCL-S8.1).
- **Deadline.** Every owned process has a deadline. Unbounded `.output()` calls are
  replaced by bounded capture with a deadline and an owned kill path.
- **Acknowledgement.** Stop returns a receipt; `Complete` names the scope it attests;
  anything not observed is `Incomplete`.
- **Containment.** Every owned process is spawned through the shared containment crate
  (§6), so its ordinary descendants are inside its owner's group or Job.

## 2. Identity, ordering, controls

Adopted from SPEC-001 "Ownership, intent and generations" without change:

- `appEpoch` (this app run), `controlIntentSequence` (each accepted Start, replacement or
  Stop), `serviceGeneration` (committed replacement config), `childIncarnation` (each
  spawn, including retries), `snapshotRevision` (each published change). Decimal strings
  at the JavaScript boundary, compared numerically (BigInt), never lexicographically.
- One latest-intent cell and one coalescing wake bit per service; no queue.
- Spawn and credential publication are **tickets** registered under the admission lock
  before the OS call; a ticket admitted before Stop is still owned and goes straight to
  teardown.
- Control results: `Accepted(token)`, `AlreadySatisfied`, `RejectedClosing`, `Busy`,
  `Quarantined(reason)`, `ObservationUnknown(token)`. Stop completes as
  `Complete(ownedScope)` or `Incomplete(remaining, reason)`.
- One reserved std deadline-publisher thread and a fixed overlay of **10** deadline
  records. SPEC-001 has 9 (8 services plus app close); SCL counts the embed server as its
  own owner (§10).

**SCL extension:** the ownership handle (§1) uses the same intent, ticket and receipt types
with a single-shot lifecycle (`Starting → Alive → Stopping → Stopped | Incomplete`), no
retry and no readiness probe.

## 3. Phases and readiness

Phase: `Stopped`, `Starting`, `Alive`, `Backoff`, `Failed`, `Stopping`, `Quarantined`.
Readiness (independent): `NotAssessed`, `Awaiting`, `Ready`, `Stale`, `Unavailable`. The
legacy mapping table is SPEC-001's "Compatibility and execution gates" table, adopted.

Service profiles on the release branch (`d16f194`), with the retry policy D-6 assigns:

| Owner | Probe today | Grace / healthy_after | Retry credit (D-6) | Readiness scope |
|---|---|---|---|---|
| Node (`citrate`) | none | default / 90 s | liveness-only | `NotAssessed`; sync and height stay domain facts |
| Node agent | none | default / 30 s | liveness-only | `NotAssessed`; "authed" stays bearer-present |
| Memory (`mem-mcp`) | none | default / 30 s | liveness-only | `NotAssessed`; semantic capability stays a domain fact |
| Llama chat | `GET /health` 5 s, 3 s timeout | 180 s / 60 s | **attested** | `Ready` gates the four local inference paths |
| Llama embed (BGE) | `GET /health` 5 s, 3 s timeout | 60 s / 30 s | **attested** | `Ready` gates embedding calls from Hermes |
| IPFS (kubo) | none | default / 30 s | liveness-only | `NotAssessed`; storage errors stay real errors |
| Comms | UDS / pipe connect, 5 s | 20 s / 30 s | **attested** (connect scope) | connect scope only, not relay reachability |
| Cluster | UDS / pipe connect, 5 s | 20 s / 30 s | **attested** (connect scope) | connect scope only, not roster or peers |
| Hermes | `GET /health` 5 s | 20 s / 30 s | **attested** | control plane only; children reported separately (O-13) |

Llama chat and llama embed both have grace longer than `healthy_after`, which is the
ordering under which bug (b) fires today (06_BUG_TRIAGE).

## 4. Probes and adapters

Adopted from SPEC-001 "Finite actual workers and transports": one probe permit per service
**across generations**; a timeout publishes `Unavailable` but keeps the permit until the
worker actually ends; a worker that never ends quarantines the service. The HTTP adapter
uses a synchronous literal-only resolver and `TcpConnector` through
`ureq::Agent::with_parts`, `ureq = "=3.3.0"` (O-4), no proxy, no redirects, 8 KiB header
limits, status only. IPC connect probes get per-platform qualified adapters. Windows Comms
foreground work uses the 8-slot design (O-14), re-grounded on #180.

## 5. Diagnostics

Adopted from SPEC-001 "Bounded diagnostics", with D-13's numbers as starting values:
truncate while reading (16 KiB partial line, 4 KiB read chunk), 256 KiB and 500 entries per
ring, 64 KiB per snapshot, at most 2 snapshot serializations app-wide, 1 KiB reason, 4 KiB
status metadata, crash sink with 64 queued records of 8 KiB, 2 rotated files of 16 MiB per
owner. Counters for truncated bytes and dropped records are visible. **SCL extension:**
ownership handles (§1) capture with the same reader and budgets; their output is kept only
until the handle completes, then summarized into one bounded record.

## 6. Containment

One shared crate (D-11; location O-9) with two backends and one interface:
`spawn_contained(cmd, profile) -> ContainedChild`, `signal_scope(term|kill)`,
`observe_absence() -> Complete | Incomplete(reason)`.

### 6.1 Unix (macOS, Linux)

Adopted from SPEC-001 "Containment scope": `pre_exec` calling only `libc::setsid`
(async-signal-safe, no allocation, no logging, no panic); do not also call
`process_group(0)`; record SID = PGID = leader pid at spawn; the containment owner is the
sole reaper; poll with typed, freshly zeroed `waitid(P_PID, pid, WEXITED | WNOHANG |
WNOWAIT)`; group TERM, graceful window, at most one group KILL, all while the leader is
unreaped; permanently close actuation before reaping; afterwards only signal-zero
observation, and only `ESRCH` counts as absence. Escaped descendants (a new session or
group) are out of scope and reported as such by a fixture.

### 6.2 Windows (D-10, not parked)

- Create the child with `CREATE_SUSPENDED` (plus the existing `CREATE_NO_WINDOW`), assign it
  to a Job the owner created, then resume its primary thread. No process code runs before
  assignment, so no descendant can be created outside the Job.
- Job limits: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; never set
  `JOB_OBJECT_LIMIT_BREAKAWAY_OK` or `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`.
- Stop: the existing graceful step for that profile, then `TerminateJobObject` inside the
  same deadline.
- Completion: `QueryInformationJobObject(JobObjectBasicAccountingInformation)` reports
  `ActiveProcesses == 0` before the deadline. A completion-port
  `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO` message may wake the owner but is never the only
  proof. Otherwise `Incomplete`.
- The Job handle is retained by the owner until completion. If core dies, the handle
  closes and kill-on-close ends the captured tree (O-12).
- If core already runs inside a Job (some launchers and terminals), nested Jobs apply
  (Windows 8 and later); the owner detects and reports this (R-7).
- Bindings: `windows-sys` 0.61, already in the tree; new feature flags
  (`Win32_System_JobObjects`, `Win32_System_Threading`) only, no new crate. The Windows
  Comms foreground helper keeps its own transport contract (§4).
- SPEC-001's rejection of `process-wrap` 10.0.1 as a full solution is adopted; this backend
  uses the APIs directly.

### 6.3 Nested ownership (Hermes)

The Hermes sidecar is a lifecycle cell in core and is itself contained. Some of its
children create their own session or process group (worker isolation, sandboxed commands),
which on Unix takes them out of the Hermes group, so a group signal to Hermes alone does not
reach them (R-4). The contract therefore nests:

1. The runtime owns each child through the shared crate and keeps its own anchors.
2. Hermes Stop is acknowledged by the runtime only after its children reach `Complete` or
   are reported `Incomplete` by kind; core's Hermes receipt includes that report.
3. On Unix the runtime writes ownership records for its children into a file core can read
   (§7), so a hard kill of the sidecar is still recoverable at the next Hermes start or app
   launch. On Windows the runtime's child Jobs are nested inside the Hermes Job and end
   with it.
4. The sidecar's graceful drain must fit inside core's close deadline (O-11 puts Hermes
   first). Blocking turns (`spawn_blocking`) are bounded or detached-and-owned so runtime
   shutdown cannot wait on them without limit.

## 7. Ownership record and next-launch cleanup (D-7)

- **What is recorded:** for each owned process at spawn: owner kind, `appEpoch`, pid, OS
  process start time, exact binary path, and group id or Job identity. One bounded file per
  app run under the app data directory (fixed maximum entries; written atomically; 0600).
- **When it is removed:** when the owner observes `Complete` for that process.
- **Next launch:** for each record left by an earlier run, signal only if the live process
  with that pid has the **same start time** and the **same exact binary path**. Start time:
  `/proc/<pid>/stat` field 22 (Linux), `proc_pidinfo` `PROC_PIDTBSDINFO` (macOS),
  `GetProcessTimes` (Windows). Path: `/proc/<pid>/exe` (Linux, with the `(deleted)` form
  handled), `proc_pidpath` (macOS, with updater-moved bundles handled), and
  `QueryFullProcessImageNameW` (Windows). A mismatch removes the record and signals nothing.
- **Graceful first:** TERM to the recorded group (Unix) or the graceful step, then KILL,
  inside a startup deadline, off the main thread.
- **Replaces** `sweep_orphan_sidecars` and its `pgrep`, `ps` and `kill` helper processes.
  The one-release legacy path for 0.4.x leftovers is O-17.

## 8. App exit coordinator (SC7, D-8)

Adopted from SPEC-001 "Shutdown and frontend observation", with SCL deltas:

- **Every exit path routes through it** (07_PROCESS_INVENTORY, exit-path table): last
  window close, Cmd+Q / menu quit, updater restart, update install on each OS, the critical
  update path, factory reset (`local_data_delete`), the Hermes setting-change restart (a
  per-owner restart, not an app exit, but drained the same way), and the #243 chain reset
  (runs only after the node owner reports `Complete`).
- **Closing:** atomically close admission (spawn, probe, lazy singleton creation, delayed
  frontend starts), then drain. Drain order is O-11 (Hermes and its children first, then the
  rest concurrently) inside one absolute monotonic deadline (15 s proposed; the 5 s graceful
  child stop sits inside it).
- **Never block the main thread.** `ExitRequested` is prevented and the coordinator is
  scheduled; the final exit, restart or installer launch is admitted only after `Complete`
  and before the deadline.
- **Capabilities:** remove `process:default` and `updater:default` from
  `src-tauri/capabilities/default.json`. Add native commands: coordinated quit, coordinated
  restart, update check, update download, update install (which drains first), and
  coordinated reset. `src/shell/updater.ts` moves to them, including the critical-update
  path. Updater signature, feed and platform policy are unchanged.
- **Force exit, crash, logout:** not `Complete` (O-12).
- **Single instance:** the second instance is still rejected before any service admission.

## 9. Frontend observation

Adopted from SPEC-001 with O-15: native state owns services through navigation; one raw
snapshot request in flight across observer epochs (the raw `invoke` promise, not the
timeout-raced wrapper); 5 s cadence plus one coalesced refresh after each control; accept
matching epochs and numerically newer revisions only; StrictMode-safe start-up; snapshots
read cached cells and do no service I/O. **SCL extension:** the Node surface label
"Supervised restarts" currently shows a crash list that only demo seeds fill; it is
replaced by the real counters from the new DTO (SCL-S12.4).

## 10. Thread and memory budgets

SPEC-001 sets 35 supervisor-owned threads for 8 services (43 with the Windows Comms slots).
SCL counts 9 lifecycle cells (embed is its own owner), so the ceiling becomes **38**
(9 monitors, 18 readers, 9 probe slots, 1 crash sink, 1 coordinator), **46** with the 8
Windows Comms slots, plus the ownership registry's bounded reader pool (proposed: at most 4
concurrent bespoke captures, 8 reader threads). Deadline overlay: 10 records of at most
128 bytes. These are ceilings to measure (O-5), not total-app thread or RSS claims.

## 11. Data sources (Rule 7)

Every new or changed command names its source before code:

| Surface | Data source |
|---|---|
| Service status snapshot | the lifecycle cell's cached status (no I/O during the read) |
| Control receipt | the monitor's acknowledged intent sequence and the receipt slot |
| Readiness | the named probe for that profile (llama and embed `/health`, Hermes `/health`, Comms and Cluster connect probe); `NotAssessed` for liveness-only |
| Retry counters | the monitor's attempt, failure and respawn counters |
| Log snapshot | the byte-bounded ring |
| Crash records | the bounded crash sink files |
| Bespoke process status | the ownership registry entry |
| Next-launch cleanup report | the ownership record files plus OS process queries (§7) |
| Hermes child ownership | the runtime status field (O-13), from runtime owners |
| Exit / restart / update / reset progress | the coordinator state and its deadline record |
| Update metadata | tauri-plugin-updater `check()` through the native wrapper (unchanged feed) |

## 12. Platform scope

| Capability | macOS arm64 | Linux x64 | Windows x64 |
|---|---|---|---|
| Lifecycle cell, receipts, readiness, probes, diagnostics | yes | yes | yes |
| Descendant containment | owned session (§6.1) | owned session (§6.1) | owned Job (§6.2) |
| Crash of core | next-launch recorded cleanup | next-launch recorded cleanup | kill-on-job-close, plus recorded cleanup for anything outside a Job |
| Update install exit path | restart path drains | restart path drains | install drains before the installer runs |
| Native acceptance | owner Mac | DGX | Windows team |

## 13. Rule bindings

- **Rule 1:** fixture processes and injected transports only behind `#[cfg(test)]`. No
  placeholder `Complete` or `Ready`.
- **Rule 2:** SC1 and SC2 change semantics that existing supervisor tests pin
  (`intermittent_crashes_with_healthy_runs_never_permanently_fail`,
  `crash_loop_without_healthy_interval_still_hits_cap_after_reset_fix`,
  `drop_kills_child_no_orphan`, the Windows tests). Rewrite in place; any removal needs an
  ADR line. Counts recorded per PR in the sprint EVIDENCE.
- **Rule 3 / I-2:** nothing in SCL signs. Generation-owned credential cleanup preserves key
  custody and the #243 keep-list.
- **Rule 7:** §11.
- **Rule 8:** zero `.unwrap()` in `src-tauri/src` and `kit/src`; every `unsafe` block
  (`pre_exec`, `waitid`, Job APIs) carries a `SAFETY:` comment.
- **Rule 12:** the shared crate needs a `[[drift]]` entry first (05 §Rule-12 drift pre-list).
- **@rule8:** see 00 "Safety gates" item 6.
