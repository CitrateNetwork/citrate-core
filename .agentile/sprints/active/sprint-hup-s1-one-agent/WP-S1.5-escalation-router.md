---
created: 2026-10-01T00:00:00Z
branch: hup/n4-escalation
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S1
wp: HUP-S1.5
---

# HUP-S1.5: the escalation router (user endpoints, daily spend budget, SpendBudget.tla)

Planset `2026-09-30-hermes-upskill`: 05_SPRINTS_AND_WPS (S1.5), 04_FEATURES_BDD (US-1.5),
02_ARCHITECTURE section 3, 03_TLA_SPECS (`SpendBudget.tla`). Unblocked for the user-endpoint part
by ADR-2026-09-30 (Rule-3 budgetable signatures, accepted). Sprint issue:
CitrateNetwork/citrate-federation#278. Runtime half: citrate-agent-runtime branch
`hup/n4-escalation` (crate `agent-escalation`, sidecar `POST /escalations`).

## What landed

| Item | Where | State |
|---|---|---|
| Member endpoints: label, OpenAI-compatible base URL (https, or http to loopback), model, price card (micro-USD per 1M tokens in/out); key sealed in the OS keyring under `escalation-endpoint:<id>`; never in a file, a view, or the sidecar's config | `src-tauri/src/escalation.rs` (`add_endpoint_with_key`, `remove_endpoint_with_key`) | implemented, unit-tested with an in-memory keyring |
| Daily spend budget per UTC day: `committed + reserved <= cap`, write-ahead reservation, settlement at reported usage capped at the reservation, member-confirmed spend tracked outside the cap, reset only when the UTC day advances | `escalation.rs` (`Ledger`, `Book::authorize`, `Book::settle`) | implemented, unit-tested |
| Quote before run: core prices the worst case for the exact text, names the destination, and a run must echo the quoted price (`PriceNotShown` otherwise); one run per quote; 5 min expiry | `Book::quote`, `Book::authorize` | implemented, unit-tested |
| HIC: over budget, untrusted context (`hic: "required"`), or an unreadable ledger needs the member's explicit approval card (HIC-1); within budget runs with a price notice (HIC-2) | `escalation.rs`; webview `src/agent/escalation.ts` | implemented; store-level tests drive `handleTool` |
| `escalate_plan` tool, offered to the sidecar session only when an endpoint exists (default tool list and the parity fixture unchanged); annotated `spend` / `untrusted`; the answer comes back fenced as untrusted data | `src/agent/escalation.ts`, `src/shell/store.ts` | implemented; offered only in the sidecar-loop preview |
| Sidecar call: core reads the key for one request and posts it to the sidecar's `POST /escalations`; the sidecar answers with `sent: false` when nothing left the machine, so core charges nothing | `hermes.rs` (`HermesManager::escalate`, `sidecar_escalate`); runtime `agent-sidecar/src/escalation.rs` | implemented both sides; the real round trip is tested per side (core with a recording control, sidecar with a real loopback HTTP endpoint), not end to end through a running app |
| Settings › Escalation & spend: add/remove endpoints, set the daily budget, today's use, recent escalations, registry status | `src/surfaces/EscalationSettings.tsx` | implemented, render-tested; not clicked through in the running app |
| Registry route (ModelRegistry CID via InferenceRouter + x402) | core `registry_status`, `escalation_registry_status`; runtime `DisabledRegistry`, `X402PaymentRequest` | **interface only, shipped disabled** with an honest status (below) |
| TLA+ `SpendBudget.tla`: SpendWithinCap, NoEscalationWithoutShownPrice, EgressOptInOnly, OverBudgetOrTaintedNeedsHic1, ReservedIsConsistent, ResetOnlyAtPeriodBoundary, PeriodMonotone | `src-tauri/formal/SpendBudget*.{tla,cfg,py}` | TLC green on two configs; 11 of 11 mutants killed |

## Why the registry route is off

All four have to be true before it can run, and none is today:

1. `InferenceRouter` is not in the 40204 address book (`addresses::inference_router()` is `None`;
   the post-reroll redeploy, federation F-4).
2. The x402 asset allowlist is empty: SALT is native, and B-2 needs a token with
   `TransferWithAuthorization`, such as a wrapped SALT (ADR owner decision O-1).
3. Core's lean crypto build has no EIP-712 hasher (ADR D3 precondition; `approve` still refuses
   `TypedData`).
4. Registry model routing waits on the model precompile integration (federation F-1).

The deployed `InferenceRouter.requestInference(bytes32,bytes,uint256)` is payable in native SALT,
which is a plain transaction (HIC-1), not a budgetable x402 authorization. When F-4 and O-1 land,
the shape the ADR fixes (`{quote_id, recipient, asset, amount, resource}`, no raw typed data) is
already the runtime's `X402PaymentRequest`.

## Owner decisions (safe defaults, pending owner sign-off)

- Default daily cap **$0.00**: every escalation asks until the member sets a budget.
- Highest cap a member can set: **$100/day**. Highest accepted price: **$1,000 per 1M tokens**.
- Confirmed (HIC-1) escalations do not count against the cap; they are shown separately.
- The ledger is plain JSON in the app data directory, without a MAC. An unreadable file fails
  closed to asking; setting the budget again starts a fresh ledger.

## Honest limits

- Prices are typed by the member and cannot be verified; the provider's bill is authoritative.
- The input-token bound is UTF-8 bytes plus 16 per message, so quotes are ceilings (about 3-4x
  for English). Core and the sidecar share a golden value so the sidecar never refuses core's
  reservation.
- `escalate_plan` only reaches the model in the sidecar-loop preview; the gateway and local
  loops keep the unchanged `AGENT_TOOLS`.
- No escalation records go to agent-metering yet (the runtime metering crate is not wired into
  sessions); history lives in core's ledger and Settings.

## Proof

- Core cargo: `escalation::tests` 39, `hermes::session_tests` +2; tripwire, ACL (`popout_tests`),
  invoke secret scan and address-book tests green.
- Vitest: +35 (tool 15, bridge contract 7, store `handleTool` 4, Settings 9); TS guard mutants
  (budget check, shown price, HIC path) each fail the suite.
- TLC: see `src-tauri/formal/README.md` (SpendBudget section).
