---
id: SPEC-001
slug: owned-sidecar-lifecycle
epic: E1
epicName: Owned sidecar lifecycle
epicTitle: Bound work, attest readiness and acknowledge cleanup across shared services
prefix: SC
tags: [reliability, sidecars, resource-bounds]
status: locked
adrs: [ADR-0001]
created: 2026-10-05
branch: docs/owned-sidecar-lifecycle
author: GPT-6.1 Sol via Codex, directed by the contributor
content_sha256: a01f279d5bc0ed653c326fa7484979fff6fabae71b0d3aa9448ed3a7c419b354
---

# Owned sidecar lifecycle — bounded work and truthful readiness

## Summary

**Proposed architecture; no implementation or release approval.** Extend the existing shared supervisor so each of Citrate's eight sidecars has one app-owned lifetime, generation-fenced cleanup, explicit process/readiness observations, bounded actual workers and byte-bounded diagnostics. Keep the current std monitor, manager prerequisites, binary resolver, signing/custody policies and Hermes loop boundary. The ADR is [ADR-0001](../adr/0001-owned-sidecar-lifecycle.md).

`status: locked` means the authored plan is content-locked for Knyte's loader. It does not mean the federation owner accepted it. Canonical private plans were unavailable; owner reconciliation, cross-cutting interface adoption and implementation/native acceptance remain outstanding. This contribution consists only of ADR/SPEC Markdown. Private test scaffolding and Knyte boards are not product changes.

## Motivation

The current owner already supervises child exit, backoff, intentional stop and log capture. Local actual-module fixtures add evidence beyond the passing original suite: repeated primitive Stop can respawn, startup grace can outlast the elapsed-Running retry-reset window, timed-out blocking probes can overlap, and a line-count cap does not limit log bytes. A Linux ordinary descendant also survived direct-child shutdown. These are explained by the source and bounded owned fixtures below, with consumer-reachability limits preserved.

The new contract addresses the root mechanisms rather than patching sampled timings: desired state governs respawn; fresh probe success governs health reset; actual completion governs worker permits; pre-allocation budgets govern log drains; generation ownership governs replacement/credential cleanup. None of these changes is proved implemented by this document. Future acceptance must rerun the negative properties against the actual product and add positive controls.

## GATE

The full Knyte gate covers: reuse std versus async rewrite · app lifetime versus view/durable operation lifetime · readiness and retry-reset semantics · actual probe ownership versus observer timeout · byte budgets before allocation · native containment capability · reproducible evidence and doc-format compatibility. Three read-only research lanes gathered current versioned official docs and maintained implementation patterns. A fresh independent evidence reviewer replayed source fixtures. Firecrawl Developer Index supplements source discovery (initial 17 successful non-partial keyless queries plus reopened Unix/IPC source discovery; Citrate and process-wrap were not indexed, so empty scoped results do not prove novelty or absence); only verified primary contracts/resolved fixes decide behavior. Independent architecture v1 required five revisions; v2 confirmed those mechanisms but required an execution boundary for native deadline publication. The candidate now uses a reserved std publisher and pure cached native receipt path, distinguishes committed ObservationUnknown, and bounds checked-update retention. Fresh independent v3 architecture review returned APPROVE DESIGN for the scoped proposal with no blocking design contract failure. It inspected complete candidate prose and current source, verified reproduction/dependency parity and transparent original/projection metadata handling, and independently checked pinned interprocess/updater APIs. Approval does not satisfy adoption, implementation or native acceptance. Essential future proofs include concurrent atomic arming/supersession/completion/expiry, expired A followed by later controls and delayed A delivery within fixed receipt storage, and publisher creation failure/unexpected termination preserving unavailable admission and no replacement. Torn reads must yield Unknown; these tests must not release actual await ownership just because observation expired.

PASS applies only to the surviving design contract. PARK applies to Windows Job mechanics, universal escaped-descendant cleanup, durable operations, group-message retention and full live/hardware readiness. No implementation is dispatched by this gate.

## Plan

First establish the shared lifecycle/control DTO contract. Migrate readiness/retry, then bounded probes, then bounded logs in sequence because they share the monitor source. Migrate two disjoint manager groups after the shared primitives. Integrate app admission/exit and frontend observation after both groups; add the narrowly scoped Unix containment adapter after shared-resource semantics. Preserve old status fields during consumer migration with explicit legacy meaning; remove them only after every consumer is checked. Windows reports its actual direct-child capability until a separately gated Job owner exists.

## Normative contract

### Ownership, intent and generations

There is one lifecycle owner per fixed ServiceId: Node, NodeAgent, Memory, Llama, Ipfs, Hermes, Comms, Cluster. Preserve the existing lazy-singleton initialization guards. An appEpoch identifies this application run; controlIntentSequence changes on a distinct accepted Start/replacement/Stop intent; serviceGeneration changes on committed replacement configuration; childIncarnation changes on each internal spawn, including retry; snapshotRevision changes whenever published state changes. Serialize counters as decimal strings at the JavaScript boundary and compare them numerically, for example with BigInt, never lexicographically. They are ephemeral ordering identities, not credentials or persistent replay IDs.

A short lifecycle lock reserves intent/ownership under the app admission lock, always in that order. No status/admission lock is held across HTTP, IPC, spawn, child wait, file I/O or reader join. The existing monitor is the sole process/control writer and performs prerequisite/preflight/credential work outside those locks, with completions carrying appEpoch/controlIntentSequence/generation/incarnation. Stale results cannot mutate readiness, retry counters, tokens, files or current snapshots. An old cleanup may release only the resources it created. Use generation-specific credential files where applicable; preserve current identity derivation and key custody.

Start reserves at most one pending replacement and validates its prerequisites without surrendering old ownership. Stop first commits a newer intent with desiredState=Stopped, invalidates every earlier pending Start/replacement, and suppresses automatic respawn admission in every phase, including Off and Backoff. Repeated matching Stop joins the same sequence and bounded completion. Immediately before credential publication or spawn admission, the owner checks the latest intent and app Open under the admission/lifecycle locks. A later Stop wins even when no generation/incarnation changed during validation. Superseded validation may dispose only its own unpublished resources; it cannot stop or alter a newer owner.

Spawn is an owned admission ticket registered before releasing the locks, not an OS call made while holding them. A Stop/Closing that follows commitment may precede the physical spawn returning. That already admitted ticket remains counted; its eventual child goes directly into matching teardown, never Ready or an unowned empty manager slot. Likewise an admitted file write may finish late, but its private generation path cannot become the active credential reference and is cleaned by its ticket. Complete waits for these tickets. The guarantee is no new admission after Stop/Closing, not that the OS cannot finish a previously admitted call. A replacement cannot commit while previous teardown/tickets remain. Incomplete cleanup retains the reservation and rejects new work as Quarantined.

The control mailbox has one latest-intent cell and one coalescing wake bit, with no payload/task queue. A wake send failure leaves committed intent visible and returns ObservationUnknown; the owner also checks the shared intent at bounded scheduling points. At most one validation/preflight executes per service, on its retained monitor, and at most one replacement descriptor is stored (argv/config/metadata together at most 64 KiB; oversized input is rejected). A newer replacement supersedes that descriptor; it does not start another worker. Probe completion has its own single occupied result slot. Receipt retrieval never registers native waiters.

| Incoming intent | Existing ownership | Required transition |
| --- | --- | --- |
| Start, app Open | Stopped, no outstanding resources | Reserve new sequence; validate; commit one spawn ticket only if still current. |
| Start/replacement, app Open | Active or draining owner | Keep old resources; reserve/coalesce one replacement; commit only after validated current intent and complete teardown. |
| Stop | Pending validation, any phase | Advance sequence to Stopped; earlier completion becomes Superseded; close spawn/publication admission before wake. |
| Repeated matching Stop | Stopping, Stopped or incomplete cleanup | Join current receipt; never respawn or allocate another teardown worker. |
| Start | Quarantined | Reject until actual outstanding work ends and retained ownership is reconciled; timeout alone never frees it. |
| Any new start/retry/probe admission | App Closing | RejectedClosing; pre-existing tickets remain owned by the close intent. |

Control results are Accepted(token), AlreadySatisfied(snapshot), RejectedClosing, Busy(resnapshot), Quarantined(reason), or ObservationUnknown(token, reason). Busy means this call committed no intent; it does not authorize replay of an earlier uncertain call. ObservationUnknown is the explicit committed-intent DTO/error channel when wake/publication observation failed: preserve its token and ownership, with a bounded reason. Legacy Start/Stop translate it to a structured observation-unknown error carrying that token, never success or automatic retry. Accepted does not attest completion. Keep at most one active and one completed control receipt per service; coalesce repeated intent rather than storing unbounded waiters. A superseded token yields Superseded if still retained, otherwise Expired/resnapshot, never an invented success. Observer timeout leaves intent and ownership unchanged. Stop acknowledgements distinguish Complete(ownedScope) from Incomplete(remainingResources, observationReason). Quarantine recovery requires actual worker completion, matching ticket cleanup and a fresh explicit Start; no timed-out permit is reset by allocating a new cell.

A per-service Stop retains its idle app-owned monitor; its Complete scope names child/group, readers, probes, admitted foreground work, preflight/tickets and credential cleanup. App Closing additionally drains/joins the monitors and crash sink. Complete requires actual joins/reconciled ownership, not is_finished alone or a received result before worker exit. The final coordinator and deadline-publication actors/framework runtime are outside the named sidecar-drain scope. Nonprogress retains their reserved slots. Late resource completion updates observation, but an expired close does not authorize a late final action; an explicit retry is required.

Reserve one app-owned std deadline-publication thread before lifecycle control admission, separate from the async and blocking pools. A creation failure leaves admission unavailable and reports an initialization error; repeated close/Stop/timeout never allocates a replacement actor. It observes a fixed nine-slot deadline overlay (eight service controls plus app close), keyed by app epoch and current control-intent sequence, with one active record per owner and no queue. Arm the absolute monotonic deadline in the short native admission path before any validation, I/O, teardown or joins. The guarantee begins at actual admission, not at a renderer click waiting to execute.

The publisher marks native DeadlineExpired/Incomplete without acquiring lifecycle/owner/detail locks, performing I/O, joining, serializing, emitting events or polling runtime futures. Use coherent fixed atomic identity/deadline/expiry records; bounded reads that cannot establish current identity yield ObservationUnknown, never another generation's expiry or Complete. Deadline detail is optional cached information: unavailable owner observations remain Unknown. The publisher's wait/park loop rechecks the absolute monotonic deadline after wakeups, with an initial maximum 50 ms observation cadence; wakeups cannot restart a relative budget. It does not stop resources or grant exit authority.

New control admission and cached receipt/snapshot commands are synchronous, bounded native operations with no blocking service/file/transport work. Use bounded try-lock/atomic reads; contention returns Busy/ObservationUnknown rather than waiting on an owner. This gives the native receipt path an execution boundary separate from Tauri's async workers. It does not promise renderer delivery while the UI thread, IPC dispatcher or renderer cannot run. Legacy async await slots and JS promise delivery remain scheduler-dependent; after delivery resumes they must expose the retained expired receipt, not invent a timely success. The native publisher itself requires OS scheduling; this is not a hard real-time guarantee. At its first scheduled check at/after expiry it publishes the native result, and receipt reads/final-action admission also compare the monotonic deadline directly so a delayed timer cannot authorize action.

Final exit/restart/install admission requires matching intent, reconciled Complete and a nonexpired authorization; recheck at the actual native final-action admission boundary, not only before queueing work on the UI/runtime. An action already admitted before expiry can physically finish later, like an owned spawn ticket; no new final-action admission follows expiry. Actual resource completion discovered after expiry remains observational until an explicit retry authorizes the action. Accept this boundary with held async workers, a saturated blocking pool, stalled owner locks/joins, repeated close, timer wakeups, and delayed caller delivery. Broad foreground adapter migration is separate: applying off_main alone adds no admission bound and merely moves accumulation to Tokio's blocking queue. Memory/Cluster native ownership gates remain PARKED; neither their adapters nor general UI availability is certified here.

### Process state, readiness and retries

Lifecycle phase is Stopped, Starting, Alive, Backoff, Failed, Stopping or Quarantined. Readiness is independently NotAssessed, Awaiting, Ready, Stale or Unavailable and carries attestation scope and monotonic freshness. Alive proves process existence only. Comms/Cluster connect-only probes attest IPC connection, not all application behavior; node synchronization, relay reachability, memory semantic readiness and Hermes session completion remain separate facts.

For a configured application probe, the first successful current-incarnation result plus existing prerequisites makes readiness Ready immediately; it does not grant retry-reset credit. Record monotonic completion time. Initial proposed freshness/max-success-gap is two configured probe intervals (10 s for the existing 5 s profiles); an intervening failure, observer timeout, unavailable observation or gap beyond that limit makes readiness Unavailable/Stale and ends the successful-health window. Grace tolerates failed/unknown observations; it never earns health credit or makes Ready. Reset the consecutive retry budget only when successful samples span healthy_after without such a gap; process lifetime alone cannot reset it. Reset credit is granted once per uninterrupted window. Re-gate a service-specific freshness change rather than silently extending it.

For a service without a configured probe, retain the explicit liveness-only reset policy and its current duration. Publish NotAssessed; do not silently infer application Ready or introduce a new expensive polling policy. Any new readiness probe requires its own bounded, service-specific acceptance. Define max_retries as retries after the first failed incarnation; separate initial failures, attempts, completed respawns and retry budget in new counters. Preserve the old `restarts` field only as a documented recorded-failure counter during compatibility migration.

Use actual monotonic elapsed time, not the sum of requested sleeps. Wall-clock timestamps are for diagnostics only. A backward wall clock cannot extend deadlines or grant healthy credit. A failed/unknown incarnation consumes at most one failure before its teardown/retry transition. Explicit Stop never consumes failure budget.

### Compatibility and execution gates

Migrate native and bundled frontend together. Preserve existing fields, but deliberately extend the lifecycle state vocabulary with `stopping` and `quarantined`; there is no lossless old five-string representation. Update every DTO/type/mapper/action consumer, including mapNodeState's current unknown→off fallback. Do not label retained teardown `stopped` or a cold live process `healthy` to preserve an old display. Keep process-existence/has-owned-resources methods distinct from scoped is_ready; ensure_started must not start a second child merely because a live cold child is not Ready.

| New authoritative phase/readiness | Legacy state where present | Process existence / healthy / action meaning |
| --- | --- | --- |
| Stopped | stopped | No owned resources from the completed control; healthy false. A new explicit start may be admitted while app Open. |
| Starting | starting | Reservation/ticket may exist; healthy false; no readiness-dependent action. |
| Alive + Awaiting/Stale/Unavailable | running | Child exists; configured-probe healthy false; readiness-dependent action denied. No implicit replacement because Ready=false. |
| Alive + Ready | running | Child exists; scoped healthy true only while current/fresh. Existing domain/identity/ceremony checks still apply. |
| Alive + NotAssessed | running | Process-only profile remains usable through its existing liveness/domain/transport checks; no fabricated generic healthy field. |
| Backoff | restarting | No eligible active incarnation; healthy false; only current Running intent may admit a retry. |
| Failed | failed | Retry budget terminal; healthy false; a new explicit Start requires complete old teardown. |
| Stopping | stopping | Owned resources/tickets may remain; healthy false; new work denied until teardown/explicit queued intent is reconciled. |
| Quarantined | quarantined | Owned resources remain; healthy false; starts/actions denied; late actual completion can resolve ownership, not silently restart. |

New controls expose Accepted receipts. Legacy Start keeps its previous returned DTO/result semantics after matching spawn admission succeeds, with honest cold readiness; it never promises Ready. Legacy Stop observes Complete for its named owned scope under the same absolute deadline and returns a structured cleanup-pending/observation-unknown/superseded error otherwise, never Ok merely because intent was Accepted. Native expiry is independent; async response delivery can resume later and must preserve that expired result. A compatibility observer has at most one retained async await slot per service (eight total, using the existing runtime and no blocking helper thread); excess native calls return Busy/resnapshot. Frontend repeated controls join one raw promise. Observer expiry expires the observation; an actual compatibility await slot stays occupied until that native await task actually returns or its cancellation is reconciled. Logical timeout alone frees neither actual await ownership nor service/probe/spawn ownership. The node watchdog must consume terminal teardown before restart and must not continue after Stop error.

The action migration inventory covers all 61 Tauri commands in the eight manager modules plus adjacent AI/storage/invite consumers. These are the load-bearing categories; implementation acceptance must check every command in that inventory, including direct calls that bypass UI selection:

| Owner | Required gate and preserved independent facts |
| --- | --- |
| Node | Process-only NotAssessed permits existing bounded RPC sampling; unavailable height/tip remains unknown. Keep follower start without wallet, persisted proposer identity, coinbase+fresh caught-up comparison for mining, validator/earnings/ceremony requirements. Migrate shell node-state mapping and watchdog stop/restart receipt handling. |
| NodeAgent | Process-only NotAssessed; authed remains bearer-present, not readiness. Daemon HTTP operations require current owned bearer and actual response validation. Public-chain user-claim ceremony construction must not gain an unrelated daemon-Ready requirement. |
| Memory | Process-only NotAssessed; semantic remains configured model capability, not attested engine readiness. Preserve Running+semantic gates for ingest/seed, Running for constellation, and actual transport/error propagation for recall/assert/search/neighbors. Migrate offline/running UI mapping without inventing Ready. |
| Llama | Fresh current-incarnation Ready plus verified model/native endpoint/key ownership gates inference routing, direct ai_chat_local_sync, direct ai_chat_local_tools_sync and Hermes session's local provider selection. Replace serve.rs's is_running→server_healthy inference input; updating only UI inference-state is insufficient. In-flight session cancellation remains its existing separate contract. |
| IPFS | Process-only NotAssessed; preserve binary/init ownership and actual Kubo transport errors for storage add/pin/list/retrieve/unpin, CID/index/ceremony checks and node startup's best-effort optional IPFS behavior. A bool running is not storage availability. |
| Comms | ensure_started checks existing ownership/process intent, not Ready; a cold admitted request may wait boundedly for current connect-scope readiness before bearer-authenticated IPC, then handles actual response errors. Apply to group membership/roster/roles/send/poll and invite routes. Identity-only and relay-observation commands remain no-start observations; relay degradation never restarts the daemon. |
| Cluster | Preserve shared Comms identity/roster/admission checks. ensure_started keeps owner semantics; IPC actions require fresh connect-scope readiness then actual transport results. Membership online/peer counts/sharedFiles are domain observations, not process readiness; status/peers/join retain their roster prerequisites. |
| Hermes | Fresh current-incarnation health plus owned bearer gates new control/session admission; remote status remains separately sampled/cached, not manufactured from process state. Preserve skill/approval/brief/track/session input checks, replay/reattachment and ceremony decisions. Local session provider additionally requires Llama Ready. Shared-service Stop must not become session Stop. |

Snapshot polling does not perform these domain calls or change their authority; foreground operations and separately bounded domain observers retain real errors. A healthy transport check is necessary only for its declared availability scope, never sufficient proof of a successful business operation.

### Finite actual workers and transports

Provision at most one actual in-flight health worker per service **across generations**, not one per newly allocated supervisor. A timeout publishes an unavailable observation, but its permit stays occupied until the worker ends. Late completion is discarded unless all identity fields match. No replacement worker or queued task can bypass that permit. A noncooperative worker produces Quarantined/Incomplete; cancellation is a request, not a promise that an arbitrary Rust closure can be interrupted.

The default HTTP health adapter must supply a synchronous literal-only resolver to ureq 3.3.0 using its public low-level `Agent::with_parts(config, TcpConnector::default(), resolver)` API. Construct it from a prevalidated loopback SocketAddr with nonzero port and canonical matching HTTP URI authority. Its `Resolver::resolve(uri, config, timeout)` rejects a different scheme/authority/proxy and returns only that stored address through `ResolvedSocketAddrs`; it performs no DefaultResolver call, ToSocketAddrs, DNS, I/O or thread creation. Merely putting 127.0.0.1 in a URL is insufficient: the pinned DefaultResolver spawns a helper for finite global timeouts even for literals. Retain all socket work in the admitted probe worker.

Use `.proxy(None)`, `.max_redirects(0)`, `.timeout_global(Some(Duration::from_secs(3)))`, 8 KiB response-header/input/output limits and zero idle pooled connections. Existing llama/Hermes probes need HTTP status only: do not instantiate/read an unbounded body decoder; drop the response/socket after the bounded headers. A future body-based health profile must separately bound wire/decoded bytes (initial 4 KiB body budget) and decoder state before admission. Keep native endpoint/auth values redacted. The 3 s global setting is a request observation budget, not a hard OS completion promise: pinned NextTimeout converts zero remaining time to 1 s for socket calls, and OS progress is not cancellable by a Rust token. Actual permit retention/quarantine remains necessary.

Because the resolver interface is explicitly unversioned, pin both direct ureq constraints to exact 3.3.0 for this adapter and keep the lock; any later patch/minor adoption re-gates its resolver, timing, buffer and connector source/native checks. This is an existing dependency constraint change proposed for SC3, not an installed product change or upgrade. A DNS/proxy/custom transport must separately own/bound hidden work; no default-agent fallback is permitted.

IPC adapters need actual finite ownership and qualified connect/read/write behavior on each platform; named-pipe timeout-setting errors do not establish I/O deadlines. Connect-only health adapters retain their limited scope and actual probe permit. Bound response decoding/buffers. An unqualified adapter publishes incomplete/observer-only timeout semantics and cannot automatically replace its owner because its caller deadline expired.

The new upstream 30c 789e Windows Comms foreground helper needs a separate actual-work bound: admit at most eight Comms request slots manager-wide across generations, before endpoint preparation/connect; store no queued payloads. Each slot owns its worker, current-incarnation credential reference, buffers and complete transport lifetime. Busy rejects before dispatch; observer timeout retains the slot/JoinHandle/resources. Stop/Closing includes these obligations and admits no more calls. All eight may remain occupied indefinitely; honest Busy/quarantine is preferable to growing thread/handle population. Eight is a proposed capacity that preserves ordinary parallel roster/drain better than one, not a measured optimum. Initial encoded request/response caps are 4 MiB each per slot, streamed before copy/parse (64 MiB payload maximum across eight slots), plus at most 8 KiB reader buffering/4 KiB read chunk per slot. Validate real group/invite workloads before adopting those limits. A request that was dispatched before an error may have taken effect; neither timeout, Busy on a different call nor cancellation authorizes automatic replay of mutation/mailbox drain. The separately PARKED group-retention issue remains, and normal parallel polling/retention regression acceptance is required.

Joining that worker alone is insufficient on pinned interprocess 2.4.4. Its Windows pipe Drop can transfer unflushed handles to a global linger pool with detached flush workers; current try_clone marks both handles Always-flush. For the selected Windows Comms adapter, use sequential borrowed Read/Write on one IpcStream without clone/split. Keep the transport-owning guard outside catch_unwind while work borrows it. Once all I/O actually finished, on success/error/unwind consume the public Stream::NamedPipe wrapper into DuplexPipeStream<Bytes>, then use fallible OwnedHandle::try_from(pipe); the successful consuming conversion bypasses the pipe destructor/linger pool. Close that owned handle through its explicit ownership path. If conversion returns a shared pipe, retain it in the occupied service slot as Incomplete; do not drop it into limbo, panic with expect, or fabricate completion. No default-drop/error branch may silently transfer ownership. The slot is released only after worker join and reconciled transport/resource closure. No flush is needed to claim resource closure, and closure does not prove message delivery.

The pinned pipe implementation uses FILE_FLAG_OVERLAPPED and ReadFileEx/WriteFileEx completion routines. Current CancelSynchronousIo cannot be credited with canceling that path. Any best-effort cancellation requires separately native-tested appropriate APIs and lifetime-protected owned handles; even a successful CancelIoEx is a request, not completion. Never force-close handles or release I/O buffers while an operation remains pending. Observer-only timeout plus retained slots is valid when cancellation is unqualified. Windows actual transport deadline/cancellation/handle-release acceptance remains PARKED; this source-verified no-clone consuming route is a design candidate, not a tested backend. Memory/Cluster clone the same library and need their own adapter/native ownership gate; this choice does not silently fix them.

The promised resource set includes the immediate child, its two capture readers, actual probe work, manager-owned credential cleanup and accepted preflight subprocesses. Adopt deadline-aware, byte-bounded ownership for IPFS init/config rather than unbounded `output()`. A stuck preflight owns the service slot; another cannot accumulate. Keep bearer/status mutex hold times short by copying the needed credential reference for a bounded call, without exporting key material. This is a static risk to test, not an already-replayed manager incident.

### Bounded diagnostics

Truncate while reading bytes before forming an unbounded String. Initial proposed limits per service: 16 KiB partial/retained line; 256 KiB retained log text and 500 entries; 64 KiB serialized snapshot. Continue consuming oversized/no-newline output, mark truncation and count dropped bytes/records. Invalid UTF-8 is represented with bounded replacement/byte diagnostics. Never block pipe draining indefinitely on a slow UI/event consumer. Finite poll work preserves control responsiveness under continuous output.

At most two log snapshot serializations are admitted app-wide. All byte limits count encoded UTF-8/JSON bytes including truncation markers/escaping, not Rust character count; reject/truncate bounded fields before encoding. Limit one reader's partial buffer to 16 KiB and its read chunk to 4 KiB. Limit reason strings to 1 KiB and status metadata to 4 KiB/service. One app crash sink accepts at most 64 queued records of 8 KiB each plus one 8 KiB active writer record; saturation drops diagnostic records with a visible counter, not a monitor block or a new writer thread. Each service has two rotated 16 MiB crash files; rotate in place within that two-file envelope, with no full third temporary copy. Unwritable disk, encoding or a wedged filesystem produces degraded diagnostics and a bounded incomplete receipt. These logs are diagnostics, not a financial/audit ledger.

The supervisor-owned thread budget is at most 35: 8 retained monitors, 16 readers, 8 probes, one crash sink, one shutdown coordinator and one independent deadline publisher; selected Windows Comms adds at most 8 actual foreground workers, making 43 for that combined scope. The fixed deadline overlay has nine records of at most 128 bytes each; no per-caller timer thread or queue is admitted. Qualified explicit-close adapters must not create hidden linger workers. Validation/preflight/credential work uses the monitor, not an uncounted helper; one sequential owned preflight child replaces the service-child slot, never overlaps another preflight/main child. This is not a bound on all Tauri/runtime/domain requests or total RSS. Retained ring text is at most 2 MiB; reader partial/read chunks total 320 KiB; two snapshots 128 KiB; crash queue 512 KiB plus active 8 KiB; pending descriptors 512 KiB; status metadata 32 KiB; Comms foreground payloads 64 MiB plus 96 KiB reader/chunk buffers. Transport/parser/object/allocator overhead must also be measured. A reader/sink/monitor/control worker that cannot finish retains its slot; no restart allocates a replacement before actual completion. Reader handover waits for prior readers, so generations do not double the 16-reader budget.

Numeric defaults are proposed acceptance budgets, not measured optimal values. Packaged noisy-output/cold-load benchmarks must validate them or re-gate a change. Test retained bytes and parser/snapshot maxima directly; allocator/platform overhead means these text budgets alone cannot establish a total-RSS ceiling.

### Shutdown and frontend observation

Atomically move the app to Closing before collecting owners and outstanding admission tickets. Closing rejects new start/retry/probe admission and delayed frontend startup intents, including lazy singleton creation; already committed spawn/file work follows the ticket rule above. Start all existing owner drains concurrently under one 15 s proposed absolute monotonic app deadline. Preserve the current 5 s graceful child-stop window within that deadline; escalation, reaping, reader/probe checks and cleanup consume its remaining budget rather than resetting independent 15 s waits per owner. Expiry makes the native receipt Incomplete under the independent publication/monotonic-read contract while reservations remain owned. OS scheduling and caller delivery remain explicit limits; this is not a promise that the OS finished killing a child or delivered a JS response within 15 s.

Do not block Tauri's exit callback waiting for that drain. Normal quit prevents ordinary exit and schedules the coordinator; it exits with a fixed ordinary code only after Complete. All application-controlled quit/restart/update-install entry points use the same app-close intent. Expose native coordinated Quit/Restart commands, not a caller-supplied exit integer. Remove `process:default`/raw allow-exit/allow-restart from capabilities; pinned process 2.3.1 calls request_restart, whose special exit code makes prevent_exit a no-op. Tauri 2.11.5 main-thread restart may also skip exit events. A final native request_restart/restart is allowed only by the coordinator's terminal Complete branch, never as a way to trigger drainage. Repeated ExitRequested/Exit joins the intent; terminal re-entry cannot recursively drain.

Updater install is an exit path too. Pinned updater 2.10.1 Windows installation invokes a synchronous cleanup hook then std::process::exit(0), bypassing RunEvent. Remove updater:default and raw check/download/install/download-and-install exposure; expose only the coordinated native update commands. Migrate `src/shell/updater.ts`'s current check, downloadAndInstall, relaunch and critical-update paths to native coordinated commands. The native check wrapper admits one actual check at a time, with no queue or replacement on observer timeout, and calls the public UpdaterExt/Updater::check API with existing feed/platform/version policy. Retain at most one cached checked Update in native app state, bound to its calling webview and an opaque check generation; return only display metadata/token, not a plugin resource-table rid. This public Rust check route does not add a webview Update resource as the plugin JS check command does. Replacement or no-update clears/drops the old cached handle only when no admitted download/install owns it. View disposal drops the view reference, while the app-scoped cache/actual check retain their declared ownership; explicit cache disposal/terminal request closure clears that ownership. A stale check completion cannot overwrite a newer token or retained install state. Rechecks while a request is active/Closing return Busy; after Closing only retry of the retained selected update is allowed. Native state owns at most one additional cloned Update and one downloaded payload for that single active request; no new clone/resource-table/payload per observer or retry. Request cancellation/error explicitly disposes or retains the same retry slot, and terminal disposition releases it. Installer payload/manifest overhead is outside the sidecar-buffer subtotal and requires packaged measurement; no total updater/RSS cap is claimed. Migrate/dispose any legacy JS Update wrapper with its public close method instead of losing its resource id. Preserve normal check/error/progress/dismiss/critical behavior and test replacement, no-update, view disposal, late checks and installer retry.

Use the selected public Update download API and existing signature/feed/platform rules, then close admission/drain before invoking its public install API. No caller-supplied installer path/bytes or new automatic-update policy is added; preserve the accepted critical-update trigger. The existing cleanup hook is insufficient and must not block the main thread to emulate this sequence. Progress stays observable; canceled/failed download does not close the app. Incomplete drain must not call install/restart/exit; it returns an honest cleanup-pending result. A later installer failure leaves services drained and reports a usable error; no implicit reopen/spawn occurs in the Closing epoch. Retry-install or coordinated quit/restart joins the retained terminal app intent. The single-instance plugin's early secondary-process rejection remains before app setup/service admission, with no active service resources to drain; acceptance must retain that ordering.

Force exit, external task termination, crashes and OS logout that cannot await drainage remain outside Complete. Any explicit force-exit policy needs owner adoption and must show incomplete cleanup rather than claim no survivors. This design does not alter updater signing/feed or extend ceremony permissions.

Remove executable-directory/name-wide orphan killing from startup. This source-only hazard was not exercised because it can affect unrelated workspace processes. Current-run handles/verified containment own cleanup; occupied endpoints or unknown leftovers surface an error. Persistent crash recovery/recapture is parked; it cannot be simulated by broad pkill/pgrep matching.

Native state owns services through navigation. Reuse the existing shell/store poll pattern with one native snapshot request in flight across observer epochs, at a proposed 5 s cadence plus a coalesced control-result refresh. Native snapshot reads copy cached lifecycle cells and perform no service I/O: do not aggregate the current Node/Comms/Hermes status methods that call RPC/relay/control endpoints. Retain the poll slot until the underlying raw Tauri invoke promise actually settles; the existing timeout-raced invoke wrapper is not that promise. An observer may time out/invalidate its epoch while the app-scoped raw request and slot remain occupied. React subscribes synchronously to the immutable cached store snapshot; accept matching epochs and numerically newer revisions only. Reconnect/resume schedules one coalesced resnapshot. This choice avoids adding native event-listener churn: 2.11.5 has a verified early-unlisten race even for await-listen then one unlisten, fixed in 2.12.0. A push bridge/framework upgrade requires separate adoption/acceptance; no delay/private-internals workaround is proposed. StrictMode repeated setup/cleanup cannot duplicate native start, subscriptions or timers. Track frontend startup timer handles and an observation epoch so late continuations cannot restart a stopped store or admit work after Closing. View-owned inference/session cancellation remains separate and is not extended by invoke timeout.

### Containment scope

Unix opt-in profiles create an owned session/group before execution (SID=PGID=owned leader PID) and retain the unreaped leader as the lifetime anchor through final group actuation. Rust 1.93.1's CommandExt::setsid is unstable; use stable unsafe pre_exec with only the existing libc::setsid async-signal-safe syscall and immediate native errno return on failure. No logging, formatting, environment access, locks, allocating error construction or panic is allowed in that child callback. Do not also call process_group(0): Rust runs setpgid before the callback, which would make setsid fail. Successful spawn records the known SID/PGID identity rather than rediscovering an exited leader using getpgid/getsid (macOS may hide zombie leaders from those queries). A new session prevents an unrelated pre-existing same-session process joining this group and prevents the direct session leader changing its group. This callback forces Rust's fork+exec fallback rather than posix_spawn; measure cold-launch latency/transient memory and resource-limit failures on the initialized multithreaded host before opt-in. No speed improvement is claimed for this initializer.

The adapter is the sole reaper: no Child::try_wait/wait, broad waitpid reaper, SIGCHLD=SIG_IGN or SA_NOCLDWAIT may consume that child earlier. The app must verify/document process-wide assumptions before enabling the capability. A mutex protects participating code only; an incompatible independent reaper makes the profile unsupported, not safely fixed by checking a numeric PID. Retain the Child and anchor in the lifecycle cell, including after observer timeout.

Poll typed libc waitid(P_PID, ownedPid, ..., WEXITED|WNOHANG|WNOWAIT). Zero the entire platform-native siginfo_t for every poll; rc=0 alone is not exit. Require matching si_pid, SIGCHLD and CLD_EXITED/KILLED/DUMPED; use native Linux/macOS layouts/accessors, never a copied ctypes structure. Pending status is distinct from terminal status. Actuation relies on the proven successful spawn session identity and retained child lifetime, not a late numeric getpgid query or a ticket whose spawn has not returned. Unexpected known membership, ECHILD, permission/observation errors or violated reaper assumptions close actuation and produce Incomplete, with no retargeting to another discovered group. Under the supported contract the session leader cannot leave its group and its unreaped lifetime prevents PID/PGID reuse during signals.

The monitor serializes group TERM, the remaining graceful window and at most one final group KILL under the same cleanup deadline. Even if the leader exited earlier, WNOWAIT retains its anchor until the last possible signal. Group-signal success does not attest exit. Permanently close this generation's actuation before consuming terminal leader status; then reap nonblockingly. If leader terminal status/reaping cannot complete by the deadline, retain the anchor/slot and report Incomplete. Never signal TERM/KILL to that numeric PGID after the anchor was consumed, including later Stop or cleanup retries.

After reaping, only signal-zero observation is permitted. On supported Linux normal signal-visibility semantics and 64-bit macOS libc UNIX 03 semantics, an observed ESRCH establishes group absence at that instant. Positive result, EPERM, unexpected errno, ambiguous platform semantics or expiry means Incomplete; macOS zombies-only groups can return EPERM and Linux zombies can yield success, so they conservatively delay completion. A subsequently reused unrelated PGID may cause a false-incomplete observation; it must never receive actuation. Do not use legacy Darwin/raw syscall semantics, or treat signal-zero as a universal membership census. Complete requires this absence observation plus all named reader/probe/preflight/credential/ticket resources complete. It attests the closed generation's ordinary cooperating group at the recorded observation, not perpetual future absence.

Descendants that change groups/sessions are excluded from this capability; an escape fixture must prove the claim is explicitly scoped, not promise that every escaped process is detectable after closing pipes. No universal no-orphan guarantee is made. Native Linux/macOS cases must verify running-child zeroed polling, leader-before-descendant exit, final-signal-before-reap order, retained-anchor ID reservation, concurrent Stop, known ownership-loss/error/permission paths, no actuation after reaping, conservative group absence and inherited pipes. Each actual sidecar profile additionally needs compatibility acceptance before opt-in. Quarantined ownership remains finite; recovery needs actual scoped absence and worker/ticket completion, never a fresh owner or numeric-ID recapture.

Windows retains explicit direct-child capability until a separately defended/native-tested owner captures before execution via suspend→owned Job assignment→resume, disables breakaway, configures kill-on-last-handle-close and retains handles for captured ordinary descendants, observes their terminal state and reconciles authoritative Job accounting within the deadline. Do not equate process-wrap 10.0.1 std wait/try_wait or arbitrary completion messages with Job emptiness. This separate backend/dependency decision is PARKED; core readiness/resource work must not claim cross-platform tree cleanup. Packaged release approval requiring that guarantee remains blocked until reopened.

## Existing service profiles to preserve

| Owner/source | Current observation and important prerequisites |
| --- | --- |
| [Node](../../src-tauri/src/node.rs) | No supervisor probe; liveness reset 90 s; RPC calls 6 s each; synchronization distinct from process existence. |
| [Node agent](../../src-tauri/src/agent.rs) | No supervisor probe; default reset 30 s; authed currently means bearer present; HTTP has no explicit global deadline here. |
| [Memory](../../src-tauri/src/memory.rs) | No supervisor probe; reset 30 s; Unix send/receive 10 s, excluded on Windows; declared 5 s connect constant has no use found. Preserve encrypted-store and semantic/lexical distinctions. |
| [Llama](../../src-tauri/src/serve.rs) | /health 5 s cadence, 3 s global HTTP deadline, 180 s grace, 60 s reset; preserve verified-model/port checks and replacement's target validation before old stop. |
| [IPFS](../../src-tauri/src/ipfs.rs) | No supervisor probe; reset 30 s; initial init/config subprocess ownership must become bounded. |
| [Comms](../../src-tauri/src/comms.rs) | Connect-only IPC 5 s/20 s grace/30 s reset; request connect retry 3 s. Unix send/receive settings 5 s; the Windows helper has a post-connect observer deadline/cancel grace that need not finish actual I/O. The 750 ms relay observation is separate. Preserve current wallet-derived scoped identity. |
| [Cluster](../../src-tauri/src/cluster.rs) | Connect-only IPC 5 s/20 s grace/30 s reset; Unix I/O 5 s, Windows excluded; identity equals Comms, with existing transport/admission gates. |
| [Hermes](../../src-tauri/src/hermes.rs) | /health 5 s/3 s bound/20 s grace/30 s reset; control HTTP 30 s, session long-poll 20 s. Shared-service status is separate from accepted session replay/reattachment. |

The prior full-modular review also covered wallet/ceremony, identity, staking/distribution, model download/storage, memory, training, group transport, updater/build/dependencies and formal/source tests. Their role in this contribution is boundary preservation and residual coverage, not invented successful runtime evidence.

## Stories

- **SC1 — Define lifecycle ownership and acknowledged controls.** Establish intent sequence/generation/incarnation/revision, latest-intent wake mailbox and registered spawn/publication tickets. VERIFY synchronized repeated Stop in Off/Backoff/Stopping; pending validation followed by Stop; conflicting replacement; spawn returns after Closing; send failure; stale completions; bounded descriptors/receipts and no native waiters. **Depends on:** — · **Touches:** `kit/src/supervisor.rs`, supervisor unit fixtures, shared status/control types.
- **SC2 — Separate readiness and repair retry credit.** Preserve per-service profiles and liveness-only behavior; credit configured probes only after a fresh successful window. VERIFY always-false/late/stale probes, grace longer than healthy_after, successful-health reset, process-only profile, monotonic clock jumps and exact retry semantics. **Depends on:** SC1 · **Touches:** `kit/src/supervisor.rs`, supervisor unit fixtures.
- **SC3 — Bound actual probe ownership and adapters.** Keep permits across timeouts/generations; use explicit synchronous literal resolver/TCP connector, exact ureq pin, constrained HTTP/IPC and bounded decoding. VERIFY held workers across timeouts/replacements, permit return only on actual completion, no default resolver/fallback/helper thread, proxy/authority/redirect/header/body limits, per-platform deadlines and quarantine recovery. **Depends on:** SC2 · **Touches:** `kit/src/supervisor.rs`, bounded probe adapter module, native adapter fixtures, `kit/Cargo.toml`, `src-tauri/Cargo.toml`, workspace lock when required by the exact existing version constraint.
- **SC4 — Bound capture and crash diagnostics before allocation.** Add streaming truncation, ring/snapshot admission and bounded crash sink/rotation. VERIFY no-newline output, 1 MiB line, alternating stdout/stderr, invalid UTF-8, slow consumer, unwritable/full disk and blocked sink; observe counters, byte envelopes and stop progress. **Depends on:** SC3 · **Touches:** `kit/src/supervisor.rs`, diagnostic sink/capture fixtures.
- **SC5 — Migrate Node, agent, memory and IPFS owners.** Keep prerequisites and custody, serialize credential cleanup/replacement, own bounded IPFS preflights, and expose actual readiness scope. VERIFY each manager using owned fixture processes/transports, held cleanup barriers, old-generation credentials and command compatibility. **Depends on:** SC4 · **Touches:** `src-tauri/src/node.rs`, `agent.rs`, `memory.rs`, `ipfs.rs`, their native fixtures.
- **SC6 — Migrate Llama, Comms, Cluster and Hermes owners.** Preserve cold-load/identity/session scope; generation-fence shared files and status; retain Comms foreground slots through actual connect/I/O and explicit no-clone transport closure. VERIFY 180 s-grace/health credit, held cleanup, stale results, retry/legacy translation, all-eight control saturation and ordinary group concurrency, no linger transfer on success/error/unwind, no replay after unknown delivery. Windows real cancellation/transport and other IPC adapters require their stated native gates; no fake native PASS. **Depends on:** SC4 · **Touches:** `src-tauri/src/serve.rs`, `comms.rs`, `cluster.rs`, `hermes.rs`, their native fixtures and selected Windows Comms ownership adapter.
- **SC7 — Coordinate app close and one shell observer.** Close admission before concurrent drains, remove broad startup orphan killing, route quit/restart/update install through the coordinator and fence raw observation/timers. VERIFY eight-owner native deadline publication, small async executor occupied by held foreground work, blocking-pool saturation, stalled owner locks/joins, eventual receipt delivery with expired authorization, nine-slot publisher accounting, update check/cache/clone disposal, repeat exit, raw check/special-code/restart/install capability unavailable, main-thread restart and Windows install only after Complete, late spawn tickets, StrictMode, delayed raw snapshots/epochs, numeric revisions and reconnect. **Depends on:** SC5, SC6 · **Touches:** `src-tauri/src/lib.rs`, native coordinated close/update commands, `src-tauri/capabilities/default.json`, `src/shell/updater.ts`, `src/shell/store.ts`, `src/App.tsx`, bridge/types and UI consumers.
- **SC8 — Add scoped Unix descendant containment.** Retain unreaped leader via typed zeroed WNOWAIT polling until final group actuation; close actuation permanently before reap; require conservative supported-platform absence plus resource completion. VERIFY ordinary descendant/leader-first exit, reap-order/ID reservation, concurrent Stop, lost ownership/errors/deadline, inherited pipes, no later numeric actuation and explicitly excluded escape on Linux/macOS. Actual profiles remain opt-in pending compatibility tests; Windows remains direct-child. **Depends on:** SC4 · **Touches:** `kit/src/supervisor.rs`, Unix ownership adapter and Linux/macOS native fixtures.

Do not dispatch these stories until the owner reconciles canonical plans and adopts the significant contracts. Shared-file stories are sequential; SC5/SC6 are disjoint lanes. This is a plan, not completed implementation.

## Acceptance and empirical evidence

Future implementation acceptance requires all eight consumers, not only the isolated module. Keep existing tests/counts and add invariant-oriented properties rather than source-string mirrors. A missing fixture acquisition/acknowledgement is Inconclusive, never a pass. Each test cleans every process/worker it creates even when its property fails.

| Obligation | Negative case and positive control |
| --- | --- |
| Stop idempotence/no respawn | Repeated Stop at each phase and after Off; synchronized monitor receipt/spawn count. Positive: explicit new accepted Start creates exactly one new incarnation. |
| Generation isolation | Hold old cleanup/probe, request replacement; no old result/file/token mutation. Positive: current-generation result updates once. |
| Health credit | All probes false/stale during long grace; initial failure plus finite retries then Failed. Positive: sustained successful fresh samples reset the counter; liveness-only profiles remain NotAssessed. |
| Actual work bound | Hold one worker through many observer timeouts and restarts; active<=1 per service across generations. Positive: release it and verify the slot can be reused. |
| Log/storage limits | Oversized partial line and continuous output under slow UI/disk; parser/ring/snapshot/queue/rotation obey exact budgets and counters. Positive: ordinary output remains ordered/useful. |
| Cleanup acknowledgement | Direct child reaped and named owned resources complete; otherwise native Incomplete under the monotonic publication contract. Caller delivery remains scheduler-dependent. Positive: ordinary exit completes. No descendant claim outside captured scope. |
| App/navigation behavior | Delayed ensure after Closing is rejected; StrictMode and route change do not duplicate/start-stop shared services or raw status requests. Positive: reconnect resnapshot restores current state. |
| Platform containment | Retained-anchor final-signal/reap ordering and conservative group absence for supported ordinary descendants; ownership/permission/unobserved cases incomplete. Escape is explicitly excluded, with no invented detection guarantee. Windows tree tests wait for its separate gate. |

End-to-end acceptance is still required on packaged Windows, macOS and Linux with actual sidecars: first launch, node synchronization, agent and memory connection, model cold-load/readiness, Comms/Cluster identity and IPC, Hermes session observer, noisy logs, stop/restart/quit and unavailable endpoints. Record exact binary/model/tool versions, machine limits, outputs, retained memory/disk and elapsed deadlines. Do not use UI rendering, 22 module tests or synthetic transport as proof of those flows. Real user signing/checkout/key custody is outside this audit's runtime actions; lifecycle changes must preserve their accepted gates and existing tests. Existing startup-wide process killing must be removed or safely contained before any ordinary app launch.

Current results, Linux/Rust 1.93.1/Node 26.4.0/Python 3.14.6. Target main is 30c 789e 844ae 98f 9e 4730012ae 7d 1c 6f 8fb 75b 98; unchanged-source receipts retain their actual execution pin:

| Executed check | Exit/result | Limit |
| --- | --- | --- |
| Frontend baseline | 0;88 files, 798 passed/3 skipped | Unit/source contract tests, not desktop integration. |
| Typecheck / production build | Both 0 | Main JS bundle ~1 MiB warning; no performance regression claim. |
| Full default native compile | 0 at 30c 789e | Privately extracted Linux development libraries; actual workspace features/renderer linkage, no bundle/app launch. |
| Native app library | 0;539 passed/5 ignored at 30c 789e | Owned stubs/injected transports in PID/net/home-masked namespace; no live sidecars. Earlier 5d 2d 6d 9 suite 533 passed. |
| Shared kit / main | 0;212 passed/1 ignored/1 filtered at 5d 2d 6d 9; main 0 tests | Kit source/dependency closure unchanged at 30c 789e. Real-keyring case filtered. First outer 600 s attempt expired after 204 successes;1800 s retry finished in 586 s. |
| Workspace doctests | 0 at 30c 789e;0 examples | Compile/list contract, not new runtime proof. |
| Original isolated supervisor suite | 0;22 passed | Actual retained module; resolver/GTK omitted, Windows tests not executed. Original shell fixtures left four owned sleep descendants, externally reaped by the wrapper. |
| Matched lifecycle fixtures | 101;3 failed/1 diagnostic passed | Stop Off→Running new PID; recorded-failure counter 3 with max_retries 1 and 13 false probes;4 simultaneous blocked workers; one retained 1, 048, 576-byte line. Scheduling/PIDs vary. |
| Matched Linux descendant fixture | 101;1 failed | Parent Off, ordinary verified descendant alive; fixture cleanup complete. |

The first independent reviewer replay used five newer transitive packages. Root then aligned all 12 harness registry package versions, sources and checksums to matching entries in the target Cargo.lock and strengthened probe acquisition checks; all observations persisted. This matches the isolated dependency closure, not full workspace feature unification or native packaging. The fresh architecture reviewer independently replayed the reconstructed matched closure: 0/101/101 with all owned cleanup complete, confirming the stronger oracles. Its initial design verdict was REVISE DESIGN. V2 confirmed those resolutions but found shared-executor deadline starvation; the revised independent-publication v3 candidate subsequently received APPROVE DESIGN. Existing runtime/native acceptance gaps remain explicit.

Full module SHA-256: `14d72aec89305a1fd03fd9c7c1667d82252fec9aa5423d663900005df1ad2d1c`; removed lines 83–124: `78106bf9e7c26735163ec3d5568e0d243683a7765726f5e2a8e7e063a5203dd6`; retained module: `ab4c771334ea98e3bf9abc1424920c65512ec54ff73eb7949cf475c91b2f34d8`. Only `resolve_external_bin` plus its following blank line is omitted. Original Unix/Windows test copies match source byte-for-byte. The extraction is not a claim about resolver integration.

## Residual coverage and parked choices

| Surface | What was established | Remaining empirical scope / reopen trigger |
| --- | --- | --- |
| Custody/ceremony/budgets | Full source review and existing tests; accepted signing/budget ADRs preserved. Earlier raw-ceremony hypotheses were closed rather than promoted. | Live custody/provider/signature flows not run. Reopen only with a source-bound failing invariant or approved isolated integration. |
| Identity/staking/distribution | Source/native boundary review and existing unit coverage. | Live RPC/payment/identity incidents not established. Older per-device identity ADR differs from current deliberate wallet-derived provider; reconciliation is separate. |
| Groups/message retention | Actual frontend source plus synthetic drain/roster/navigation fixtures produced three expected property failures. | No live daemon delivery/deletion proof. Separate durable-retention/idempotency design gate; this lifecycle SPEC does not fix message retention. |
| Models/storage/training | Full source review; lifecycle/model preconditions preserved. Metadata/partial-download/RPC error questions remain source-only. | Real model download/cold-load, storage failure and training hardware tests remain required. Never claim the observed release GPU freeze is caused by these supervisor fixtures. |
| Agent/session/store | Accepted Hermes core/sidecar boundary preserved; invoke timeout diagnostic showed a later synthetic effect can finish. | No external late-effect/duplicate action demonstrated. Durable operation registry and financial replay remain parked. |
| Updater/build/dependencies | Workflows/lockfiles read; frontend baseline and full Linux compile/native units passed; updater exit paths traced. npm reported 23 advisories, not exposure-assessed. | Packaged/native UI, signing/update channels and dependency applicability remain open; no vulnerability/exposure claim. |
| Process containment | Actual ordinary Linux descendant survived; Unix/Windows native contracts researched. | Windows Job owner and escaped-child/crash-recovery semantics require separate gate/native evidence. |
| Executor availability | Source inventory: 61 manager async commands, 40 offloaded/21 inline; nine invite commands also reach inline filesystem/Comms/keyring paths. The async-command tripwire does not cover them. No runtime starvation incident was reproduced. | The reserved native publisher/cached receipt path addresses selected lifecycle observation. General UI/domain progress and finite foreground adapter admission require separate work; off_main alone supplies neither a queue bound nor cancellation. |
| Formal/source tests | Existing assumptions/status read and original 22 controls retained. | A model/source string check alone does not prove runtime refinement, packaging or release readiness. |

There is no claim that all imaginable fault techniques were exhausted or that the whole deployed product is safe. Applicable local lanes included full source/dataflow review, upstream/accepted-policy comparison, baseline/type/build checks, owned child fault and retry tests, blocked-work scheduling, output-size diagnostics, synthetic async order/drain tests, platform API review and independent replay. Independent replay/architecture review reopened five blocking choices; later main added Comms workers and dependency review exposed linger/overlapped ownership. These were incorporated and returned for fresh independent defense. Saturation is evidence-derived, not a fixed agent count. The audit is bounded and its nonexecuted integration scopes remain explicit.

## Adoption and migration gates

Before a builder: readable pinned canonical planset or an explicit owner deviation decision; accepted status/control DTO semantics; attested versus liveness-only retry policy; quarantine and UX consequence; measured/default diagnostic budgets; Unix containment compatibility; and explicit disposition of Windows/PARK and force-exit guarantees. No automatic ceremony/budget/identity policy widening is permitted.

Before landing implementation: nondecreasing workspace tests on supported native runners, existing frontend baseline, invariant tests above, all manager migrations, packaged cross-platform evidence appropriate to claims, source-compatible API migration and owner review. Before broader release: normal project production/security approvals remain separate; this document is not that approval.

## Knyte artifact contract

Knyte/Spindle examined at 93d 983c 7d 1f 81ad 865fc 2e 58a 092a 200752bfc 4f; actual CLI 0.160.0 workers use gpt-6.1-sol with ultra reasoning. Its built-in executor is Claude-only, so it was not used to dispatch. Research/review runs used Codex; actual Spindle new/lock/check/spec verify/load/state gate primitives produce design artifacts and private receipts.

Citrate requires created/branch/author/status frontmatter. Knyte's ADR schema rejects the first three keys. Preserve the full original ADR and verify its actual doc-lock; for typed ADR schema/check/index use a private lossless projection removing **only** created/branch/author, preserving body/all other fields and envelope positions. Reinsert the envelope to assert byte-exact reconstruction. Check the projection with the unmodified engine; its independently re-locked digest differs from the full original. Never state direct `spindle-adr check` passes on the original. Unexpected/duplicate/nested keys are rejected, not silently filtered. Owner adoption of this explicit split-validation policy is pending.

The full SPEC is locked/verified/loaded directly by the real engine. Its private board gate receipt is tied to the full final SPEC digest and explicit ADR ID. A receipt/hash attests authored content and a recorded design verdict, not engine-enforced review quality, adoption, implementation or release. Private indexes/board/logs stay outside the contribution. Re-lock after changes and rerun fresh review when a decision changes. Final artifact verification uses the unmodified pinned engine: real ADR new/lock; full-original verifyDoc; lossless private projection lock/index/check; full SPEC lock/verify/index/load; eight SpecGated receipts using the full final SPEC lock digest and explicit ADR ID; and render --check. Original strict ADR check is the expected schema rejection (exit1), while projected ADR check and SPEC verification/load exit0. Duplicate/unknown/nested metadata and meaningful body-tamper negative controls reject; repeated load/gate is idempotent. All eight stories remain TODO, with no dispatch or implementation. Private indexes/boards/command receipts stay outside this contribution. The final frontmatter lock value and private event digest are checked directly; no document embeds a circular claim to its own final hash.

## Sources

Pinned repo behavior: [supervisor](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/kit/src/supervisor.rs), [llama profile](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/src-tauri/src/serve.rs), [app exit/startup](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/src-tauri/src/lib.rs), [groups adapter](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/src/bridge/tauri/comms.ts), [groups slice](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/src/shell/slices/groups.ts).

Native ownership: [Rust 1.93.1 Child source](https://raw.githubusercontent.com/rust-lang/rust/1.93.1/library/std/src/process.rs), [Rust 1.93.1 Unix API](https://raw.githubusercontent.com/rust-lang/rust/1.93.1/library/std/src/os/unix/process.rs), [std JoinHandle contract](https://doc.rust-lang.org/std/thread/struct.JoinHandle.html), [POSIX group signals](https://pubs.opengroup.org/onlinepubs/009604499/functions/kill.html), [Microsoft Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [Job query](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-queryinformationjobobject).

Probe/shutdown patterns: [Tokio 1.52.3 blocking implementation](https://raw.githubusercontent.com/tokio-rs/tokio/tokio-1.52.3/tokio/src/task/blocking.rs), [Tokio owner channels](https://tokio.rs/tokio/tutorial/channels), [Tokio shutdown](https://tokio.rs/tokio/topics/shutdown), [ureq 3.3.0 defaults](https://docs.rs/ureq/3.3.0/src/ureq/config.rs.html#864-909), [ureq resolver](https://docs.rs/ureq/3.3.0/src/ureq/unversioned/resolver.rs.html#142-158), [body limits](https://docs.rs/ureq/3.3.0/ureq/struct.BodyWithConfig.html#method.limit), [exact 3.3.0 timings](https://github.com/algesten/ureq/blob/b2adbf00f9a7ac0e2fbcb39d23c1b4f3da723e5c/src/timings.rs), [later merged phase-timeout fix 1194](https://github.com/algesten/ureq/pull/1194). The global budget retains its starting point on the pin; receive/body phase budget alone is insufficient. The later fix is 18 commits beyond 3.3.0, not existing behavior.

Explicit health adapter: [ureq 3.3.0 Resolver contract/default helper](https://github.com/algesten/ureq/blob/b2adbf00f9a7ac0e2fbcb39d23c1b4f3da723e5c/src/unversioned/resolver.rs), [Agent::with_parts](https://github.com/algesten/ureq/blob/b2adbf00f9a7ac0e2fbcb39d23c1b4f3da723e5c/src/agent.rs#L127), [resolver invocation](https://github.com/algesten/ureq/blob/b2adbf00f9a7ac0e2fbcb39d23c1b4f3da723e5c/src/run.rs#L381), [TCP connector/timeouts](https://github.com/algesten/ureq/blob/b2adbf00f9a7ac0e2fbcb39d23c1b4f3da723e5c/src/unversioned/transport/tcp.rs), [unversioned compatibility policy](https://github.com/algesten/ureq/blob/b2adbf00f9a7ac0e2fbcb39d23c1b4f3da723e5c/src/unversioned/mod.rs).

Unix retained anchor: [POSIX 2024 process lifetime/ID reuse](https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html#tag_04_17), [setsid](https://pubs.opengroup.org/onlinepubs/9799919799/functions/setsid.html), [async-signal-safe functions](https://pubs.opengroup.org/onlinepubs/9799919799/functions/V2_chap02.html#tag_16_04_03), [waitid](https://pubs.opengroup.org/onlinepubs/9799919799/functions/waitid.html), [kill](https://pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html), [pinned Linux WNOWAIT/reaping](https://github.com/torvalds/linux/blob/b636fef85bda7d1bab9c0a45067ab1508d79d946/kernel/exit.c#L1184), [pinned XNU waitid/no-status/reaping](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_exit.c#L3263), [XNU group signal result](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_sig.c#L1675). Supported 64-bit Darwin libc uses UNIX 03 kill semantics; no legacy/raw-syscall inference is accepted. [Merged NVIDIA 2270](https://github.com/NVIDIA/Model-Optimizer/pull/2270) and its [pinned implementation](https://github.com/NVIDIA/Model-Optimizer/blob/6a2ae5a25bea8244cdfe1a316f12c500296161e4/tests/_test_utils/examples/run_command.py#L110) adopt observe-without-reaping→group kill→reap ordering; their blocking wait/pipe trigger is not adopted.

All exit paths: [process 2.3.1 commands](https://github.com/tauri-apps/plugins-workspace/blob/e7a68fa63755603b9fa12d28e077eea645551d24/plugins/process/src/commands.rs), [process permissions](https://github.com/tauri-apps/plugins-workspace/blob/e7a68fa63755603b9fa12d28e077eea645551d24/plugins/process/permissions/default.toml), [updater 2.10.1 install/direct exit](https://github.com/tauri-apps/plugins-workspace/blob/d6a3898001a4bcc659e045f9501498751b77dbe6/plugins/updater/src/updater.rs), [updater framework-cleanup hook](https://github.com/tauri-apps/plugins-workspace/blob/d6a3898001a4bcc659e045f9501498751b77dbe6/plugins/updater/src/lib.rs), [early single-instance bootstrap](https://github.com/tauri-apps/plugins-workspace/tree/cad301fcc1f3ebad1eaef552c886b0bc8580c3fe/plugins/single-instance/src/platform_impl). Cached/published/pinned process and updater sources were byte-checked; these are source contracts, not native updater execution receipts.

App observation: [Tauri state management](https://v2.tauri.app/develop/state-management/), [Tauri 2.11.5 app implementation](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.11.5/crates/tauri/src/app.rs), [Tauri 2.11.5 event API](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.11.5/packages/api/src/event.ts), [React subscription contract](https://react.dev/reference/react/useSyncExternalStore), [React effect cleanup](https://react.dev/reference/react/useEffect), [Tauri early-unlisten issue 15799](https://github.com/tauri-apps/tauri/issues/15799), [merged fix 15800](https://github.com/tauri-apps/tauri/pull/15800), [2.12.0 guarded implementation](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.12.0/crates/tauri/src/event/mod.rs), [React 19.2.7 implementation](https://raw.githubusercontent.com/facebook/react/v19.2.7/packages/react-reconciler/src/ReactFiberHooks.js).

Rejected dependency pattern: [process-wrap 10.0.1 std Child API](https://raw.githubusercontent.com/watchexec/process-wrap/v10.0.1/src/std/core.rs), [std Job wrapper](https://raw.githubusercontent.com/watchexec/process-wrap/v10.0.1/src/std/job_object.rs), [Windows completion helper](https://raw.githubusercontent.com/watchexec/process-wrap/v10.0.1/src/windows.rs), [maintenance policy](https://github.com/watchexec/process-wrap), [closed unmerged cleanup proposal 55](https://github.com/watchexec/process-wrap/pull/55). Job accounting alone is not sufficient terminal-handle evidence; this proposed repair did not land. These pinned source limits are not a Windows runtime finding. [Firecrawl Developer Index](https://www.firecrawl.dev/developer-index) is supplementary discovery, not an authority that overrides pinned source.

Windows Comms: [current upstream IPC/helper](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/src-tauri/src/comms.rs#L691), [injected deadline/cancellation tests](https://github.com/CitrateNetwork/citrate-core/blob/30c789e844ae98f9e4730012ae7d1c6f8fb75b98/src-tauri/src/comms_tests.rs#L617), [CancelSynchronousIo](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelsynchronousio), [ReadFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-readfileex), [CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex). Interprocess 2.4.4 pins commit e27f397daebff9054f8e2f7b3dc034bae36b2867: [public enum](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/local_socket/enumdef.rs#L18), [borrowed I/O](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/local_socket/stream/enum.rs#L19), [wrapper conversion](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/named_pipe/local_socket/stream.rs#L126), [fallible consuming handle conversion](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/named_pipe/stream/impl/handle.rs#L39), [drop transfer](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/named_pipe/stream.rs#L79), [detached linger workers](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/linger_pool.rs#L267), [clone](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/named_pipe/stream/impl/handle.rs#L92), [Always flush state](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/needs_flush.rs#L19), [overlapped creation](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/named_pipe/c_wrappers.rs#L158), [completion-routine I/O](https://github.com/kotauskas/interprocess/blob/e27f397daebff9054f8e2f7b3dc034bae36b2867/src/os/windows/c_wrappers.rs#L91). Source-proved API route/caveats do not substitute for Windows runtime acceptance.

Existing decisions: [Hermes loop ADR](../adr/ADR-2026-09-30-hermes-loop-in-sidecar.md), [cluster extraction](../adr/ADR-2026-08-28-cluster-daemon-extraction.md), [budget amendment](../adr/ADR-2026-09-30-rule3-budgetable-signatures.md), [identity ADR](../adr/ADR-2026-08-30-cluster-identity-and-transport.md). Canonical pointer: [.agentile planset](../../.agentile/planset/README.md). Merged [PR 180](https://github.com/CitrateNetwork/citrate-core/pull/180) adds a Windows post-connect observer deadline and best-effort cancellation, with no retained population/complete transport ownership proof; its helper tests are included in the 539-test current app baseline. Backlog novelty was assessed against fetched snapshots, not every unseen branch.

Deadline execution: [pinned Tauri async runtime](https://github.com/tauri-apps/tauri/blob/7cd71369c00978a3783b6ae3e9972358abbe4ae6/crates/tauri/src/async_runtime.rs#L222), [Tokio 1.52.3 timeout source](https://github.com/tokio-rs/tokio/blob/d87569164fb61145e79e7ffe0b25783569cc8f93/tokio/src/time/timeout.rs), [blocking queue/capacity](https://github.com/tokio-rs/tokio/blob/d87569164fb61145e79e7ffe0b25783569cc8f93/tokio/src/runtime/builder.rs#L517), [Rust 1.93.1 monotonic Condvar wait contract](https://github.com/rust-lang/rust/blob/1.93.1/library/std/src/sync/poison/condvar.rs#L263). Firecrawl's reopened deadline queries returned 200/nonpartial; its 1.49 docs and unrelated paused-clock/Emscripten changes do not override the inspected 1.52.3 native source. [Public updater builder/check](https://github.com/tauri-apps/plugins-workspace/blob/d6a3898001a4bcc659e045f9501498751b77dbe6/plugins/updater/src/lib.rs#L33), [check resource insertion](https://github.com/tauri-apps/plugins-workspace/blob/d6a3898001a4bcc659e045f9501498751b77dbe6/plugins/updater/src/commands.rs#L40). These are source-backed future contracts, not an executed starvation or updater incident.


## Reproduction appendix — isolated actual source

These are baseline failure reproducers inside the SPEC, not production code changes or future passing regression tests. The four-worker acquisition establishes overlap on this baseline; a corrected implementation needs a scheduling/acknowledgement control while asserting that its actual worker cap holds. Run only on an isolated Linux test host. They start owned sleep/Python children, never the desktop app or real services. Linux /proc, child-subreaper support, Rust 1.93.1, Python 3, /bin/sleep and standard shell fixture tools are needed. Original 22 tests require external descendant cleanup; use the wrapper below, not an uncontained baseline command. Cargo dependency availability is a separate prerequisite. All expected failing properties preserve Cargo exit 101, not a false green result.

1. Check out the pinned target commit in an isolated checkout. From its root set absolute `CITRATE_SOURCE`; create an empty private `CITRATE_HARNESS` outside that checkout. Populate Cargo.toml/Cargo.lock/tests from the labeled fences below. Create retained source and copy original tests with the following Python. The hash assertion deliberately stops on drift rather than silently applying line offsets to another version.

```python
import hashlib, os, shutil
from pathlib import Path
source=Path(os.environ['CITRATE_SOURCE']); h=Path(os.environ['CITRATE_HARNESS'])
h.mkdir(parents=True,exist_ok=True); (h/'src').mkdir(exist_ok=True); (h/'tests').mkdir(exist_ok=True)
b=(source/'kit/src/supervisor.rs').read_bytes()
assert hashlib.sha256(b).hexdigest()=='14d72aec89305a1fd03fd9c7c1667d82252fec9aa5423d663900005df1ad2d1c'
lines=b.splitlines(keepends=True); retained=b''.join(lines[:82]+lines[124:])
assert hashlib.sha256(retained).hexdigest()=='ab4c771334ea98e3bf9abc1424920c65512ec54ff73eb7949cf475c91b2f34d8'
(h/'src/supervisor.rs').write_bytes(retained)
(h/'src/lib.rs').write_text('pub mod supervisor;\n')
for name in ['supervisor_tests.rs','supervisor_windows_tests.rs']:
 shutil.copyfile(source/'kit/src'/name,h/'src'/name)
```

2. Run `python3 replay-owned.py lib`, `python3 replay-owned.py lifecycle`, and `python3 replay-owned.py descendants` with CITRATE_HARNESS exported. The wrapper preserves real Cargo exits 0/101/101 and verifies its owned process group is empty. An exit 125 means containment/deadline failed and invalidates the receipt. Test acquisition assertions distinguish an unobserved scheduling timeout from the intended failure. The precise sampled phase/PID/probe count can vary; counters, new-child existence, worker overlap and exact retained bytes are the evidence.

The twelve registry versions/checksums below match the pinned workspace lock. This is an isolated dependency closure; the isolated fixture does not establish workspace feature parity or packaged/native UI integration. Full default workspace compilation/native baseline is separately recorded above. All full-module retained bytes and original test copies are unchanged. A future implementation should use synchronized native acknowledgements instead of treating these short observation windows as a proof that no future respawn exists.

### Cargo.toml

```toml
[package]
name = "citrate-supervisor-audit"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
libc = "=0.2.186"
serde = { version = "=1.0.228", features = ["derive"] }
serde_json = "=1.0.150"
```

### Cargo.lock

```toml
# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "citrate-supervisor-audit"
version = "0.0.0"
dependencies = [
 "libc",
 "serde",
 "serde_json",
]

[[package]]
name = "itoa"
version = "1.0.18"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682"

[[package]]
name = "libc"
version = "0.2.186"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "68ab91017fe16c622486840e4c83c9a37afeff978bd239b5293d61ece587de66"

[[package]]
name = "memchr"
version = "2.8.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98"

[[package]]
name = "proc-macro2"
version = "1.0.106"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "8fd00f0bb2e90d81d1044c2b32617f68fcb9fa3bb7640c23e9c748e53fb30934"
dependencies = [
 "unicode-ident",
]

[[package]]
name = "quote"
version = "1.0.46"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "dfbc457d0c7a0759a614551b11a6409e5951f6c7537be1f1b7682b9ae9230368"
dependencies = [
 "proc-macro2",
]

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"
dependencies = [
 "serde_core",
 "serde_derive",
]

[[package]]
name = "serde_core"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "41d385c7d4ca58e59fc732af25c3983b67ac852c1a25000afe1175de458b67ad"
dependencies = [
 "serde_derive",
]

[[package]]
name = "serde_derive"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "d540f220d3187173da220f885ab66608367b6574e925011a9353e4badda91d79"
dependencies = [
 "proc-macro2",
 "quote",
 "syn",
]

[[package]]
name = "serde_json"
version = "1.0.150"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "e8014e44b4736ed0538adeecded0fce2a272f22dc9578a7eb6b2d9993c74cfb9"
dependencies = [
 "itoa",
 "memchr",
 "serde",
 "serde_core",
 "zmij",
]

[[package]]
name = "syn"
version = "2.0.118"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "1b9ae57f904213ebb649ce6895b8a66c66f0203b9319718f69a5612a065b1422"
dependencies = [
 "proc-macro2",
 "quote",
 "unicode-ident",
]

[[package]]
name = "unicode-ident"
version = "1.0.24"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75"

[[package]]
name = "zmij"
version = "1.0.21"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "b8848ee67ecc8aedbaf3e4122217aff892639231befc6a1b58d29fff4c2cabaa"
```

### tests/lifecycle.rs

```rust
//! Private audit fixtures. Real owned child processes; no network or app startup.
use citrate_supervisor_audit::supervisor::*;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static NONCE: AtomicUsize = AtomicUsize::new(0);
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("citrate-audit-{}-{}", std::process::id(), NONCE.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn config(tmp: &Temp, spec: SidecarSpec) -> SupervisorConfig {
    let mut cfg = SupervisorConfig::new(spec, tmp.0.join("crashes.jsonl"));
    cfg.stop_grace = Duration::from_millis(50);
    cfg.join_timeout = Duration::from_secs(2);
    cfg
}
fn sleep_spec() -> SidecarSpec { SidecarSpec::new("owned-audit-sleep", "/bin/sleep", vec!["30".into()]) }
fn wait_for(timeout: Duration, mut pred: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + timeout;
    while Instant::now() < until { if pred() { return true; } std::thread::sleep(Duration::from_millis(10)); }
    pred()
}

#[test]
fn second_stop_must_not_resurrect_child() {
    let tmp = Temp::new();
    let mut sup = Supervisor::start(config(&tmp, sleep_spec())).unwrap();
    let first = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(2));
    assert_eq!(first.state, SupervisorState::Running);
    sup.stop();
    let stopped = sup.status();
    assert_eq!(stopped.state, SupervisorState::Off);
    sup.stop();
    let respawned = wait_for(Duration::from_millis(600), || sup.status().pid.is_some());
    let observed = sup.status();
    sup.shutdown();
    println!("AUDIT {{\"probe\":\"second_stop\",\"first_pid\":{:?},\"after_first\":\"{:?}\",\"after_second\":\"{:?}\",\"new_pid\":{:?},\"respawned\":{}}}", first.pid, stopped.state, observed.state, observed.pid, respawned);
    assert!(!respawned, "stop is documented idempotent; second stop spawned an owned child again");
    assert_eq!(observed.state, SupervisorState::Off);
}

#[test]
fn never_healthy_child_must_exhaust_retry_budget() {
    let tmp = Temp::new();
    let mut spec = sleep_spec();
    let probes = Arc::new(AtomicUsize::new(0));
    let probe_count = probes.clone();
    spec.health_check = Some(HealthCheck { interval: Duration::from_millis(20), grace: Duration::from_millis(120), probe: Arc::new(move || { probe_count.fetch_add(1, Ordering::SeqCst); false }) });
    let mut cfg = config(&tmp, spec);
    cfg.healthy_after = Duration::from_millis(40);
    cfg.backoff.max_retries = 1;
    cfg.backoff.base_delay = Duration::from_millis(1);
    cfg.backoff.max_delay = Duration::from_millis(1);
    let mut sup = Supervisor::start(cfg).unwrap();
    let acquired = wait_for(Duration::from_secs(2), || matches!(sup.status().state, SupervisorState::Failed) || sup.status().restarts >= 3);
    let observed = sup.status();
    sup.shutdown();
    println!("AUDIT {{\"probe\":\"never_healthy_retry\",\"max_retries\":1,\"restarts\":{},\"state\":\"{:?}\",\"probe_count\":{},\"successful_probes\":0}}", observed.restarts, observed.state, probes.load(Ordering::SeqCst));
    assert!(acquired, "fixture must reach terminal failure or three recorded failures; a mere scheduling timeout is inconclusive");
    assert_eq!(observed.state, SupervisorState::Failed, "always-false probes must not reset consecutive failure budget");
}

#[test]
fn log_ring_line_count_does_not_bound_bytes_diagnostic() {
    let tmp = Temp::new();
    let bytes = 1_048_576usize;
    let spec = SidecarSpec::new("owned-audit-log", "/usr/bin/python3", vec!["-c".into(), format!("import sys,time; sys.stdout.write('x'*{bytes}+'\\n');sys.stdout.flush();time.sleep(30)")]);
    let mut sup = Supervisor::start(config(&tmp, spec)).unwrap();
    let captured = wait_for(Duration::from_secs(3), || !sup.logs().is_empty());
    let logs = sup.logs();
    sup.shutdown();
    let retained: usize = logs.iter().map(|l| l.line.len()).sum();
    println!("AUDIT {{\"probe\":\"log_bytes\",\"input_bytes\":{},\"lines\":{},\"retained_bytes\":{},\"documented_line_cap\":{},\"diagnostic_only\":true}}", bytes, logs.len(), retained, LOG_RING_CAPACITY);
    assert!(captured);
    assert_eq!(retained, bytes, "diagnostic measures retained bytes; no existing byte limit is asserted");
}

struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Release { fn drop(&mut self) { let (lock, cv) = &*self.0; *lock.lock().unwrap() = true; cv.notify_all(); } }
#[test]
fn health_probe_timeout_must_not_accumulate_inflight_workers() {
    let tmp = Temp::new();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(gate.clone());
    let active = Arc::new(AtomicUsize::new(0));
    let total = Arc::new(AtomicUsize::new(0));
    let a = active.clone(); let t = total.clone(); let g = gate.clone();
    let mut spec = sleep_spec();
    spec.health_check = Some(HealthCheck { interval: Duration::from_millis(20), grace: Duration::from_secs(2), probe: Arc::new(move || {
        a.fetch_add(1, Ordering::SeqCst); t.fetch_add(1, Ordering::SeqCst);
        let (lock, cv) = &*g;
        let mut done = lock.lock().unwrap();
        while !*done { done = cv.wait(done).unwrap(); }
        a.fetch_sub(1, Ordering::SeqCst); false
    }) });
    let mut sup = Supervisor::start(config(&tmp, spec)).unwrap();
    let acquired = wait_for(Duration::from_secs(2), || active.load(Ordering::SeqCst) >= 4);
    let concurrent = active.load(Ordering::SeqCst);
    drop(release);
    sup.shutdown();
    let cleaned = wait_for(Duration::from_secs(2), || active.load(Ordering::SeqCst) == 0);
    println!("AUDIT {{\"probe\":\"wedged_health\",\"simultaneous_workers\":{},\"total_started\":{},\"cleanup_complete\":{}}}", concurrent, total.load(Ordering::SeqCst), cleaned);
    assert!(cleaned, "fixture releases every owned blocked probe");
    assert!(acquired, "fixture must acquire four blocked workers; a scheduling timeout is inconclusive");
    assert!(concurrent <= 1, "timeout dropped handles without cancelling workers, allowing concurrent stale probes");
}
```

### tests/descendants.rs

```rust
//! Bounded Linux-only diagnostic of the owned process tree; private test fixture.
#![cfg(target_os = "linux")]
use citrate_supervisor_audit::supervisor::*;
use std::time::{Duration, Instant};

fn identity(pid: u32) -> Option<(u32,u64)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, tail) = s.split_once(") ")?;
    let fields: Vec<_> = tail.split_whitespace().collect();
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
}
struct OwnedDescendant { pid: u32, start: u64 }
impl Drop for OwnedDescendant {
    fn drop(&mut self) {
        if identity(self.pid).is_some_and(|(_, start)| start == self.start) {
            unsafe { libc::kill(self.pid as i32, libc::SIGKILL); }
        }
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            let r = unsafe { libc::waitpid(self.pid as i32, std::ptr::null_mut(), libc::WNOHANG) };
            if r != 0 { break; }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[test]
fn stopping_fixture_parent_must_also_stop_owned_descendant() {
    // Make THIS isolated test process reap only the descendant created below.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) }, 0);
    let tmp = std::env::temp_dir().join(format!("citrate-descendants-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let spec = SidecarSpec::new("owned-audit-parent", "/usr/bin/python3", vec!["-c".into(), "import subprocess,time; p=subprocess.Popen(['/bin/sleep','30']);print(p.pid,flush=True);time.sleep(30)".into()]);
    let mut cfg = SupervisorConfig::new(spec, tmp.join("crashes.jsonl"));
    cfg.stop_grace = Duration::from_millis(50); cfg.join_timeout = Duration::from_secs(2);
    let mut sup = Supervisor::start(cfg).unwrap();
    let until = Instant::now()+Duration::from_secs(2);
    let pid = loop {
        if let Some(pid) = sup.logs().first().and_then(|line| line.line.parse::<u32>().ok()) { break pid; }
        assert!(Instant::now()<until, "fixture parent must report owned child's PID");
        std::thread::sleep(Duration::from_millis(10));
    };
    let parent = sup.status().pid.unwrap();
    let (ppid,start) = identity(pid).expect("owned child present");
    assert_eq!(ppid,parent, "cleanup authority proven by fixture parent-child relationship");
    let cleanup = OwnedDescendant { pid,start };
    assert_eq!(std::fs::read(format!("/proc/{pid}/cmdline")).unwrap(), b"/bin/sleep\x0030\x00");
    sup.shutdown();
    let remaining = identity(pid).is_some_and(|(_,t)|t==start);
    let stopped = sup.status();
    drop(cleanup);
    let cleaned = identity(pid).is_none();
    let _ = std::fs::remove_dir_all(&tmp);
    unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 0, 0, 0, 0); }
    println!("AUDIT {{\"probe\":\"owned-descendant\",\"parent_pid\":{parent},\"descendant_pid\":{pid},\"parent_status\":\"{:?}\",\"descendant_survived\":{remaining},\"fixture_cleanup_complete\":{cleaned},\"linux_only\":true}}",stopped.state);
    assert!(cleaned, "fixture must reap its own descendant");
    assert!(!remaining, "supervisor terminates direct child but leaves this ordinary owned descendant alive");
}
```

### replay-owned.py

```python
import ctypes, json, os, signal, subprocess, sys, time
from pathlib import Path
harness=Path(os.environ['CITRATE_HARNESS']).resolve(); root=harness.parent
mode=sys.argv[1]
if mode not in ('lib','lifecycle','descendants'):
    raise SystemExit('expected lib, lifecycle or descendants')
cmd=['cargo','test','--locked','--lib'] if mode=='lib' else ['cargo','test','--locked','--test',mode,'--','--nocapture','--test-threads=1']
libc=ctypes.CDLL(None,use_errno=True)
if libc.prctl(36,1,0,0,0)!=0:
    raise SystemExit('subreaper preflight failed')
env=dict(os.environ,CARGO_NET_OFFLINE='true',TMPDIR=str(root),CARGO_INCREMENTAL='0')
def members(pgid):
    found=[]
    for d in Path('/proc').iterdir():
        if not d.name.isdigit():
            continue
        try:
            s=(d/'stat').read_text(); fields=s[s.rfind(')')+2:].split()
            if int(fields[2])==pgid:
                found.append({'pid':int(d.name),'state':fields[0],'ppid':int(fields[1]),'start_ticks':fields[19],'comm':s[s.find('(')+1:s.rfind(')')]})
        except (FileNotFoundError,PermissionError,ProcessLookupError):
            pass
    return sorted(found,key=lambda x:x['pid'])
print('command='+json.dumps(cmd),flush=True)
print('env_overrides='+json.dumps({k:env[k] for k in ['CARGO_NET_OFFLINE','TMPDIR','CARGO_INCREMENTAL']}),flush=True)
proc=subprocess.Popen(cmd,cwd=harness,env=env,start_new_session=True)
start=time.monotonic(); timed_out=False
while proc.poll() is None:
    if time.monotonic()-start>120:
        timed_out=True
        os.killpg(proc.pid,signal.SIGTERM)
        time.sleep(.5)
        if proc.poll() is None:
            os.killpg(proc.pid,signal.SIGKILL)
        break
    time.sleep(.01)
code=proc.wait()
leftover=members(proc.pid)
if leftover:
    try:
        os.killpg(proc.pid,signal.SIGTERM)
    except ProcessLookupError:
        pass
    time.sleep(.1)
    if any(x['state']!='Z' for x in members(proc.pid)):
        try:
            os.killpg(proc.pid,signal.SIGKILL)
        except ProcessLookupError:
            pass
reaped=[]; deadline=time.monotonic()+3
while time.monotonic()<deadline:
    while True:
        try:
            pid,status=os.waitpid(-1,os.WNOHANG)
        except ChildProcessError:
            break
        if pid==0:
            break
        reaped.append({'pid':pid,'wait_status':status})
    if not members(proc.pid):
        break
    time.sleep(.02)
remaining=members(proc.pid)
print('REPLAY_RESULT '+json.dumps({'cargo_exit_code':code,'timed_out':timed_out,'owned_descendants_after_cargo_exit':leftover,'adopted_descendants_reaped':reaped,'owned_group_remaining':remaining,'cleanup_complete':not remaining},sort_keys=True))
raise SystemExit(code if not remaining and not timed_out else 125)
```
