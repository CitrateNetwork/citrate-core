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

## Fan-out 6 follow-up (branch `hup/n6-escalation-rest`, 2026-10-04)

Closes the buildable registry-route gaps from the M1 verification. Runtime half: citrate-agent-runtime
branch `hup/n6-escalation-rest`.

| Item | Where | State |
|---|---|---|
| EIP-712 hasher (ADR D3 precondition): typeHash, hashStruct, domain separator, `\x19\x01` digest, the pinned EIP-3009 `TransferWithAuthorization`, an approval-card view, and low-S `(v, r, s)` | `kit/src/eip712.rs` | implemented; EIP-712 "Ether Mail" spec vectors, EIP-3009 type hash, a `cast`-computed digest, and the deployed WrappedSALT's `DOMAIN_SEPARATOR()` all match |
| Pinned x402 template: asset, approved payee, active wallet, validity at most 10 min, OS CSPRNG nonce (never the caller's) | `kit/src/web_budget.rs` `build_x402_authorization`, `fresh_x402_nonce` | implemented and tested; B-2 stays **inert** (allowlist empty, O-1). The ceremony's `TypedData` refusal is unchanged. |
| InferenceRouter route: `requestInference` calldata, `getRequest` / `providers(address)` / `getProviders` / `getUserRequests` / `refundOwed` decoders, a read client, quote from the live route, gas from `eth_estimateGas` + 25 %, the HIC-1 ceremony tx | `src-tauri/src/inference_router.rs` | implemented; commands `escalation_registry_quote/request/result/mine` refuse with no network call while 40204 has no router pin |
| The ceremony shows a registry request legibly and says the input is stored publicly on chain | `kit/src/txdecode.rs` (`requestInference`, `claimRefund`) | implemented, tested (malformed encodings fall back to the generic label) |
| Anvil dry run against citrate-chain source: deploy InferenceRouter + WrappedSALT on chain id 40204, register a provider, quote, estimate, send the exact ceremony tx JSON, complete as the provider, read answer/price/refund, claim the refund; WrappedSALT accepts a `TransferWithAuthorization` signed over the kit digest and refuses its replay | `scripts/anvil-registry-dryrun.sh`, ignored test `inference_router::tests::anvil_dry_run_inference_router_and_wsalt_authorization` | green locally (forge/anvil 1.5.1); never touches 40204 or a real key |
| Registry status reports the real payment model (`native-salt-hic1`, `x402Enabled: false`) and no longer lists the EIP-712 hasher as missing | `escalation.rs` `registry_status`; runtime `DisabledRegistry` | implemented |
| Escalation receipts in metering (AC3, member-endpoint route): one content-free receipt per escalation that may have cost money; `/metering/daily` sums them | runtime `agent-metering` `EscalationReceipt`/`EscalationLog`/`EscalationSummary`; sidecar `POST /escalations` | implemented, tested |

### What the chain actually offers (finding for the owner)

- The deployed `InferenceRouter` takes **native SALT** only. A registry escalation is therefore a
  plain transaction (HIC-1 every time), never a budgetable B-2 authorization. Budgeted x402 for
  registry escalation needs either an x402/wSALT entry point on the router (a chain change) or an
  ADR amendment.
- `X402Facilitator.settlePayment` consumes `TransferWithFeeAuthorization` (fee bound into the
  digest), not the ADR-pinned `TransferWithAuthorization`. Pinning the facilitator path would need
  an ADR D3 amendment to add that type.
- The router stores `inputData` in contract storage, so a registry prompt is public. The approval
  card says so, and the input is capped at 4 KiB.

### Placeholders (pending owner sign-off)

- Highest registry price ceiling per request: **10 SALT** (`MAX_REGISTRY_PRICE_WEI`).
- Registry input limit: **4096 bytes**.
- Registry escalations are not counted against the USD daily cap (different unit; each one is
  HIC-1 anyway).

### Still not done

- 40204 has no InferenceRouter pin (F-4); no providers are registered there.
- Registry receipts in metering: the registry route runs in core, not the sidecar, so its spend is
  the on-chain request record (`escalation_registry_mine`), not an agent-metering receipt.
- No settings surface for the registry route beyond its status; the commands are reachable through
  the bridge only.
- Ledger MAC (owner decision, unchanged).
