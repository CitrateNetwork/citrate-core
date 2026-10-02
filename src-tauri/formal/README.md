# SidecarSupervisor formal model (CORE-C1.0b)

TLA+ model of the `SidecarSupervisor` state machine
(`src-tauri/src/supervisor.rs`), written for CORE-C1.0b to pin the C1.0b fixes
(F-1 consecutive-not-lifetime counter, the fork-bomb bound) and to cover the
transitions the Rule-8 review (`citrate-security`
`reviews/2026-07-13-rule8-c1-0-supervisor`) flagged as untested. The supervisor
is the substrate for every Phase-C sidecar, and the example-based tests MISSED
F-1 — so this model checks the counter-reset semantics exhaustively.

## Files

- `SidecarSupervisor.tla` — the model (states, transitions, invariants).
- `SidecarSupervisor.cfg` — RECOVERING-child config (`CanHealthy = TRUE`):
  pins INV-1 (never permanently Failed while recovering) + the safety invariants.
- `SidecarSupervisor_ForkBomb.cfg` — CRASH-LOOP-ONLY config
  (`CanHealthy = FALSE`, never stopped): pins INV-2 (the fork-bomb bound) as both
  a safety invariant AND a liveness property (the run actually reaches Failed).

## States and transitions

States `{Off, Starting, Running, Backoff, Failed}` mirror `SupervisorState`.
Transitions map 1:1 to `run_monitor` arms: `SpawnOk`, `SpawnFail`,
`BecomeHealthy` (the F-1 reset), `Crash`, `Unhealthy`, `TryWaitError`,
`RetryFromBackoff`, `Stop`, `StopInBackoff` (cancels a pending restart),
`Shutdown`.

Modeling fidelity for F-1: in the recovering config a `Crash` can only occur
AFTER the episode became sustained-healthy (`healthyRun` = TRUE), which is the
real distinction — a long-lived daemon runs past `healthy_after` between crashes
(resetting the counter), while a fast crash-loop exits before it (never resets).

## Invariants and the Rust tests they mirror

| Invariant | Meaning | Rust test(s) (`supervisor_tests.rs`) |
|---|---|---|
| **INV-1** `INV_NeverFailedWhenRecovering` | An intermittently-crashing-but-recovering child is NEVER permanently Failed (the F-1 property). | `intermittent_crashes_with_healthy_runs_never_permanently_fail` |
| **INV-2** `INV_CounterBounded` + `EventuallyFailedIfNeverHealthy` | A crash-loop-only child (no healthy interval) reaches Failed within `max_retries` (the fork-bomb bound). | `crash_loop_without_healthy_interval_still_hits_cap_after_reset_fix`, `fork_bomb_is_bounded_and_backoff_increases`, `fork_bomb_bound_is_load_bearing`, `spawn_failure_reaches_failed_within_bound` |
| **INV-3** `INV_StopIsNotACrash` (+ Stop/StopInBackoff/Shutdown keep `crashes` UNCHANGED) | An intentional stop never triggers a restart and never records a crash. | `graceful_stop_kills_child_and_does_not_restart`, `stop_during_backoff_cancels_pending_restart` |
| **INV-4** `INV_NoOrphan` | Off/Failed implies no live child (no orphan). | `drop_kills_child_no_orphan`, `drop_with_bounded_join_returns_promptly` |

F-2 (the non-blocking health probe + bounded teardown) is a liveness property of
the implementation rather than a state-machine invariant; it is pinned by the
Rust tests `hanging_health_probe_does_not_stall_crash_detection_or_stop` and
`drop_with_bounded_join_returns_promptly`. The model captures the related
transition faithfully: the probe is an event that cannot pre-empt the crash/stop
transitions.

## Running TLC

Requires Java + `tla2tools.jar`. On the build machine these were at
`/opt/homebrew/opt/openjdk/bin/java` and `~/.local/share/tla/tla2tools.jar`.

```sh
JAR=~/.local/share/tla/tla2tools.jar
JAVA=/opt/homebrew/opt/openjdk/bin/java   # or `java` if on PATH
cd src-tauri/formal

# INV-1 + safety (recovering child)
"$JAVA" -cp "$JAR" tlc2.TLC -config SidecarSupervisor.cfg SidecarSupervisor.tla

# INV-2 fork-bomb bound (safety + liveness, crash-loop-only child)
"$JAVA" -cp "$JAR" tlc2.TLC -config SidecarSupervisor_ForkBomb.cfg SidecarSupervisor.tla
```

## Run result (2026-07-13)

Both configs were run headless with TLC 2026.05.26:

- `SidecarSupervisor.cfg` — **No error found** (52 distinct states; INV-1..4 +
  TypeOK hold).
- `SidecarSupervisor_ForkBomb.cfg` — **No error found** (12 distinct states;
  INV-2 + INV-4 + the `<>Failed` liveness property hold).

Negative control (model has teeth): removing the counter reset in `BecomeHealthy`
(re-introducing the F-1 lifetime-counter bug) makes TLC report
`Invariant INV_NeverFailedWhenRecovering is violated` with a concrete
counterexample trace reaching Failed — the exact bug the example tests missed.

Note: the `crashes` counter is bounded by `MaxCrashes` (a `CONSTRAINT` in the
primary cfg) purely to keep the state space finite; it does not weaken the
invariants (INV-1..4 are inductive on `state`/`failures`).

---

# MemoryPack formal model (Hermes P1 / WP1.1)

TLA+ model of the docs-corpus packer (`docs_ingest::ingest_docs_incremental` +
`memory::ingest_docs_corpus`). Pins the "monotone, no dupes" property the
content-hash seen-set provides, so a corpus that GROWS across app versions
(WP1.2's reference packs added to the Almanac docs) re-packs only the new chunks.

## Files
- `MemoryPack.tla` — states (`corpus`, `packed`, `seen`, `lastRun`) + actions
  (`Ingest`, `GrowCorpus`, `WipeStore`).
- `MemoryPack.cfg` — a 4-hash universe; checks TypeOK + NoDupes + Integrity and
  the `MonotoneUnderIngest` action property.

## Invariants and the code they mirror
- **INV-Pack-1 (monotone)** — `MonotoneUnderIngest`: an `Ingest` step only grows
  `packed`. Mirrors `ingest_docs_incremental` unioning `corpus \ seen`. `WipeStore`
  is the one intentional exception (a wiped store), modeled explicitly.
- **INV-Pack-2 (no dupes)** — `packed`/`seen` are sets and `Ingest` only adds
  `corpus \ seen`, disjoint from `seen`. Mirrors the sha256 seen-set skip.
- **INV-Pack-3 (integrity)** — `Integrity`: `packed \subseteq corpus`; the packer
  never invents a node (Rule 1).

## Run result (2026-09-11)
Run headless with TLC (`tla2tools.jar`, same harness as above):
- `MemoryPack.cfg` — **No error found** (221 distinct states; TypeOK + NoDupes +
  Integrity + the MonotoneUnderIngest property hold).

Also present: `ModelRouter.tla`/`.cfg` (Hermes P0 / WP0.1 — the model-router
selection invariants).

---

# ConsentGate formal model (Telemetry WP-T.1)

TLA+ model of the telemetry consent gate — the property `telemetry_send` (the one pinned
HTTPS POST, `telemetry.rs`) must satisfy: **nothing egresses except a bundle the member
reviewed AND consented to, and only while the telemetry toggle is on.**

## Files
- `ConsentGate.tla` — states (`toggle`, `reviewed`, `consented`, `egressed`, `reviewedBody`,
  `sentBody`) + actions (`ToggleOn`/`ToggleOff`, `Review`, `Consent`, `Egress`).
- `ConsentGate.cfg` — a 2-bundle universe; checks TypeOK + INV_Consent_1 + INV_Consent_3 and
  the INV_Consent_2 action property.

## Invariants
- **INV-Consent-1** — every egressed bundle was consented (`egressed ⊆ consented`). Mirrors
  `telemetry_send` being called only from the consented review flow (WP-T.4).
- **INV-Consent-2** — no egress step occurs while the toggle is off (`Egress` guards on
  `toggle = TRUE`). Toggle-off keeps the historical consent record; it doesn't erase it.
- **INV-Consent-3** — what was sent equals what was reviewed (`sentBody = reviewedBody`, nonzero)
  — the UI sends exactly the bundle it displayed.

## Run result (2026-09-11)
Run headless with TLC (`tla2tools.jar`): **No error found** (32 distinct states; TypeOK +
INV_Consent_1 + INV_Consent_3 + the INV_Consent_2 property hold). Negative control (the model
has teeth): an earlier draft cleared `consented` on `ToggleOff`, and TLC produced a concrete
counterexample violating INV-Consent-1 (a legitimately-sent bundle no longer showed as
consented) — fixed by keeping consent as a historical record.

---

# AgentLoop formal model (HUP-S1.3)

TLA+ model of Hermes's verifier-judged loop (`citrate-agent-loop`: `run_workflow` /
`run_turn_with`; ADR `docs/adr/ADR-2026-09-30-hermes-loop-in-sidecar.md`). A workflow has
`NSteps` steps; each step gets `MaxAttempts` attempts of up to `MaxTurns` model calls; a model call
answers (verifiers judge the attempt) or proposes an effect, which passes a gate (human approve,
human deny, or HIC-2 auto); reading untrusted content taints the task; Stop can arrive any time.

| Property | Meaning | Rust tests (`agent-loop/tests/`) |
|---|---|---|
| `Bounded` | model calls ≤ NSteps × MaxAttempts × MaxTurns | `the_loop_is_bounded_by_max_steps` |
| `OnlyVerifierSucceeds` | succeeded ⇒ every step verified | `the_models_claim_of_success_is_not_success`, `steps_run_in_order_and_all_must_pass` |
| `NoEffectWithoutGate` | every executed effect passed a gate; a denied effect never executes | `a_denied_tool_is_reported_to_the_model_as_declined`, `a_declined_tool_does_not_count_as_succeeded` |
| `TaintDowngrade` | no auto-approved effect after untrusted content entered the task | (enforcement lands with HIC budgets in S2.7) |
| `StopIsLive`, `Terminates` | a stop request always halts; every run ends | `stop_during_a_tool_halts_before_the_next_model_call`, `stop_during_a_workflow_stops_it` |

**Run:** `scripts/run-tlc.sh AgentLoop` (needs a JDK: `brew install openjdk`, and `~/.tla/tla2tools.jar`).
**Result (2026-09-30, TLC 2.19, OpenJDK 27):** 2,342 states generated, 1,258 distinct, depth 27,
no error; both temporal properties hold. **Mutation checks:** removing the taint guard violates
`TaintDowngrade`; letting a failed judgement mark success violates `OnlyVerifierSucceeds`.
`NoEffectWithoutGate` holds by construction (the model has no ungated execution path) — it
documents the design rather than being mutation-tested.

Re-checked on the same date with this script: `ConsentGate`, `MemoryPack`, `ModelRouter`,
`SidecarSupervisor` and `SidecarSupervisor_ForkBomb` — no error.

# WebSigningBudget formal model (HUP-S2.3, formal half)

Implementation (HUP-S2.3) and the test that pins each property: `docs/WEB_SIGNING_BUDGETS.md`.

TLA+ model of the signing side of the **proposed** Rule-3 amendment,
`docs/adr/ADR-2026-09-30-rule3-budgetable-signatures.md` (section D9). It models a design,
not code: nothing in the tree calls a budget, the ADR's sign-off block is not complete (the
formal-methods row is open), and every signature is still HIC-1. The spec is the gate the ADR sets before any B-1 or B-2 wiring can merge.

## Files

- `WebSigningBudget.tla`: the model.
- `WebSigningBudget.cfg`: the ADR's small bounds, every request kind (SIWE, x402, `Tx`,
  `Permit`), strict taint rule, 2 requests, symmetry on recipients and nonces.
- `WebSigningBudget_Siwe.cfg`: SIWE only, 3 requests, 2 origins (1 allowlisted), 3
  provenances, domain/URI binding, Resources, 3 nonces (so a third sign-in to one origin,
  after a revoke and re-grant, can reach the per-origin rolling rate).
- `WebSigningBudget_X402.cfg`: x402 only, 4 requests, so the per-signature, rolling
  per-recipient, rolling global and `max_count` caps all bind while the window slides.
- `WebSigningBudget_O3.cfg`: owner decision O-3 accepted (same-origin content does not taint a
  sign-in to that origin), taint from either origin or `"ext"`. The owner accepted the O-3
  exemption on 2026-09-30 (ADR sign-off block), so this config checks the accepted reading;
  the other configs keep the strict rule, which is the stronger property.
- `WebSigningBudget_Live.cfg`: liveness (`FallThroughLive`) without symmetry, smaller bounds.
- `WebSigningBudget_mutants.py`: the mutation check (below).

## What is modelled

The closed list (D1); the SIWE checks of D2 that carry a safety property (core-attested
top-frame origin, managed browser only, allowlist, domain and URI binding, nonce ledger, no
Resources, per-origin rolling rate and burst gap, taint); the x402 cap model of D3
(per-signature max, recipient pinning, rolling per-recipient and global windows, `max_count`,
expiry, write-ahead reservation that is never refunded); budgets, grant, revoke and
"Stop all autonomy" under the dedicated budget lock (D4); write-ahead decision records; crash
and recovery with `outcome_unknown` (D8); a bounded clock with rolling windows of `W` ticks.

`request_budgeted` is three steps under one lock acquisition: `Decide` (check, reserve, and
write the record atomically, or fall through to HIC-1), `Sign` (the gated signer), `Return`
(close the record, release the lock). `Crash` can happen between any two of them.
`Revoke`, `RevokeAll` and `Grant` take the same lock, so a revoke commits only between two
auto-sign critical sections.

## Invariants and properties

| Name | Statement | Mutant that breaks it |
|---|---|---|
| `OnlyClosedList` | every auto-sign is `Siwe` or `X402`; a `Tx` or `Permit` never is | M01: x402 check also accepts `Permit` |
| `OriginBound` | SIWE domain = URI origin = attested origin, and it is allowlisted | M02: drop URI binding; M03: drop the allowlist at grant and at check |
| `TopFrameOnly` | no SIWE auto-sign from an iframe/popup/worker or in attach mode | M04: drop the provenance check |
| `NonceUnique` | no (origin, nonce) pair is auto-signed twice | M05: drop the ledger check |
| `NoCapabilityDelegation` | no auto-signed SIWE carries Resources (ReCap) | M06: drop the Resources check |
| `RecipientPinned` | the x402 payee is the recipient pinned in the paying budget | M07: drop the `to = recipient` check |
| `NeverExceedsCaps` | `used ≤ max_count`; value ≤ per-signature max; per-recipient and global sums ≤ caps in **every** window up to now; SIWE per-origin window count and burst gap | M08: per-signature; M09: `max_count`; M10: calendar-day window instead of rolling; M11: global cap; M12: SIWE burst gap; M23: SIWE per-origin rolling rate |
| `ReservedBeforeSigned` | there are never more signatures than reservations | (structural) |
| `RevokeImmediate` | no auto-sign under a (budget, generation) after its revoke committed (history variable) | M13: revoke without taking the budget lock |
| `ExpiredInert` | no auto-sign at or after the budget's expiry | M14: no expiry re-check before the signer |
| `TaintDowngrade` | tainted context never auto-signs (O-3 exemption: SIWE only, same origin only) | M15: no taint re-check before the signer; M16: no taint check at all; M17: exemption widened to any web origin and to x402 |
| `RecordBeforeSignature` | every auto-sign has its decision record, written before the signature | M18: record written at return (write-behind) |
| `NoFalseNegative` | no record says `not_signed` when a signature exists for it | M19: recovery marks open records `not_signed` |
| `NoDrop` | every request is accounted for: HIC-1 pending, signed, `outcome_unknown`, lost in a crash before core decided it, or in progress | M20: an ineligible request is silently rejected |
| `BudgetMonotone` (action) | within a generation `used` never drops, expiry never extends, a revoke is never undone; reservations, ledger and auto-sign history only grow, across crashes | M22: a crash resets `used` |
| `FallThroughLive` (temporal) | a waiting request is always decided, the lock is always released, a crash is always recovered | M21: only eligible requests are ever decided |

## Run results (2026-09-30 local, TLC 2.19 rev 5a47802, OpenJDK 27, 12 workers)

`scripts/run-tlc.sh WebSigningBudget all` (33 min wall clock for all five):

| Config | Generated | Distinct | Depth | Time | Result |
|---|---|---|---|---|---|
| `WebSigningBudget.cfg` | 37,916,870 | 7,828,872 | 21 | 1 min 36 s | no error |
| `WebSigningBudget_Live.cfg` | 10,609,496 | 2,361,774 | 21 | 8 min 11 s | no error (temporal properties hold) |
| `WebSigningBudget_O3.cfg` | 131,030,726 | 23,191,496 | 24 | 5 min 59 s | no error |
| `WebSigningBudget_Siwe.cfg` (3 nonces, review fix, 2026-10-01Z) | 21,454,563 | 5,077,204 | 26 | 54 s | no error |
| `WebSigningBudget_X402.cfg` | 381,276,329 | 95,058,700 | 31 | 16 min 50 s | no error |

Mutation check, `python3 src-tauri/formal/WebSigningBudget_mutants.py` (about 40 s): **23 of
23 mutants caught**, each by the invariant it targets, with counterexamples of 4 to 15 states.
M13's trace is the race the dedicated lock exists for: Decide (auto-sign holds the lock),
Revoke commits, Sign. One bound note from the first pass: with `max_count` 2 and only 2
nonces, the SIWE config can never reach a third sign-in to one origin, so M09 is checked in
the x402 config, where the same `BudgetLive` guard binds. Review note: for the same reason the
per-origin rolling rate (`SiweWindowMax`) never bound in any config with 2 nonces, and removing
its guard went undetected; the SIWE config now has 3 nonces and M23 covers that guard.

## Abstractions (what this model does not prove)

- Deterministic message checks with no cross-request state (strict EIP-4361 parse and
  re-serialization, Version, Chain ID, address, Issued At / Not Before skew, statement
  length) are not separate variables. Any failing check takes the same HIC-1 branch as the
  modelled ones; their correctness is a parser property for unit and property tests.
- The x402 authorization nonce is core-generated and unique by construction; not modelled.
- One member and one principal. Cross-principal isolation (red-team correction 6) is not here.
- The wall-clock-backwards suspension (D8) is not modelled: the model clock is monotonic truth.
- Budget voiding on wallet unlink, reroll, failed re-verification or a sidecar identity change
  is modelled only as `RevokeAll`.
- No refinement mapping to `ConsentGate.tla` yet (D9 says it refines it where the gates overlap).
- The x402 per-request bindings of D3 other than recipient pinning and value (`validity_max`,
  i.e. `validBefore - now`, `validAfter <= now`, `from`, `chainId`, and the asset-domain
  allowlist) are not modelled: `Values` are base units of one allowlisted asset. Like the SIWE
  parse checks, they are per-request and stateless and belong in unit and property tests.
- No invariant distinguishes a grant that takes the budget lock from one that does not (A-6):
  with `Idle` removed from `Grant`, `RevokeImmediate` still holds in the combined config,
  because a grant only opens a new generation. The lock on grant is the safer reading, not a checked property.

## Ambiguities in the ADR (the model takes the safer reading; owner to confirm)

- **A-1 nonce ledger pruning vs `NonceUnique`.** D2 #12 prunes ledger entries after
  expiration plus skew; D9 states `NonceUnique` over all of `signed`. With pruning, a site that
  reuses a nonce in a later message would be auto-signed again. The model never prunes. Either
  keep the ledger for the budget's lifetime (it is bounded by `max_count`), or restate
  `NonceUnique` as "within the retention window".
- **A-2 when taint and expiry are evaluated.** D7 runs the checks once, at the start of the
  locked section. Taint can arrive (and the clock can pass `expires_at`) while the lock is held.
  The model re-reads both immediately before the gated signer and, if either now fails, closes
  the record as `not_signed` and falls through to HIC-1. Mutants M14 and M15 show that without
  the re-check, `ExpiredInert` and `TaintDowngrade` fail.
- **A-3 reservation and record in one write.** D3 says a reservation is never refunded; D4 says
  if the record cannot be written nothing is signed; D8 says a crash after reservation yields an
  `outcome_unknown` record. These agree only if the reservation and the record are persisted in
  one atomic write. If the reservation is persisted first, a crash between the two leaves a
  reservation with no record to mark. The model uses one atomic write.
- **A-4 windows across grants.** `per_recipient_window_max` is listed as an `X402Budget`
  field. The model counts the rolling per-recipient sum (and the SIWE per-origin rate) across
  every generation of the budget, so revoke-and-regrant cannot reset a window.
- **A-5 when a revoke "has occurred".** `RevokeImmediate` holds from the moment the revoke
  commits (acquires the lock). An auto-sign already holding the lock when the member clicks
  completes. The UI should report "revoked" only after `budget_revoke` returns.
- **A-6 grant takes the budget lock.** D4 says revoke does; grant is not stated. The model has
  grant take it too, so a grant never interleaves with a check-reserve-sign sequence.
- **A-7 a request lost in a crash.** A request waiting for the lock when core crashes was never
  decided, so it is neither pending nor recorded (`NoDrop` counts it as lost). The sidecar sees
  a broken connection and must retry; D2's "never silently dropped" should say so.
- **A-8 lock naming.** D8 says concurrent requests serialize "on the ceremony lock"; D4 says a
  dedicated budget lock, not the pending-map lock. The model uses the dedicated budget lock.

# DeployGate (HUP-S6.4)

Model of the D-4 deploy gate (`src/deploy_gate.rs`, `src/contract_deploy.rs`).

## Files

- `DeployGate.tla`: the model.
- `DeployGate.cfg`: the safety run (2 init codes, 3 deploy attempts).
- `DeployGate_Reach.cfg`: non-vacuity. TLC is expected to report `NeverSigns` violated,
  which shows a deploy can be signed at all.
- `DeployGate_mutants.py`: the mutation check.

## What maps to what

| Model | Code |
|---|---|
| `gate[c]` (latest verdict per code; hashing as identity) | `GateStore.records`, keyed by keccak256(init code) |
| `Evaluate(c, v)` under the store lock; NOT_READY rejects pending ceremonies for `c` | `deploy_gate_submit` → `GateStore::record_and_revoke` |
| `Check(i)` (read the bytes once, take the lock, require READY) then `Open(i)` (ceremony from the same bytes, release) | `contract_deploy_sync` → `GateStore::open_ceremony` |
| `Approve(i)` / `Reject(i)` | the SignatureCeremony approve / reject |

## Invariants

| Invariant | Meaning | Rust tests |
|---|---|---|
| `NotReadyNeverSigns` | every signature was made while the latest verdict for exactly the signed bytes was READY | `store_not_ready_refusal_names_every_failing_item`, `a_not_ready_record_rejects_the_open_ceremonies_for_that_hash`, `open_ceremony_refuses_a_not_ready_record_and_never_calls_the_opener` |
| `OpenOnlyForReadyBytes` | a ceremony that can still be approved carries bytes whose own verdict is READY; a READY for an earlier version of the bytecode never covers a new one | `store_ready_record_allows_exactly_that_initcode`, `a_later_not_ready_record_revokes_an_earlier_ready_for_the_same_hash` |
| `NoTOCTOU` | the bytes a ceremony carries are the bytes the gate check read | `contract_deploy_requires_a_ready_gate_before_any_ceremony` (source check: one parse, the gated `initcode` is what the tx carries) |

## Run result (2026-10-01)

TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802), `scripts/run-tlc.sh DeployGate`:
**No error found**, 17,438 states generated, 3,378 distinct, depth 15.

`scripts/run-tlc.sh DeployGate DeployGate_Reach`: `NeverSigns` violated as expected
(a signing trace exists).

Mutation check (`python3 src-tauri/formal/DeployGate_mutants.py`), all 7 killed:

| Mutant | Break | Caught by |
|---|---|---|
| M01 | `Open` re-reads the source instead of the checked bytes | `NoTOCTOU` |
| M02 | same, seen per hash | `OpenOnlyForReadyBytes` |
| M03 | NOT_READY does not reject open ceremonies | `NotReadyNeverSigns` |
| M04 | `Evaluate` ignores the store lock (lands between check and open) | `OpenOnlyForReadyBytes` |
| M05 | `Check` accepts any recorded verdict | `NotReadyNeverSigns` |
| M06 | `Check` accepts a code with no record | `NotReadyNeverSigns` |
| M07 | `Approve` signs a rejected ceremony | `NotReadyNeverSigns` |

## Abstractions

- keccak256 is assumed collision-free (codes stand for hashes).
- Compiler settings are folded into the code identity (they are part of `binding_hash`).
- The evidence parsers are folded into the verdict `Evaluate` writes; the Rust tests cover
  them (every "not installed" or tool error is a failing item).
- The store's size bound is not modelled. Eviction revokes open ceremonies like NOT_READY.

# DaemonBudget formal model (HUP-S10.3)

Model of daemons inside a budget: `src-tauri/src/daemons.rs` (`DaemonBook`), the runner
(`src/daemons/runner.ts`) and the daemon turn's HIC rules (`src/daemons/turn.ts`). Design:
`docs/WIDGETS_AND_DAEMONS.md`.

- `DaemonBudget.tla`: the model. `Fire` (a schedule minute passes), `Claim` (the runner's tick,
  only when not paused, nothing in flight and the day's run and token budgets are not used up;
  the allowance is `min(PerRun, MaxTokens - tokens)`), `Skip` (due but not claimable), `Finish`
  (charged what it used, at most the allowance plus one model round, because the runner stops a
  run once its estimate passes the allowance), `Abandon` (a run never reported back, charged its
  allowance), pause and resume (one or all; resuming drops missed minutes), `Propose` and
  `Decide` (an effect a run proposes runs only on Approve), `NewDay`. There is no spend action.
- `DaemonBudget.cfg`: two daemons, two days, `MaxRuns = 2`, `MaxTokens = 3`, `PerRun = 2`,
  `Over = 1`, two effects.
- `DaemonBudget_mutants.py`: the mutation check.

| Invariant | Meaning | Rust / TS tests |
|---|---|---|
| `RunsWithinCap` | runs today never exceed the day's cap | `the_daily_run_budget_runs_out_and_says_so` |
| `TokensBounded` | tokens today exceed the cap by at most one model round | `a_run_that_overshoots_is_charged_what_it_used`, runner `stops a run at its token allowance` |
| `AllowanceWithinDay` | a run's allowance never reaches past what is left of the day | `the_daily_token_budget_runs_out_and_caps_the_last_run` |
| `NoEmptyRun` | no run starts with nothing left | `the_daily_token_budget_runs_out_and_caps_the_last_run` |
| `SpendZero` | a daemon never spends | `a_spend_budget_above_zero_is_refused`, `daemonAvailability` (local model only) |
| `OneInFlight` | one run in flight per daemon | `only_one_run_of_a_daemon_is_in_flight` |
| `NoStartWhilePaused` | a paused daemon (or "pause all") never starts a run | `a_paused_daemon_never_fires_and_does_not_catch_up_on_resume`, `pausing_everything_stops_every_daemon` |
| `EffectsOnlyApproved` | nothing a daemon proposes runs without the member's approval | turn `marks every other tool HIC-required`, sidecar `an_unattended_session_asks_before_its_first_effectful_call` |

Run (2026-10-01, TLC2 2.19, `scripts/run-tlc.sh DaemonBudget`): **no error**, 11,655,489 states
generated, 909,376 distinct, depth 23, 9 s wall clock.

Mutation check (`python3 src-tauri/formal/DaemonBudget_mutants.py`), all 10 killed:

| Mutant | Break | Caught by |
|---|---|---|
| M01 | `Claim` ignores the run budget | `RunsWithinCap` |
| M02 | `Claim` ignores the token budget | `NoEmptyRun` |
| M03 | the allowance is `PerRun` even near the day's end | `AllowanceWithinDay` |
| M04 | the runner does not stop a run at its allowance | `TokensBounded` |
| M05 | `Claim` ignores a run in flight | `OneInFlight` |
| M06 | `Claim` ignores a paused daemon | `NoStartWhilePaused` |
| M07 | `Claim` ignores "pause all" | `NoStartWhilePaused` |
| M08 | a declined effect runs | `EffectsOnlyApproved` |
| M09 | a proposed effect runs at once | `EffectsOnlyApproved` |
| M10 | a run spends | `SpendZero` |

Abstractions: schedules, the time of day, the 24 h catch-up window and the 30 minute stale-run
timer are folded into the nondeterministic `Fire` and `Abandon`; token amounts are small naturals.

# FlRoundGate (HUP-S9.4)

Federated round start (HIC-1) and the LoRA eval gate, mirroring `src-tauri/src/fl_rounds.rs`.

## Files

- `FlRoundGate.tla`: the model.
- `FlRoundGate.cfg`: safety (one plan, since plans are independent; two source files; two adapter
  versions; two base models).
- `FlRoundGate_Reach.cfg`, `FlRoundGate_ReachLoad.cfg`: non-vacuity (a round can start; an
  adapter can load).
- `FlRoundGate_mutants.py`: the mutation check.

## What maps to what

| Model | Code |
|---|---|
| `Plan(p)` | `fl_round_plan` (`build_plan` over a coordinator read; the plan hash binds what was read) |
| `Approve(p)` | the member's Approve click on the plan's card (`store.ts`, `fl_round_start` tool and the Train surface) |
| `Start(p)` | `start_round`: plan known and startable, not already started, coordinator re-read still open, settlement unchanged |
| `CoordChange` | the coordinator moving at any time |
| `Gate(f, d, b)` | `fl_adapter_gate` (`evaluate_adapter` + `record_gate`, keyed by sha256) and `must_unload_after_gate` |
| `Load(v)` | `fl_adapter_load` (`authorize_load`: ACCEPT, same base, content-addressed copy re-hashed) |
| `Swap(f, v)` | the member's source file changing on disk |
| `DamageCopy(v, w)` | an app-store copy damaged while not served |
| `CrashRestart` | the supervisor respawning llama-server with the same `--lora` argv |
| `SelectBase(b)` | `select_model` dropping the adapter on a base change |

## Invariants

| Invariant | Meaning | Rust tests (`fl_rounds_tests.rs`) |
|---|---|---|
| `StartOnlyApproved` | no start without the member's approval of that plan | `start_requires_a_plan_core_built` (the command is reached only from the approval path) |
| `StartOnlyWhatWasApproved` | a start happens only while work is open, as planned, under the settlement mode the member saw | `start_refuses_when_the_coordinator_changed_since_the_plan`, `start_refuses_a_settlement_mode_change`, `start_refuses_a_blocked_plan` |
| `AtMostOneStart` | one authorization per plan | `start_refuses_a_stale_plan_and_a_second_start` |
| `LoadedIsAccepted` | the configured adapter's latest record is ACCEPT for the served base | `load_needs_an_accepted_gate_for_this_exact_file_and_base`, `a_later_reject_revokes_an_earlier_accept`, `a_new_gate_record_unloads_the_served_adapter_unless_it_still_fits`, `serve_argv_carries_the_loaded_adapter_and_a_model_switch_clears_it` |
| `ServedIsLoaded` | what llama-server actually read is the gated version, across source swaps and crash restarts | `a_source_swapped_after_the_gate_is_refused_and_after_load_is_not_served` |

## Run result (2026-10-01)

TLC2 Version 2.19 (rev 5a47802), `scripts/run-tlc.sh FlRoundGate`: **No error found**,
24,204,496 states generated, 881,600 distinct, depth 15 (19 s).

`FlRoundGate_Reach`: `NeverStarts` violated as expected. `FlRoundGate_ReachLoad`: `NeverLoads`
violated as expected.

The first draft of this model found a gap: re-gating a served adapter as ACCEPT with scorecards
from a different base left it loaded. The fix is `must_unload_after_gate` (mutant M09 below is
that first draft).

Mutation check (`python3 src-tauri/formal/FlRoundGate_mutants.py`), all 13 killed:

| Mutant | Break | Caught by |
|---|---|---|
| M01 | start without approval | `StartOnlyApproved` |
| M02 | start without re-reading the coordinator | `StartOnlyWhatWasApproved` |
| M03 | start under a settlement mode the member did not see | `StartOnlyWhatWasApproved` |
| M04 | a second start for one plan | `AtMostOneStart` |
| M05 | start a plan that was blocked when made | `StartOnlyWhatWasApproved` |
| M06 | load accepts any recorded verdict | `LoadedIsAccepted` |
| M07 | load ignores the measured base | `LoadedIsAccepted` |
| M08 | a new gate record never unloads | `LoadedIsAccepted` |
| M09 | only a REJECT unloads (first draft) | `LoadedIsAccepted` |
| M10 | a base switch keeps the adapter | `LoadedIsAccepted` |
| M11 | an existing copy is reused without re-hashing | `ServedIsLoaded` |
| M12 | llama-server points at the member's source file | `ServedIsLoaded` |
| M13 | the copy is not re-hashed after copying | `ServedIsLoaded` |

## Abstractions

- sha256 is assumed collision-free (versions stand for hashes).
- Device fit, plan expiry (15 minutes) and the plan memory bound are not modelled; the Rust
  tests cover them.
- The app-owned adapter copy is assumed not to change while it is being served (the same trust
  the model files get). A copy damaged while not served is re-hashed and replaced on load.
- The eval decision itself is folded into the verdict `Gate` writes; `decide_eval_gate` is
  covered by the Rust tests.

# SpendBudget formal model (HUP-S1.5)

TLA+ model of the escalation spend budget (`src-tauri/src/escalation.rs`: `Book`, `Ledger`;
the sidecar half is citrate-agent-runtime `agent-escalation`). Written with the WP; the Rust unit
tests in `escalation_tests.rs` mirror each invariant.

## Files

- `SpendBudget.tla`: the model.
- `SpendBudget.cfg`: one endpoint, three escalations, prices {1, 2}, caps {0, 2, 3}, a clock over
  days 0..2 that also moves backwards.
- `SpendBudget_TwoEndpoints.cfg`: two endpoints (removal voids only that endpoint's quotes), two
  escalations, one day boundary. The mutation check runs on this config.
- `SpendBudget_mutants.py`: the mutation check (single worker, so the first violation is
  deterministic; property mutants run without TypeOK so an earlier state-invariant failure cannot
  hide the transition that breaks the property).

## Invariants

| Invariant | Meaning | Mutants that break it |
|---|---|---|
| `SpendWithinCap` | budgeted spend this period (`committed + reserved`) never exceeds the cap | M01 no cap check on a budget run; M02 the cap can be lowered below today's use; M03 settlement can charge more than the reservation |
| `NoEscalationWithoutShownPrice` | every run used exactly the price the webview showed for that quote | M04 the run does not echo the shown price |
| `EgressOptInOnly` | every run went to an endpoint the member had added at that moment | M05 removal keeps the endpoint's quotes and the run does not re-check the endpoint (each layer alone is redundant in the model; both are kept) |
| `OverBudgetOrTaintedNeedsHic1` | a run that did not fit, or ran with untrusted context, was the member's decision | M06 taint ignored; M07 cap ignored |
| `ReservedIsConsistent` | the in-flight counter equals this period's budget reservations | M08 an earlier period's reservation is settled against today |
| `ResetOnlyAtPeriodBoundary` (action) | committed spend drops only when the period advances | M09 the clock moving backwards rolls the period; M10 changing the cap resets spend |
| `PeriodMonotone` (action) | the period never moves backwards | M11 (as M09) |

## Run results (2026-10-01 local, TLC 2.19 rev 5a47802, OpenJDK)

`scripts/run-tlc.sh SpendBudget all`:

| Config | Generated | Distinct | Time | Result |
|---|---|---|---|---|
| `SpendBudget.cfg` | 51,284,760 | 6,926,616 | 6 min 10 s | no error |
| `SpendBudget_TwoEndpoints.cfg` | 1,898,882 | 243,840 | 3 s | no error |

Mutation check, `python3 src-tauri/formal/SpendBudget_mutants.py` (about 25 s): **11 of 11
mutants caught**, each by the invariant or property it targets.

## Abstractions

- Prices are abstract integers; the worst-case arithmetic, rounding and overflow are unit-tested.
- Persistence is assumed write-ahead and durable; the restart and corrupt-file paths are
  unit-tested (`a_corrupt_ledger_file_fails_closed_and_is_kept_aside`, round trip).
- Quote expiry only removes quotes and is unit-tested.
- The sidecar's own refusal of an under-reserved request is a second guard on `SpendWithinCap`,
  tested in the runtime, not modelled.

# ComponentSwap formal model (HUP-S5.5)

TLA+ model of the signed component updater's install path (`components/src/install.rs`,
`components/src/manifest.rs`): Begin (verify and record a manifest, refusing a lower sequence),
Verify (size, SHA-256, artifact signature and health check folded into one outcome), Rename,
Commit (the state-file rename, the only commit point), Crash at any step, Recover, Rollback.
Policy and runbook: `docs/COMPONENT_UPDATER.md`.

## Files

- `ComponentSwap.tla`: the model.
- `ComponentSwap.cfg`: three versions, one of them tampered, sequences 1..3.
- `ComponentSwap_Wide.cfg`: four versions (three good), sequences 1..4, so pruning is reachable.
- `ComponentSwap_mutants.py`: breaks one guard per mutant and expects TLC to find a violation.

## Invariants and the code they mirror

| Invariant | Meaning | Code / tests |
|---|---|---|
| `CurrentVerified` | Only a version that verified is ever current or previous | `Store::stage_and_swap`; `tests/install.rs` hash, size, signature, health cases |
| `InstalledOnDisk` | What the state file names is always on disk, through crashes | rename before commit; `Store::recover`; `recover_removes_leftover_staging_and_unreferenced_versions` |
| `OnlyVerifiedOnDisk` | Nothing unverified is placed among the version directories | staging under `.staging/` |
| `JobIsNewest` | An install comes from the newest recorded manifest | `install` re-checks the sequence; `install_refuses_a_manifest_older_than_the_one_recorded` |
| `BoundedDisk` | Between installs only current and previous are on disk | pruning after commit; `only_two_versions_are_kept_on_disk` |
| `SeenMonotone` (action property) | The recorded sequence never goes down | `check_sequence`; `an_older_sequence_is_a_rollback_and_is_refused` |

## Run result (2026-10-01)

`scripts/run-tlc.sh ComponentSwap all`: `ComponentSwap.cfg` 265 states generated, 130 distinct,
depth 12, no error; `ComponentSwap_Wide.cfg` 1119 generated, 517 distinct, depth 14, no error.
`python3 src-tauri/formal/ComponentSwap_mutants.py`: 9 of 9 mutants killed (M01..M09).

# AnchorSettle (HUP-S7.3, core half)

The runtime's `citrate-agent-runtime/agent-anchor/formal/AnchorBatch.tla` proves the batch side
(what a day's root covers, no re-batching, no double anchor in the ledger). `AnchorSettle.tla`
covers what core adds: the registry may not be deployed, the member turns anchoring on and off,
the scheduler raises one approval card per day, an approval is single use and signs once with the
anchor key, a receipt may stay unmined or revert, and a day is marked anchored only on a mined,
successful receipt.

## Files

- `AnchorSettle.tla`, `AnchorSettle.cfg` (2 days, up to 3 cards per day)
- `AnchorSettle_mutants.py` (one mutant per guard, each run with only its target invariant)

## Invariants and the code they mirror

| Invariant | Meaning | Code / tests |
|---|---|---|
| `AnchoredOnlyOnConfirmedReceipt` | a day is anchored only after a mined receipt with status 1 | `chain_agent::settle`, `ceremony::anchor::receipt_confirms`; `a_day_is_marked_anchored_only_on_a_mined_successful_receipt`, `a_reverted_or_unmined_receipt_never_confirms` |
| `SingleUseCard` | one approval, one signature | `AnchorCeremony::approve_and_broadcast` consumes first; `approve_signs_with_the_anchor_key_and_reports_the_receipt` |
| `NoSignatureBeforeDeploy` | nothing is signed for a registry that is not in the address book | `apply_settings`, `anchor_gate`; `nothing_can_be_turned_on_before_its_registry_is_deployed`, `a_tick_that_is_not_ready_touches_nothing` |
| `NoPendingCardWhileOff`, `NothingSignedWhileOff` | turning anchoring off drops pending cards unsigned | `drop_pending_when_off`; `turning_anchoring_off_drops_every_pending_card_unsigned` |
| `NoCardAfterAnchored` | an anchored day never gets another card | sidecar plan `already_anchored`; `request_from_plan` returns `None` for any plan that is not `ready` |
| `AtMostOneInFlight` | a day never has two anchor transactions waiting on the chain | `nightly_tick_with` skips in-flight days; `a_day_waiting_on_its_receipt_never_gets_a_second_card` |

## Run result (2026-10-01)

`scripts/run-tlc.sh AnchorSettle`: **No error found**, 8,271 states generated, 3,146 distinct,
depth 24.

Mutation check (`python3 src-tauri/formal/AnchorSettle_mutants.py`), all 7 killed:

| Mutant | Break | Caught by |
|---|---|---|
| M01 | settle accepts any receipt | `AnchoredOnlyOnConfirmedReceipt` |
| M02 | approve does not consume the card | `SingleUseCard` |
| M03 | the setting turns on (and cards are raised) before the registry is deployed | `NoSignatureBeforeDeploy` |
| M04 | turning off keeps pending cards | `NoPendingCardWhileOff` |
| M05 | same, seen as a signature while off | `NothingSignedWhileOff` |
| M06 | the scheduler ignores the ledger and the receipt | `NoCardAfterAnchored` |
| M07 | a new card while the last anchor's receipt is pending | `AtMostOneInFlight` |

## Abstractions

- The batch itself (root, proofs, pruning) is AnchorBatch's job and is not repeated here.
- Gas, the delegate binding and unattended (HIC-2) approval are open owner decisions (ADR O-5);
  the model has every anchor wait for an explicit approval, which is what the code does.
