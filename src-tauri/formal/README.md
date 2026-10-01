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

# WebSigningBudget formal model (HUP-S2.3, formal half)

TLA+ model of the signing side of the **proposed** Rule-3 amendment,
`docs/adr/ADR-2026-09-30-rule3-budgetable-signatures.md` (section D9). It models a design,
not code: nothing in the tree calls a budget, the ADR is not accepted, and every signature
is still HIC-1. The spec is the gate the ADR sets before any B-1 or B-2 wiring can merge.

## Files

- `WebSigningBudget.tla`: the model.
- `WebSigningBudget.cfg`: the ADR's small bounds, every request kind (SIWE, x402, `Tx`,
  `Permit`), strict taint rule, 2 requests, symmetry on recipients and nonces.
- `WebSigningBudget_Siwe.cfg`: SIWE only, 3 requests, 2 origins (1 allowlisted), 3
  provenances, domain/URI binding, Resources, 2 nonces.
- `WebSigningBudget_X402.cfg`: x402 only, 4 requests, so the per-signature, rolling
  per-recipient, rolling global and `max_count` caps all bind while the window slides.
- `WebSigningBudget_O3.cfg`: owner decision O-3 accepted (same-origin content does not taint a
  sign-in to that origin), taint from either origin or `"ext"`.
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
| `NeverExceedsCaps` | `used ≤ max_count`; value ≤ per-signature max; per-recipient and global sums ≤ caps in **every** window up to now; SIWE per-origin window count and burst gap | M08: per-signature; M09: `max_count`; M10: calendar-day window instead of rolling; M11: global cap; M12: SIWE burst gap |
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
| `WebSigningBudget_Siwe.cfg` | 15,669,387 | 3,799,870 | 23 | 30 s | no error |
| `WebSigningBudget_X402.cfg` | 381,276,329 | 95,058,700 | 31 | 16 min 50 s | no error |

Mutation check, `python3 src-tauri/formal/WebSigningBudget_mutants.py` (38 s): **22 of 22
mutants caught**, each by the invariant it targets, with counterexamples of 4 to 13 states.
M13's trace is the race the dedicated lock exists for: Decide (auto-sign holds the lock),
Revoke commits, Sign. One bound note from the first pass: with `max_count` 2 and only 2
nonces, the SIWE config can never reach a third sign-in to one origin, so M09 is checked in
the x402 config, where the same `BudgetLive` guard binds.

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
