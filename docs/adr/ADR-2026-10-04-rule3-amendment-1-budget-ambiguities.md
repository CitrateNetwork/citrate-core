---
created: 2026-10-04T00:00:00Z
branch: hup/n7-docs-almanac-retro
author: Larry Klosowski + Claude Opus 5.5
status: proposed (pending owner and @rule8 review; changes nothing until accepted)
planset: 2026-09-30-hermes-upskill
wp: HUP-S2.0 follow-up (retro action A17)
amends: ADR-2026-09-30-rule3-budgetable-signatures.md (accepted 2026-10-01; that file is not edited)
formal: src-tauri/formal/WebSigningBudget.tla, src-tauri/formal/README.md ("Ambiguities in the ADR")
implementation: kit/src/web_budget.rs, kit/src/ceremony.rs (request_siwe_budgeted)
---

# ADR-2026-10-04: Rule-3 amendment 1, the eight budget ambiguities (A-1 to A-8)

## Status

**Proposed, 2026-10-04.** Drafted for owner and @rule8 review as retro action A17
([RETRO-2026-09-30-fanout-2.md](../../.agentile/sprints/active/sprint-hup-s1-one-agent/RETRO-2026-09-30-fanout-2.md)).
It does not change any code. It writes into the ADR record the readings that
`WebSigningBudget.tla` already takes and that `kit/src/web_budget.rs` already implements, and it
names the two places where the implementation and the model differ, so a reviewer can accept or
correct each one.

The accepted ADR,
[ADR-2026-09-30-rule3-budgetable-signatures.md](ADR-2026-09-30-rule3-budgetable-signatures.md), is
not edited. When this amendment is accepted, the ADR's D-sections are read with the clarifications
below, and the README's "Ambiguities in the ADR" section points here.

## Context

While modelling D9, the S2.3 work found eight places where the ADR text allows more than one
reading (`src-tauri/formal/README.md`, "Ambiguities in the ADR"). The model took the safer reading
in each case and recorded it as an owner question. The budget store then shipped on those
readings, with two deliberate differences (A-1 and A-2 below). Leaving the questions open means the
ADR, the model and the code can drift apart without anyone noticing.

## Decision (proposed)

Each row says what the ADR left open, the reading proposed for adoption, and where the
implementation stands today. "Model" is `WebSigningBudget.tla`; "code" is `kit/src/web_budget.rs`
and `SignatureCeremony::request_siwe_budgeted` in `kit/src/ceremony.rs`.

| # | Open point | Proposed reading | Model | Code |
|---|---|---|---|---|
| A-1 | D2 #12 prunes the nonce ledger after expiration plus skew; D9 states `NonceUnique` over every signature | Restate `NonceUnique` as: no two auto-signed messages for one origin share a nonce **while either message is still valid** (before its `Expiration Time` plus skew). Pruning after that is allowed, because a replay of an expired message fails the expiry check before the ledger is consulted. Consequence to accept: after pruning, a **new** message from the same origin that reuses an old nonce can be auto-signed again within the budget (the README's A-1 note); it still costs one sign-in from `max_count`. | never prunes (stronger) | prunes entries whose `keep_until` has passed, inside `reserve` |
| A-2 | D7 runs the checks once at the start of the locked section; taint and expiry can change while the lock is held | Immediately before the gated signer, re-read the clock (budget and message expiry) and the budget's revocation state. If either fails, close the record as `not_signed` and fall through to HIC-1. Taint is read once per request, from the snapshot of the sidecar's live sessions that core takes when it builds the request (`web_signin.rs`); a session that becomes tainted after that affects its next request. A reviewer may instead require a live re-read (see below). | re-reads expiry and taint | `still_signable` re-reads expiry and revocation; taint is the snapshot taken with the request |
| A-3 | D3 (never refunded), D4 (no record, no signature) and D8 (crash gives `outcome_unknown`) agree only if reservation and record are written together | The reservation, the counter debit, the nonce, the window slot and the `Reserved` decision record are persisted in **one** write, before any signature. If that write fails, nothing is signed. | one atomic step | `reserve` builds the next file and persists it once; on failure memory is unchanged |
| A-4 | `per_recipient_window_max` and the SIWE per-origin rate: do windows reset on revoke and regrant? | Rolling windows count across every generation of a budget for the same origin or recipient. Revoking and granting again never resets a window. | counts across generations | SIWE: reservations are kept per origin, not per budget, so a regrant sees them. x402 (B-2): inert, the asset allowlist is empty; the rule binds when B-2 is enabled |
| A-5 | When has a revoke "occurred"? | A revoke takes effect when it acquires the budget lock. An auto-sign that already holds the lock completes. The UI says "revoked" only after `budget_revoke` returns. | as proposed | `revoke` takes the lock; the Budgets panel waits for the command |
| A-6 | D4 says revoke takes the budget lock; grant is not stated | Grant takes the budget lock too, so a grant never interleaves with a check, reserve and sign sequence. The model notes this is the safer reading, not a property TLC distinguishes. | grant takes the lock | `grant` takes the lock |
| A-7 | D2 says a request is "never silently dropped"; a request waiting for the lock when core crashes was never decided | Add to D2: a request that had not acquired the budget lock when core stopped was never decided and is not recorded. The caller sees a broken connection and must ask again. "Never silently dropped" covers every request that reached a decision. | `NoDrop` counts it as lost | no record exists for it; the caller sees an error |
| A-8 | D8 says requests serialize "on the ceremony lock"; D4 says a dedicated budget lock | It is the dedicated budget lock of D4, not the ceremony's pending-map lock. D8's wording is corrected to say so. | dedicated lock | the budget store's mutex is the lock; the pending-map lock is separate |

### Where model and code differ

- **A-1.** The model never prunes; the code prunes expired ledger entries. Under the proposed
  reading both satisfy the restated invariant. This row is also a real choice, not only a record:
  it accepts the weaker `NonceUnique` (nonce reuse by a site after the retention window is signed
  again) in exchange for a ledger that does not grow with the budget's lifetime. A follow-up model change (prune after
  `expires_at + skew` and check the restated `NonceUnique`) would make the model match the code; it
  is not a precondition for accepting this amendment.
- **A-2.** The model re-reads taint before signing; the code uses the taint snapshot taken when the
  request was built. The window between the two is the time a request waits for the budget lock.
  Under the proposed reading that window is accepted, because the request's own content was judged
  at snapshot time. If a reviewer prefers taint to be re-read from live session state immediately
  before the signer, that is a code change in `request_siwe_budgeted` (and a sidecar call under the
  lock) and should be a separate WP with a red test first. A-1 and A-2 are the two rows where accepting
  the amendment is a real choice rather than a record of the code.

## Consequences

- The accepted ADR, the TLA+ model and the code are described by one text.
- Nothing about a member's app changes: every reading above is either what the code already does or
  a statement about crash and UI timing.
- Two follow-ups are optional and named, not implied: the A-1 model change and, only if a reviewer
  asks for it, an A-2 live-taint re-read.

## Alternatives considered

- **Edit the accepted ADR in place.** Rejected: the accepted text carries five sign-off rows, and
  changing it after sign-off would hide what was signed. An amendment keeps both visible.
- **Keep the nonce ledger for the budget's lifetime (A-1, the other reading).** Viable, since the
  ledger is bounded by `max_count`. Not proposed because the expiry check already refuses a replay of
  an expired message and the code already prunes. The stronger reading would also refuse a new
  message that reuses an expired nonce; a reviewer who wants that guarantee should choose it, at the
  cost of keeping up to `max_count` ledger entries per budget until the budget expires.

## Sign-off

| Role | Name | Decision | Date |
|---|---|---|---|
| Federation lead | Larry Klosowski | pending | |
| @rule8 security reviewer | | pending | |
| Core + runtime maintainer | | pending | |
| Formal methods | | pending (optional; the A-1 model change is the natural moment) | |
