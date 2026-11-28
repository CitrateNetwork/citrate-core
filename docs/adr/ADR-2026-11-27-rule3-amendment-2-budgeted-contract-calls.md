---
created: 2026-11-27T00:00:00Z
branch: feat/v60-l2-wp06a-b3-adr
author: Larry Klosowski + Claude Opus 5.5
status: proposed (owner decision D-4 of 2026-11-27 chose this scope; the text awaits owner review and the sign-off block; nothing is wired)
planset: 2026-11-07-v060
wp: L2-WP06a (AR-E2.3, D7)
amends: ADR-2026-09-30-rule3-budgetable-signatures.md D1 (accepted 2026-10-01; that file is not edited)
reads_with: ADR-2026-10-04-rule3-amendment-1-budget-ambiguities.md, ADR-2026-08-29-session-ceremony-budget.md
formal: src-tauri/formal/WebSigningBudget.tla (B-3 section), src-tauri/formal/WebSigningBudgetB3*.cfg, src-tauri/formal/README.md ("WebSigningBudget, B-3")
blocks: L2-WP06b (wiring, private, owner sign-off)
---

# ADR-2026-11-27: Rule-3 amendment 2, budgeted contract calls (B-3)

## Status

**Proposed, 2026-11-27.** The owner chose the scope on 2026-11-27 (decision D-4 in the 0.6.0
owner decision queue, `citrate-federation/.agentile/planset/2026-11-07-v060/10_OWNER_QUESTIONS.md`):
a chain HIC-2 budget covers zero-value calls to the member's own deployed contracts only, within
a budget and an expiry, and deploys and value always go through the SignatureCeremony. This ADR
writes that decision down as an amendment to the closed list, with the checks, the grant, the
records and the formal obligation. It is binding only when the sign-off block at the end is
complete. Owner decision D-1 makes the owner the human reviewer for this class of change.

Nothing is implemented. The wiring is L2-WP06b, which is private, starts only after this ADR is
accepted, and ships behind a flag that is compiled off by default. Until then every transaction
stays HIC-1 and the two Rule-3 tripwires
(`core_g2_session_budget_is_not_wired_into_the_production_signer`,
`rule3_tripwire_no_ceremony_bypass_except_budgeted_web_sign_in`) stay as they are.

The threat notes for this amendment are kept in the private federation security repository.
This ADR states the design response without restating them.

## Context

### Rule 3 after ADR-2026-09-30

ADR-2026-09-30 D1 allows exactly two budgetable kinds: B-1 hardened SIWE and B-2 capped x402.
Everything else is HIC-1, explicitly "every transaction (including value-0 and gasless ones)".
D1 also says that adding a kind needs a new ADR with its own sign-off, and that no flag, setting
or constant can widen the list. This is that ADR. It adds one kind and changes nothing else in
ADR-2026-09-30 or its amendment 1.

### What 0.6.0 needs

The 0.6.0 HIC model gives an agent session an HIC-2 envelope: a member-granted set of action
classes, scope, budget and expiry inside which the agent acts without a card per action
(L2-WP05). For chain work the common loop after a deploy is many small calls to the contract the
member just published: set a parameter, mint a test item, call a view-changing function while
iterating on a front end. Today each one is a SignatureCeremony card. The 0.6.0 lane file
proposed budgeted calls to the member's own contracts as the narrow fix (L2-WP06a, question Q-3)
and the owner accepted it (D-4).

### What exists in the tree

- `SessionBudget` (`kit/src/ceremony.rs`), the dormant generic primitive of ADR-2026-08-29. Its
  `covers` checks origin, kind, chain, ops left and expiry. It does not look at the transaction:
  a budget over `Transaction` would cover a creation, a value transfer or an approval. The model
  keeps that reading as mutant `Covers`, and TLC shows it auto-signing a creation.
- `request_siwe_budgeted` (`kit/src/ceremony.rs`) and `kit/src/web_budget.rs`: the B-1 shape this
  ADR copies (one lock, write-ahead reservation and record, re-read the clock, one gated signer,
  any failure is an ordinary pending ceremony).
- Durable deploy records (`src-tauri/src/deploy_records.rs`, CORE-A3): for each confirmed
  creation, the address, deployer, init code and its hash, the ABI from the forge artifact, and
  the READY gate record for exactly that init code.
- The deploy gate (`src-tauri/src/deploy_gate.rs`, `DeployGate.tla`): a 40204 deploy opens a
  ceremony only with a READY record for exactly the init code.

## Decision

### D1. The closed list, amended

ADR-2026-09-30 D1 gains one row:

| # | Kind | Wire form | Signed by | Budget object |
|---|---|---|---|---|
| B-3 | **Budgeted contract call** | EIP-155 transaction on chain 40204, `value = 0`, `to` = a contract in the budget's target set, calldata that decodes under that contract's ABI | member wallet key, via the ceremony | `CallBudget` (per agent session) |

**Still HIC-1, always**, and never coverable by a B-3 budget:

- every contract creation (every 40204 publish is its own per-deploy ceremony behind a READY
  gate record, unchanged);
- any transaction with non-zero value;
- approvals, allowances, permits and operator grants of any standard;
- asset transfers of any standard, ownership and role changes, upgrades, and governance calls
  (D2 #7 lists the selector classes);
- a transaction to any contract outside the budget's target set, on any chain other than 40204,
  or submitted as a generic `Transaction` intent rather than through the B-3 request type;
- calldata that does not decode under the target's ABI (there is no raw-ack inside a budget).

The closed list is now B-1, B-2 and B-3. Adding a fourth kind still needs a new ADR.

### D2. The B-3 checks

A B-3 request is covered only if every check holds at decision time, under the budget lock.
Any failure gives an ordinary pending ceremony carrying the same transaction (HIC-1). The
numbers match the comments in `WebSigningBudget.tla` (`CallChecks`).

| # | Check | Notes |
|---|---|---|
| 1 | **Origin.** The request came in on the authenticated channel of the agent session whose envelope holds the budget (`agent:<session>`), as core attests it. | The sidecar names the budget it claims; core compares it with the session it attests. Pages, external MCP clients, other sessions and the member's own UI are never covered. A budget is never inherited across sessions or principals. |
| 2 | **Chain 40204.** | Core sets the chain id itself. The request type has no chain field. |
| 3 | **Value 0.** | Core sets `value = 0` itself. The request type has no value field. |
| 4 | **No creation.** | The request type has a required `to`; a transaction with an empty `to` cannot be expressed. A decoded transaction without `to` is HIC-1. |
| 5 | **Own target, granted.** `to` is in the budget's target set. | The set is chosen at grant (D3) from the deploy records where the deployer is this wallet, the creation succeeded on 40204 and a READY gate record exists for exactly that init code. A contract deployed later is not covered until a new grant. Core also re-reads the runtime code hash at `to` and refuses a call when it differs from the hash recorded at grant. |
| 6 | **Decodable and granted.** The selector is a function of the target's ABI from its deploy record, the arguments decode under it, and the function is in the budget's function list for that target. | No raw-ack inside a budget. The grant card lists the target's state-changing functions, minus the never-covered classes of #7, and only the functions the member ticks are covered. A function of the member's own contract that moves funds the contract holds (a `withdraw(to)`, say) is covered only if the member ticked it. |
| 7 | **Never-covered selector classes.** | Approvals and allowances (`approve`, `increaseAllowance`, `decreaseAllowance`, `setApprovalForAll`, `permit`, operator functions), asset transfers (`transfer`, `transferFrom`, `safeTransferFrom` and batch forms), ownership and roles (`transferOwnership`, `renounceOwnership`, `acceptOwnership`, `grantRole`, `revokeRole`, `renounceRole`), upgrades (`upgradeTo`, `upgradeToAndCall`, `changeAdmin`), and governance (`propose`, `castVote*`, `queue`, `execute`, `delegate`, `delegateBySig`). The list is a constant in the signer's module and only grows; a function with one of these names or selectors is refused whatever its arguments. |
| 8 | **Budget live.** Granted, not revoked, not expired. | Expiry at most 24 h after the grant. |
| 9 | **max_ops.** Fewer than `max_ops` calls signed under this budget. | `max_ops` at most 20. |
| 10 | **Gas cap.** The worst-case cost of this call (`gas_limit * max_fee_per_gas`, both set by core from its own estimate and a per-call ceiling) plus what is already reserved under this budget stays within the budget's gas cap. | Reserved write-ahead, never refunded (over-count is the safe direction). Gas is paid from the member's balance. |
| 11 | **Taint, strict.** The session's context holds no untrusted content. | No exemption of the O-3 kind. A tainted session's calls are HIC-1 for the rest of the task. |
| 12 | **No standing approval to the target.** The wallet holds no live allowance or operator approval naming the target. | Read from 40204 (approval logs for this wallet, confirmed by the current `allowance` / `isApprovedForAll` value). With an approval in place, a call to the member's own contract could move the member's assets, which D1 excludes. |

Budget values (default `max_ops`, default gas cap, per-call gas ceiling) are not set here. They
come from the default budget values WP, as ADR-2026-09-30 O-2 did for B-1 and B-2.

**Voiding.** Besides `budget_revoke` and "Stop all", a B-3 budget is voided when: its session
ends; its envelope is revoked; the session's HIC level is raised to HIC-1 or HIC-0 (raising
control applies at once, 04_HIC_MODEL L-1); the wallet is unlinked or changes; the chain
allowlist entry changes (for example after a reroll); or a target's runtime code hash changes.

### D3. Grant

A B-3 budget is created by **one HIC-1 ceremony in the native window: the member signs one
`personal_sign` message over the budget's terms** (the ADR-2026-08-29 pattern). The card shows
the session, each target with its contract name and address and its ticked functions,
`max_ops`, the gas cap and the expiry in plain language. The terms message is domain-separated
(`Citrate B-3 call budget v1`), names the budget id, the wallet, chain 40204, the session, each
target with its runtime code hash and its function selectors, `max_ops`, the gas cap and the
expiry, and is accepted once.
It is not a valid EIP-4361 message and cannot be a transaction.

This differs on purpose from ADR-2026-09-30 O-4, where a B-1 or B-2 grant is an approval-card
decision with no signature. B-3 budgets spend gas from the member's balance and act on chain, so
the signed terms are kept with the budget as the member's own evidence of what was authorized,
and go into the nightly anchor batch with the records. Question O-1 below asks the owner to
confirm this or to use the card-only grant.

A grant never widens a live budget: a new target set, a higher cap or a later expiry needs a
new grant, which starts a new generation. Rolling counts and gas reservations are kept per
generation and are never reset by a regrant of the same generation.

### D4. Storage, revocation and records

The same rules as ADR-2026-09-30 D4 apply, with `CallBudget` as a third, separate budget type
so that none of the three can be widened into another:

```text
CallBudget { id, session, wallet, chain_id: 40204,
             targets: [(address, code_hash, selectors)],
             max_ops, used_ops, gas_cap, gas_reserved, expires_at,
             terms_signature, revoked_at }
```

- Storage in core only, integrity-protected like the B-1 and B-2 budgets. Missing, unreadable
  or failing the integrity check means no budgets (HIC-1). Counters and reservations persist
  across restarts.
- Revocation is immediate and takes the same budget lock as every auto-signature (amendment 1,
  A-5 and A-8 apply unchanged).
- Every auto-signed call writes a decision record **before** the signature (write-ahead; no
  record, no signature). The record carries the call itself: budget id, session, target,
  function name and decoded arguments, calldata digest, gas reserved, signer address, nonce, and
  after broadcast the transaction hash. Records are hash-chained with the B-1 and B-2 records,
  shown in Settings, Budgets and in the HIC decisions view, and included in the nightly anchor.
- The "Signed for you" notification for a call carries the summary and a deep link only. It
  never carries an approve or reject action (the F-13 contract with the notification layer).

### D5. Where it hooks into the ceremony

- A new entry point `SignatureCeremony::request_call_budgeted(vault, gate, req, clock) ->
  BudgetedOutcome` (L2-WP06b), mirroring `request_siwe_budgeted`: evaluate every D2 check and
  the budget under the budget lock, reserve write-ahead (op, gas, record), re-read the clock and
  the revocation state, sign through the one gated transaction signer, close the record. Any
  failure returns an ordinary pending ceremony with the same transaction.
- Core builds the transaction: chain id, nonce, `value = 0`, gas limit and fee fields. The
  sidecar supplies only `{session, target, calldata, gas hint}`.
- `request()`, `approve()` and `approve_and_broadcast()` keep their semantics. `IntentKind`
  gains no budgeted variant; a generic `Transaction` intent can never reach the B-3 path.
- The HIC engine is unchanged in kind: chain effects still return `Ask{SignatureCeremony}` at
  every level (engine invariant H-1). The budget decision is inside the ceremony, not in the
  engine and not in any notification path.
- The two Rule-3 tripwires are **replaced by stricter ones in the same reviewed change** that
  wires the path, never deleted: `session_budget_reaches_the_signer_only_through_request_call_budgeted`
  and `rule3_tripwire_no_ceremony_bypass_except_siwe_and_budgeted_calls`, plus the WP06b set
  (`creation_is_never_budget_covered`, `value_transfer_is_never_budget_covered`,
  `raw_ack_intent_is_never_budget_covered`, `foreign_target_falls_through_to_hic1`,
  `record_written_before_signature`, `revoke_wins_over_concurrent_sign`).
- The path ships behind `CITRATE_HIC2_CHAIN_CALLS`, compiled default **off**, turned on only
  after owner sign-off of WP06b.

### D6. What the sidecar may request, and what it never does

| The sidecar MAY | The sidecar NEVER |
|---|---|
| submit `{session, target, calldata, gas hint}` and receive a transaction hash or a pending CeremonyId | supply the chain id, the value, the nonce, the fee fields or raw transaction bytes |
| read its own session's budget status (exists, remaining ops and gas, expiry) | create, widen, extend or un-revoke a budget, or add a target |
| ask the member for a budget (raises the HIC-1 grant card in the native window) | approve or reject any ceremony, or name "the latest" one |
| see its own session's records | read another session's budget or records, or the budget store |

### D7. Failure modes

| Failure | Behaviour |
|---|---|
| Vault locked | No signature; the pending ceremony also fails closed until unlocked. |
| Budget store or deploy records missing, unreadable or failing integrity | No B-3 budgets, no own targets: HIC-1. |
| Any D2 check fails, cap or count exhausted | HIC-1 with the reason on the card. |
| Target code hash changed since the grant | The budget is voided; HIC-1. |
| Decision-record write fails | No signature (write-ahead). |
| Crash after reservation, before signing | Reservation stays counted; the record becomes `outcome_unknown` on recovery (amendment 1, A-3). |
| Crash after signing, before broadcast | Record exists; the signed transaction may be lost to the caller; op and gas stay charged. |
| Broadcast fails or the transaction is never mined | Record shows the failure; op and gas stay charged; a retry is a new request against the budget. |
| Revoke races an auto-sign | Linearized on the budget lock; after the revoke commits, no auto-sign for that budget. |
| Sidecar compromised | Bounded by the session, the granted targets, the selector exclusions, `max_ops`, the gas cap and the expiry; every call recorded; "Stop all" ends it. |

### D8. Formal obligation: `WebSigningBudget.tla`, B-3

`WebSigningBudget.tla` is extended in place, so the closed list lives in one module:
`OnlyClosedList` now ranges over `{Siwe, X402, BudgetedCall}`. The B-3 path is a separate
action (`DecideCall`, the model of `request_call_budgeted`) under the same lock as `Decide`,
signed by the same `Sign` and `Return`. The environment raises adversarial call requests:
creations, foreign targets, value, other chains, raw-ack calldata, approval-class selectors,
another session's budget, any gas.

| Invariant | Statement |
|---|---|
| `OnlyClosedList` | every auto-sign is SIWE, x402 or a B-3 call; a generic `Tx` or `Permit` never is |
| `NoCreationAutoSigned` | no auto-signed call is a contract creation |
| `NoValueAutoSigned` | no auto-signed call carries value |
| `OnlyOwnDeployedTargets` | an auto-signed call's target is in its budget's granted set, and that set holds only contracts this wallet deployed (as of the signature) |
| `ChainPinned` | 40204 only |
| `NoRawAckAutoSigned` | no auto-signed call needs a raw-mode ack |
| `NoDelegationAutoSigned` | no auto-signed call has a never-covered selector or a function outside the granted list (one abstract class in the model) |
| `SessionBound` | the attested session is the session whose budget paid |
| `GasCapped` | reserved worst-case gas per budget generation never exceeds the cap |
| `NeverExceedsCaps` | `max_ops` per budget generation (with the B-1 and B-2 caps as before) |
| `RecordBeforeSignature` | every auto-sign has its record written first; for a call the record carries the exact call |
| `TaintDowngrade`, `ExpiredInert`, `RevokeImmediate`, `NoFalseNegative`, `NoDrop`, `BudgetMonotone`, `FallThroughLive` | as in ADR-2026-09-30 D9, now over B-3 too (targets fixed within a generation, deploy records only grow) |

TLC must be green on the B-3 configs before any WP06b code merges, and every B-3 mutant must
be caught by the invariant it targets. The configs carry EXPECT lines and run in CI
(`.github/workflows/tla-models.yml`, `tlc_expect.py`). Results and abstractions are in
`src-tauri/formal/README.md`, section "WebSigningBudget, B-3".

## Alternatives considered

1. **Keep chain HIC-2 as prompts only for 0.6.0.** The safest option, and the behaviour until
   WP06b ships with its flag on. Not chosen as the target because the owner chose budgeted calls
   (D-4), and a card per routine call trains members to click through cards.
2. **Wire the generic `SessionBudget` over `Transaction`.** Rejected. `covers` does not look at
   the transaction. Mutant `Covers` in the model shows it auto-signing a creation.
3. **Any contract, not only the member's own.** Rejected. A third-party contract's behaviour is
   not bounded by anything the member reviewed; the member's own contract passed the full gate
   and a per-deploy ceremony.
4. **Allow value or creations inside the budget.** Rejected by D-4: deploys and value always go
   through the SignatureCeremony.
5. **Targets as "every contract I ever deployed", evaluated at call time.** Rejected in favour of
   a target set fixed at grant, so the member sees every target on the grant card and a later
   deploy is never covered silently.
6. **Smart-account session keys (ERC-4337, ERC-7715).** Deferred, as in ADR-2026-09-30: 40204 has
   no native paymaster or bundler path today.

## Consequences

- Rule 3 reads: every signature needs a per-signature human approval, except the three kinds of
  ADR-2026-09-30 D1 as amended here, inside a member-granted, capped, revocable, recorded budget.
  The custody boundary is unchanged: one gated signer reachable only from the ceremony module, no
  key outside core.
- B-3 bounds which contracts and which functions a budget reaches. It does not bound what the
  member's own contract does when called. That is the job of the deploy gate and the per-deploy
  ceremony, and it is why D2 #5 pins the code hash and D2 #12 refuses targets the wallet has
  approved.
- New work: L2-WP06b (private): `CallBudget`, `request_call_budgeted`, the grant card, the
  records, the stricter tripwires, the flag. CLAUDE.md Rule 3 gets a pointer to this ADR only in
  the WP that wires it.

## Owner decisions requested

| Id | Question | Proposed answer |
|---|---|---|
| O-1 | Is the B-3 grant a `personal_sign` over the terms (D3) or an approval-card decision with no signature, as for B-1 and B-2? | `personal_sign` over the terms, for the reasons in D3. |
| O-2 | May a proxy contract the member deployed be a target? | No: a target whose ABI has an upgrade function, or whose EIP-1967 implementation or beacon slot is set, is not offered at grant. |
| O-3 | Default `max_ops`, default gas cap, per-call gas ceiling | From the default budget values WP; the hard limits here are `max_ops <= 20` and expiry `<= 24 h`. |
| O-4 | D2 #12 reads approvals from chain on every call. Accept the latency, or read at grant and on every block that touches the wallet? | Read on every call; the cost is one log query and at most one view call per target. |
| O-5 | Function list at grant (D2 #6): pre-tick every state-changing function, or none? | None pre-ticked; the member ticks each function. |
| O-6 | Simulate each budgeted call (`eth_call` with a call trace on 40204) before signing, and refuse when the trace calls a contract outside the target set or moves value? | Yes. The private threat notes give the reason; core already runs a fork dry run for deploys. |

## Sign-off

This ADR is **not accepted** until every row below is signed. Signatures are added by the
named people, not by an agent.

| Role | Name | Decision (accept / accept-with-changes / reject) | Date | Signature / commit |
|---|---|---|---|---|
| Federation lead / owner (D-1 human reviewer) | | pending | | |
| @rule8 security reviewer (T1 signing path) | | pending | | |
| citrate-core maintainer (ceremony and custody) | | pending | | |
| citrate-agent-runtime maintainer (sidecar boundary) | | pending | | |
| Formal-methods reviewer (`WebSigningBudget.tla`, B-3) | | pending | | |
