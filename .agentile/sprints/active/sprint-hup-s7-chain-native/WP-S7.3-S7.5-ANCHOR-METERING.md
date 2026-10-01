---
created: 2026-10-01T10:30:00Z
branch: hup/n4-anchor-metering
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S7
wp: HUP-S7.3, HUP-S7.5 (core halves; runtime wiring on the same branch name in citrate-agent-runtime)
---

# HUP-S7.3 + S7.5: nightly anchor scheduling and the daily metering report

Planset `2026-09-30-hermes-upskill`: 05_SPRINTS_AND_WPS (S7.3, S7.5), 04_FEATURES_BDD US-7.2 and
US-7.3, 02_ARCHITECTURE section 8, D-23 (amended RT-2), D-27. ADR:
`docs/adr/ADR-2026-09-30-rule3-budgetable-signatures.md` D5 (the anchor key). Sprint issue:
CitrateNetwork/citrate-federation#284.

## What landed

| Piece | Where | State |
|---|---|---|
| Anchor ceremony: a single-purpose signer with its own no-funds key (OS CSPRNG, OS keyring account `hermes-anchor-key-v1`), one card per day, consume-first approval, receipt reported honestly | `kit/src/ceremony/anchor.rs` (a child of the ceremony module) | implemented, tested with a scripted RPC (signer recovered from the raw transaction) |
| Nightly schedule: gate (deployed registry + member opt-in), plan via the sidecar, one card per ready day, in-flight days skipped, settle only on a mined status-1 receipt | `src-tauri/src/chain_agent.rs` | implemented; **ships off**: `AnchorRegistry` is not in the 40204 address book, so the scheduler thread never starts |
| Sidecar client for `/metering/daily` and `/anchor/*`; core hands the sidecar its metering, decision-record and anchor-ledger folders | `src-tauri/src/hermes_chain.rs` (child of `hermes.rs`) | wired |
| Commands `hermes_chain_status`, `hermes_chain_settings_set`, `hermes_metering_daily`, `hermes_anchor_approve`, `hermes_anchor_reject` (all async, off the main thread) | `lib.rs`, main-window ACL only | registered; pop-outs get none |
| Journal "Hermes report" panel: real numbers, unknowns as unknown, anchor status line, anchor toggle and BenchmarkRegistry sharing toggle (both off and disabled until deployed), pending anchor cards, owner sign-off list | `src/journal/HermesDailyReport.tsx`, `meteringView.ts`, `src/surfaces/Journal.tsx` | implemented |
| TLA+ `AnchorSettle` (core half of the AnchorBatch story) | `src-tauri/formal/AnchorSettle.*` | TLC green, 7 of 7 mutants killed |

## Why the sidecar route and not a core port

The task allowed either. The batch code (`citrate-agent-anchor`) lives in citrate-agent-runtime;
core has no dependency on that repo, and adding one would need a `[[drift]]` entry first (Rule 12)
and would leave two copies of the batching rules in the app (Rule 9). The sidecar already has the
bearer-authed loopback channel and the decision records live beside it. So the sidecar plans and
builds calldata, and core re-checks everything that crossed the process boundary: the anchor
ceremony accepts only `anchor(NightlyMerkle, commitment)` to the pinned registry on 40204 with no
value, and the signer rebuilds the transaction from the commitment alone. The sidecar never sees
the anchor key and cannot choose what is signed beyond the day's commitment.

## Evidence (2026-10-01)

| Gate | Result |
|---|---|
| Red first | kit `anchor_tests.rs` failed to compile (60 errors) before `anchor.rs`; `chain_agent` in-flight and turn-off tests failed to compile before the functions; `hermesDailyReport.test.tsx` failed (no module) before the panel |
| `cargo test -p citrate-core-kit --lib ceremony::anchor` | 13 passed |
| `cargo test --lib chain_agent` (src-tauri) | 16 passed |
| ACL, tripwire, hermes, ceremony, invoke-secret filters | 111 passed (main-thread tripwire green with the five new commands) |
| `npx vitest run` | 1013 passed, 9 skipped (10 new) |
| `npx tsc --noEmit` | clean |
| clippy 1.98.1 `-D warnings` (`citrate-core-kit`, `citrate-core`, all targets) | clean |
| `cargo fmt --all -- --check` | clean |
| TLC `AnchorSettle.cfg` | 3,146 distinct states, depth 24, no error |
| TLA+ mutants | 7 of 7 killed |
| Rust mutants (hand) | 12 run, 12 killed (registry pin, kind word, value, receipt rule, destination, commitment match, day conflict; settle, plan kind, in-flight skip, opt-in gate, zero address) |

Full-workspace numbers are in the sprint issue comment.

## Decisions taken while the owner was away (conservative, reversible)

1. Every anchor waits for an explicit approval of its own card (HIC-1 per day). ADR D5 allows an
   unattended nightly signature by the anchor key, but the delegate binding and the gas path
   (O-5) are open, so nothing signs unattended. Pending owner sign-off.
2. Gas: the anchor key holds no funds and nothing tops it up. Until O-5 is decided an approved
   anchor would fail for lack of gas; the schedule cannot start anyway while `AnchorRegistry` is
   not deployed. Pending owner sign-off.
3. Benchmark sharing builds the calldata (sidecar `POST /metering/benchmark`) but nothing submits
   it: per-metric `record()` calls from the member's account would be about fifteen approvals a
   day, and folding them into the nightly batch changes who `msg.sender` is. Pending owner
   sign-off.
4. Turning anchoring off drops pending cards unsigned. Turning a feature on is refused while its
   registry is not in the address book.
5. The address book tripwire `the_shipped_book_has_no_anchor_or_benchmark_registry_yet` fails on
   purpose when the redeploy lands, so the surface gets rechecked against the real contracts.

## Not done (honest)

- No decision records are written yet (agent-records has no writer), so the anchor status shows
  no closed days even once enabled. Wiring HIC decisions into `agent-records` is separate work.
- AnchorRegistry, BenchmarkRegistry and AgentSBT are not deployed on 40204 (federation F-4). No
  transaction was sent; the signer is proven against a scripted RPC, not a live chain.
- The anchored-day proof UI (US-7.2 AC3) is reachable through the sidecar's `/anchor/proof` but
  has no surface yet.
- D-27 measures outside the event stream (time to first token, tokens per second, SALT, gas,
  CPU/GPU/RAM, energy, labelled self-review) are listed as unknown in the report.

## Journal

The interesting failure was one the model found, not a test. Writing `AnchorSettle.tla` with an
`inflight` counter turned up that the scheduler would have raised a second card for a day whose
first anchor was sent but not yet mined: the sidecar still lists that day as awaiting
confirmation, because it only learns about the anchor once core confirms it. Nothing in the unit
tests exercised a slow receipt. The fix is small (skip in-flight days), and the mutant that
removes it is now caught both by TLC and by a Rust test. The other lesson was about where the
single-purpose rule should live: putting the decode check in the request path was not enough,
because the signer could still be handed different bytes later; rebuilding the transaction from
the commitment alone at signing time is what makes the restriction a property of the code rather
than of the caller.
