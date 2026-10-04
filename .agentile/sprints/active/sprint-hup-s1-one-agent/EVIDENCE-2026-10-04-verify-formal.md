---
created: 2026-10-04T14:45:00Z
branch: hup/n6-verify-formal
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S1
---

# HUP-S1 evidence: S1.3 verifiers + formal and gate re-runs (fan-out 6, 2026-10-04)

Every run below was made on 2026-10-04 on this Mac (Apple Silicon, 12 cores) with the commands
shown. This file only records evidence. It does not edit `gates.yaml`: the retro step flips the
criteria, using the mapping at the end.

Heads used:

| Repo | Ref | Commit |
|---|---|---|
| citrate-core | `origin/hup/m2-core` (base of this branch) | `7581e88` |
| citrate-agent-runtime | `hup/n6-verify-formal` (S1.3 build, below) on `origin/hup/m2-runtime` `1181a51` | `cf25f1e` |
| citrate-cluster | `origin/main` | `35a4caf` |

Tools: TLC2 2.19 (08 Aug 2024, rev 5a47802) from `~/.tla/tla2tools.jar`, on Homebrew OpenJDK 27,
`-workers auto` (12 workers), default heap. Every run went to completion: no timeout, 0 states
left on the queue.

## 1. TLC runs (committed configs, to completion)

Command shape: `java -XX:+UseParallelGC -cp ~/.tla/tla2tools.jar tlc2.TLC -workers auto -metadir <tmp> <Spec>.tla -config <Spec>.cfg`, run from the spec's directory.

| Spec | Path | Result | Distinct states | Generated | Depth | Duration |
|---|---|---|---|---|---|---|
| FolderGrant | runtime `agent-grants/formal/FolderGrant.{tla,cfg}` | no error, temporal `FullAccessExpires` checked | 153,484 | 270,585 | 8 | 1 min 06 s |
| SpendBudget | core `src-tauri/formal/SpendBudget.{tla,cfg}` | no error | 6,926,616 | 51,284,760 | 25 | 41 s |
| WebSigningBudget | core `src-tauri/formal/WebSigningBudget.{tla,cfg}` | no error | 7,828,872 | 37,916,870 | 21 | 3 min 39 s |
| DeviceLink | cluster `formal/DeviceLink.{tla,cfg}` | no error | 8,248 | 80,876 | 11 | under 1 s |
| AgentLoop | core `src-tauri/formal/AgentLoop.{tla,cfg}` | no error, `StopIsLive` and `Terminates` checked | 1,258 | 2,342 | 27 | under 1 s |

Invariants checked per config: FolderGrant (TypeOK, NoParentEscape, SecretsNeverReadable,
ExpiredGrantInert, NoAccessOutsideActiveGrant, ReadNotImpliesWrite, DotEnvOnlyViaFolderGrant,
FullAccessReadOnly, FullAccessBounded, NoGrantRootedInDenyList); SpendBudget (TypeOK,
SpendWithinCap, NoEscalationWithoutShownPrice, EgressOptInOnly, OverBudgetOrTaintedNeedsHic1,
ReservedIsConsistent; properties ResetOnlyAtPeriodBoundary, PeriodMonotone); WebSigningBudget
(TypeOK, OnlyClosedList, OriginBound, TopFrameOnly, NonceUnique, NoCapabilityDelegation,
RecipientPinned, NeverExceedsCaps, ReservedBeforeSigned, RevokeImmediate, ExpiredInert,
TaintDowngrade, RecordBeforeSignature, NoFalseNegative, NoDrop; property BudgetMonotone);
AgentLoop (TypeOK, Bounded, OnlyVerifierSucceeds, NoEffectWithoutGate, TaintDowngrade;
properties StopIsLive, Terminates).

The figures match the 2026-10-01 verification runs exactly for every spec (same state counts),
so nothing in the specs drifted between the two runs.

## 2. Mutation checks (one invariant per spec)

Each mutant was applied to a copy outside the source tree. The committed spec was never edited.
`git status` shows no change under any `formal/` directory after the runs.

| Spec | Mutant | Target | Result |
|---|---|---|---|
| SpendBudget | `SpendBudget_mutants.py M01`: the budgeted run forgets the `Fits(q)` cap check | SpendWithinCap | killed (TLC reports the violation) |
| WebSigningBudget | `WebSigningBudget_mutants.py M08`: the `rq.value <= PerSigMax` guard is removed | NeverExceedsCaps | caught, trace length 7 |
| FolderGrant | manual: drop `~ImplSecret(root)` (RootDenied) from `GrantA`, cfg reduced to TypeOK + the target | NoGrantRootedInDenyList | `Error: Invariant NoGrantRootedInDenyList is violated.` after 60 states |

FolderGrant has no committed mutant script; the mutant above is the one-line patch shown, run
from a scratch copy. Adding a `FolderGrant_mutants.py` beside the spec is a follow-up for the
runtime repo.

## 3. AgentLoop and the S1.3 planner / self-review

The S1.3 build (runtime `hup/n6-verify-formal`, `cf25f1e`) adds:

- `HttpStatusIs` and `Sha256Equals` verifiers and the `http_status_is` / `sha256_equals`
  VerifierSpec kinds (catalog and posted workflows). The verdict comes from a session probe:
  HTTP to loopback or a member-consented origin only (no redirects, no proxy, at most 10 s);
  hashing only inside the session's live folder grants.
- `Event::SelfReview {step, attempt, text, label: "opinion"}`: the model's self-assessment of
  each attempt, asked before the verifiers run, stored in the session's event log, never added
  to the conversation and never read by the outcome.
- `planner::ModelPlanner`: the model proposes a workflow as JSON with no tools offered; a step
  without a verifier (or naming a tool the session lacks) is refused by the same
  `Workflow::new` gate. The executor (`run_workflow`) is unchanged.

Does this change the AgentLoop model's state space? The planner does not: it runs before the
workflow and can only produce a workflow that already satisfies the spec's premise (every step
has a verifier; `Workflow::new` refuses otherwise), so `OnlyVerifierSucceeds`, `StopIsLive` and
`Terminates` are unchanged and were re-run above. No invariant was added. The self-review adds at
most one model call per attempt with no tools offered, so it can propose no effect and cannot
change `outcome`. The spec's `Bounded` counts the loop's own model calls only; the review calls
are bounded separately by construction (one per attempt) and by `SELF_REVIEW_MAX_TOKENS` (160) in
the sidecar. If the owner wants the review calls inside `Bounded`, that is a small spec change
(add a `reviews` counter to the judge phase); it is not needed for the three properties named in
the S1.3 task.

Tests (runtime): `agent-loop/tests/verify_plan_tests.rs` (20: pass, fail, unreachable URL, path
outside the grant, bounded timeout, spec kinds and malformed kinds, PASS opinion with a failing
verifier still fails, FAIL opinion with passing verifiers still succeeds, failed review call is
"no opinion", no reviewer means no extra call, bounded opinion text, planner proposes, refuses a
step without a verifier, refuses an unknown tool, needs probes for HTTP/hash, refuses unusable
plans, executor judges a planned workflow); `agent-metering/tests/metering_tests.rs` (+1: an
opinion opens no turn); `agent-sidecar/src/verify_probes_tests.rs` (12: live loopback servers for
200/404/302, unreachable port, origin allowlist, granted file hashed, outside-grant and symlink
refused, posted workflows judged live, self-review recorded as an opinion and not verifying).
Affected crates: 723 passed, 0 failed. `rustup run 1.98.1 cargo clippy --no-deps -p
citrate-agent-loop -p citrate-agent-learn -p citrate-agent-metering -p agent-sidecar -p
citrate-agent-browser --all-targets -- -D warnings`: clean.

Not covered by a test: the consented-origin branch of the HTTP probe (it needs an attached
browser session); the allow path is exercised for loopback only.

## 4. Gate test re-runs on the release head (core `7581e88`)

| Test | Command | Result |
|---|---|---|
| Approval enumeration | `npx vitest run src/shell/agentToolGates.test.ts` | 21 passed |
| Annotation enumeration | `npx vitest run src/agent/toolAnnotations.test.ts` | 6 passed |
| Sidebar (D-34) nav | `npx vitest run src/shell/sidebarIa.test.tsx` | 7 passed |
| Main-thread tripwire | `cargo test --manifest-path src-tauri/Cargo.toml --lib main_thread_tripwire` | 3 passed (`no_synchronous_tauri_command_blocks_the_main_thread`, `tripwire_scans_the_kit_crate_too`, `tripwire_detects_a_transitive_blocker`) |

The task named `src-tauri/tests/main_thread_tripwire.rs`; the tripwire lives at
`src-tauri/src/main_thread_tripwire.rs` (a `#[cfg(test)]` module of the lib), which is what ran.

### Approval-class coverage since 0.4.1

Agent tools in `src/agent/harness.ts` added since tag `v0.4.1`: `fl_round_plan`,
`fl_round_start`, `get_verified_source`, `widget_create`. Coverage:

| Tool | Annotation (`toolAnnotations.ts`) | On `READ_ONLY_AGENT_TOOLS` | Gate |
|---|---|---|---|
| `fl_round_plan` | none / trusted | yes | read-only, never asks (asserted) |
| `fl_round_start` | write / trusted | no | stops at a member approval (tripwire) |
| `get_verified_source` | none / untrusted | yes | read-only, never asks (asserted) |
| `widget_create` | write / trusted | no | stops at a member approval (tripwire) |

The tripwire in `agentToolGates.test.ts` enumerates `AGENT_TOOLS` at test time and invokes every
tool not on the reviewed read-only list, so a new tool is covered without editing the test.
`toolAnnotations.test.ts` asserts that a tool is `effect: none` exactly when it is on the
read-only list, and that a tool which asks the member is never annotated `none`. The 14
`effect: none` tools are exactly the 14 read-only tools. No tool was missing, so no red-green
addition was needed. Sidecar-hosted tools (`shell_run`, `fs_*`, `browser_*`, capsules) are not in
`harness.ts`; their HIC routing is tested in the runtime (`shell_run_tests.rs`,
`browser_session_tests.rs`, `grants_session_tests.rs`), outside this enumeration.

## 5. Mapping to `gates.yaml` (for the retro flip; not edited here)

| Criterion | What this file supplies | Suggested evidence path |
|---|---|---|
| `g0-formal` | All eight specs exist and are TLC-green. Five re-run to completion today (section 1); SkillPersistence, DeployGate and AnchorBatch were green on 2026-10-01 (`handoffs/HUP_FANOUT5_PARTIAL_2026-10-01.json` verify.M1). One mutant per spec for FolderGrant, SpendBudget, WebSigningBudget (section 2). | core `src-tauri/formal/`, runtime `agent-grants/formal/`, `agent-learn/formal/`, `agent-anchor/formal/`, cluster `formal/`, and this file |
| `g1-approval-audit` | Enumeration tests green on the release head; every tool added since 0.4.1 covered (section 4). | `src/shell/agentToolGates.test.ts`, `src/agent/toolAnnotations.test.ts`, this file |
| `g1-no-block` | Tripwire scans every `src/*.rs` (and the kit crate) at test time; green on the release head. | `src-tauri/src/main_thread_tripwire.rs`, this file |
| `g1-sidebar` | D-34 nav tests green on the release head (7). Screenshots are part of the S0.8 proof and are not re-taken here. | `src/shell/sidebarIa.test.tsx`, this file |
| US-1.3 AC2 / WP S1.3 | Self-review recorded as a labelled opinion; HTTP and hash verifiers; model planner (section 3). Merges with the M1/M2 integration of runtime `hup/n6-verify-formal`. | runtime `agent-loop/src/{lib,planner,workflows}.rs`, `agent-sidecar/src/verify_probes.rs` |

Open items that stay open: `metering.rs` still lists "self-review" under NOT_MEASURED, because
metering does not count opinions (they are in the session's event log, not the turn record). The
consented-origin branch of the HTTP probe has no automated test. No benchmark is owed (no chain
core crate changed).
