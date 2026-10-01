---
created: 2026-09-30T00:00:00Z
branch: hup/s2-rule3-adr
author: Larry Klosowski + Claude Opus 5.5
status: proposed (requires @rule8 security sign-off before ANY budgeted signature is wired)
planset: 2026-09-30-hermes-upskill
wp: HUP-S2.0
decisions: D-6 (amended), D-12 (amended), D-23 (amended); red-team corrections 1, 2, 3, 6
relates_to: ADR-2026-08-29-session-ceremony-budget.md (narrows it), ADR-2026-09-30-hermes-loop-in-sidecar.md (on hup/s1-one-agent)
blocks: HUP-S1.5 (x402), HUP-S2.3 (SIWE budgets), HUP-S7.3 (nightly anchor)
---

# ADR-2026-09-30: Rule-3 amendment, the closed list of budgetable signatures

## Status

**Proposed.** Nothing in this ADR is implemented. The only budget code in the tree today is
the dormant `SessionBudget` primitive in `kit/src/ceremony.rs`, which no production signer
path calls. The CORE-G2 tripwire
(`kit/src/ceremony_tests.rs::core_g2_session_budget_is_not_wired_into_the_production_signer`)
enforces that. This ADR becomes binding only once the sign-off block at the end is
complete. Until then, every signature stays HIC-1, exactly as it is now.

## Context

### Rule 3 as it stands

CLAUDE.md Rule 3 says that every signature goes through the SignatureCeremony
(`kit/src/ceremony.rs`). The gated signers (`wallet::sign_personal`,
`wallet::sign_transaction`) are reachable only from `SignatureCeremony::approve` and
`SignatureCeremony::approve_and_broadcast`. Approval is bound to an explicit CeremonyId,
with no auto-approve and no "approve latest". One approval yields exactly one signature,
because the ceremony is removed from the pending map before signing. Undecodable payloads
need an explicit raw-mode ack, a locked vault fails closed, and no sidecar, daemon or
remote service ever holds a user key or signs.

Some ceremony behaviour this ADR depends on:

- `request(intent)` decodes the intent and stores it PENDING. It never signs.
- `approve` signs `PersonalSign` only (EIP-191 via `wallet::sign_personal`). It refuses
  `TypedData` and `Transaction` with `UnsupportedSigningKind` (PBA-L4-007), because the
  lean `crypto` build has no EIP-712 domain-separated hasher yet.
- `approve_and_broadcast` is the only path that produces an EIP-155 transaction.
- `SessionBudget { origins, kinds, chain_id, max_ops, expires_at_ms }` exists and is
  unit-tested, but it is dormant (ADR-2026-08-29, status proposed).

### What the Hermes upskill needs

The 2026-09-30 Hermes upskill planset needs three kinds of signature that a person should
not have to click through every time:

1. **Sign-In with Ethereum on allowlisted sites** (D-12, US-2.3). A developer has Hermes
   log into a dApp they already trust, without one approval card per login.
2. **x402 payments for registry escalation** (D-6, US-1.x AC3). Hermes escalates a hard
   turn to a ModelRegistry CID through the InferenceRouter and pays a small amount per
   request.
3. **The nightly anchor** (D-23, S7.3). One batched root of HIC decisions, the memory
   root and benchmarks goes to `AnchorRegistry` every night, while the member is asleep.

The second-model red-team pass (planset 00_OVERVIEW, "Red-team corrections" 1–3 and 6)
concluded that a general "budget over any intent" is too broad, and that the budgetable
set has to be an explicit, closed list that a security reviewer signs. Its findings are
kept in the private federation sprint. This ADR states the design response without
restating them.

### Architectural boundary (unchanged)

The design is "brain in the sidecar, hands in core" (ADR-2026-09-30-hermes-loop-in-sidecar).
The agent loop, the managed browser and the toolchain run in supervised sidecar processes
(`citrate-agent-runtime`). Keys, the ceremony, budgets and the signer stay in citrate-core.
This ADR does not move that boundary.

## Decision

### D1. The closed list

Exactly **two** signature kinds may be **budgetable**, meaning auto-approved inside a budget
the member granted through an HIC-1 ceremony. In the planset's tier vocabulary, a
signature auto-approved inside such a budget is HIC-2; granting the budget is HIC-1:

| # | Kind | Wire form | Signed by | Budget object |
|---|---|---|---|---|
| B-1 | **Hardened SIWE sign-in message** | EIP-4361 text, signed as EIP-191 `personal_sign` | member wallet key, via the ceremony | `WebSigningBudget` (per origin) |
| B-2 | **Capped x402 payment authorization** | EIP-712 typed data of one pinned primary type on one allowlisted asset domain | member wallet key, via the ceremony | `X402Budget` (per recipient) |

A third mechanism is **not** a budget on the wallet key:

| # | Kind | Signed by |
|---|---|---|
| A-1 | **Nightly anchor** to `AnchorRegistry` | a separate **anchor key** that holds no user funds and can sign nothing except anchor submissions (D5) |

**Everything else is HIC-1**, meaning explicit per-signature approval of a specific
CeremonyId in the native ceremony window. That covers every transaction (including
value-0 and "gasless" ones), `eth_sign`, every EIP-712 primary type other than the one
pinned in B-2 (EIP-2612 `Permit`, Permit2, order or listing signatures,
`setApprovalForAll`-style delegations), SIWE messages that fail any check in D2, x402
authorizations that fail any check in D3, key operations, deploys, and anything whose
decode is `Unrecognized`. There is no "raw-ack inside a budget": an intent that needs raw
ack is never budgetable.

Adding a kind to the closed list requires a new ADR with its own @rule8 sign-off. A config
flag, a settings toggle or a code constant cannot widen the list.

### D2. Hardened SIWE (B-1): the checks

A SIWE request is auto-approvable only if **every** check below passes, evaluated in core at
request time. If any check fails, the request **falls through to a normal HIC-1 ceremony**
(the person sees it and decides). It is not silently rejected, so a legitimate but
unusual sign-in still works with a click.

**Provenance (where the request came from)**

1. **Origin attestation by core.** The page origin is the **top-level frame's** origin,
   which core reads over its own DevTools-protocol session with the managed browser at
   request time. The origin is never taken from the page, the model, or the sidecar's
   request body. If core cannot attest the origin independently, the request goes to HIC-1.
2. **Top frame only.** A request raised from an iframe, a popup that is not the attested
   top frame, or a worker is never budgetable.
3. **Managed browser only.** No budgets apply in attach-to-Chrome mode. An attached
   browser's frames cannot be attested to the same standard.
4. **Origin allowlist.** The attested origin is an exact `https` scheme+host+port match for
   an origin the **member** added in the native Budgets window. The sidecar and the model
   can *suggest* an origin, but a suggestion is only a pending HIC-1 request to add it.
   Loopback, IP-literal, `http`, `file:` and custom-scheme origins cannot be allowlisted
   for budgets.
5. **Signing method.** Only `personal_sign` (EIP-191) over the exact message bytes is
   accepted. `eth_sign` and typed-data requests are never B-1.

**Message (strict EIP-4361)**

6. **Strict parse.** The message must parse against the EIP-4361 ABNF exactly. The parser
   rejects unknown fields, duplicate fields, out-of-order fields, non-`\n` line endings,
   leading or trailing bytes, and a total size over **2048 bytes**. Core then
   **re-serializes** the parsed message and requires byte equality with the bytes it will
   sign, so the parser and the signer agree on what was signed.
7. **Domain binding.** The `domain` (authority) equals the attested top-frame origin's
   host[:port] after IDNA/punycode normalization and lower-casing. If the message carries
   a scheme, it must be `https`.
8. **URI binding.** `URI` is an absolute `https` URI whose origin (scheme, host, port)
   equals the attested origin.
9. **Address.** `address` equals the member's active wallet address (EIP-55 checksum
   validated).
10. **Version.** `Version` is `1`.
11. **Chain.** `Chain ID` is in the chain allowlist, which is exactly `{40204}` in this ADR.
12. **Nonce.** The nonce is present and at least 16 alphanumeric characters. EIP-4361
    allows 8, and the stricter floor is deliberate. It must not appear in the
    **per-origin nonce ledger**. Core records it in that ledger before signing. Ledger
    entries are kept until the message's expiration time plus the clock-skew tolerance,
    then pruned.
13. **Issued At.** Present, and within `[now − 5 min, now + 60 s]`.
14. **Expiration Time.** **Required** for the budget path (a message without one goes to
    HIC-1). It must be later than now and no more than **24 h** after `Issued At`.
15. **Not Before.** If present, it is no later than `now + 60 s`.
16. **Statement.** Optional. If present it is at most **280 characters**, printable,
    contains no control or bidirectional-override code points, and is stored verbatim in
    the decision record.
17. **Resources.** Must be **absent**. Any `Resources` line, including EIP-5573
    capability (ReCap) URIs, sends the request to HIC-1. A budget may authenticate the
    member. It never delegates a capability.
18. **Request ID.** Optional, at most 128 characters, stored in the decision record.

**Context**

19. **Taint rule (red-team correction 3).** If the current agent task has untrusted
    content in context from **any source other than the same allowlisted origin** (other
    web origins, search results, third-party skill or MCP output, verified-source
    comments, memory from other principals), the request goes to HIC-1 for the rest of
    that task. *Owner/@rule8 decision O-3 below:* this ADR proposes that content from the
    same allowlisted origin does **not** taint a sign-in to that origin, because the
    checks above are deterministic and do not depend on what the model concluded. The
    reviewer may reject that exemption, in which case SIWE budgets only apply to
    member-initiated navigation. **Until O-3 is answered in the sign-off block, the strict
    rule of red-team correction 3 applies with no exemption**, because that correction
    supersedes the planset text and this ADR may not relax it on its own.
20. **Budget live.** The `WebSigningBudget` for this origin exists, is not revoked, is not
    expired, and has count remaining (D4).
21. **Rate.** At most one auto-approved SIWE per origin per 30 s and 20 per origin per
    rolling 24 h, further bounded by the budget's own `max_count`. Bursts go to HIC-1.

### D3. Capped x402 (B-2): the cap model

**Scope.** In this ADR, B-2 covers **registry escalation only** (D-6). The payee is the
InferenceRouter settlement path and the request is raised by the escalation router. An
HTTP 402 from an arbitrary website is HIC-1. Extending B-2 to web origins needs an ADR
amendment.

**Core builds the bytes it signs.** The sidecar sends a structured request
`{quote_id, recipient, asset, amount, resource}` and never raw typed data. Core builds the
EIP-712 payload itself from pinned templates, generates the 32-byte authorization nonce
from the OS CSPRNG, and sets the validity window. A request containing caller-supplied
typed-data bytes is never B-2.

**Pinned form.** One EIP-712 primary type is budgetable: the EIP-3009-style
`TransferWithAuthorization(from, to, value, validAfter, validBefore, nonce)` that the x402
`exact` EVM scheme uses. Its domain `{name, version, chainId, verifyingContract}` must
exactly match an **asset allowlist** entry. `ReceiveWithAuthorization`, `Permit`, Permit2
and every other type are HIC-1. *Owner decision O-1:* SALT is the native asset on 40204,
so the allowlisted asset has to be a token contract that implements this authorization
(for example a wrapped SALT). Its address must come from on-chain truth
(`addresses/40204.json` re-verified with `getCode` after any reroll). Until that contract
exists and is pinned, the allowlist is **empty** and B-2 is inert.

**Caps.** Every one of these must hold, and core evaluates them under the ceremony lock:

| Cap | Meaning | Proposed default (O-2) |
|---|---|---|
| `per_signature_max` | `value` of this one authorization | 1 SALT-equivalent |
| `per_recipient_window_max` | sum of `value` authorized to this recipient in a **rolling 24 h** window | 10 SALT-equivalent |
| `global_window_max` | sum across all B-2 budgets in a rolling 24 h window | 20 SALT-equivalent |
| `max_count` | authorizations under this budget | 200 |
| `validity_max` | `validBefore − now` | 10 min |
| `expires_at` | budget lifetime | ≤ 7 days |

The window is rolling rather than a calendar day, so there is no double allowance around
midnight. Amounts are compared as `U256` base units of the allowlisted asset. Overflow
fails closed.

**Recipient pinning.** `to` must equal the recipient pinned in the budget at grant time.
For escalation, that is the InferenceRouter settlement address read from on-chain truth.
The budget-grant ceremony displays it, and the member approves that exact address. A quote
naming any other payee goes to HIC-1.

**Other bindings.** `from` equals the member's active wallet address. `chainId` equals
40204. `validAfter ≤ now`. The authorization nonce has never been used, which holds by
construction because core generates it, and core records it in the ledger anyway.

**Accounting.** Core **reserves** spend before signing: it debits the counters,
write-ahead, in the same critical section as the cap check. A reservation is never
refunded, even when settlement fails, because the signed authorization stays usable until
`validBefore`. Over-counting is the safe direction.

**Taint.** The taint rule (D2 #19) applies without exception. If any untrusted content is
in context, B-2 goes to HIC-1.

**Precondition.** B-2 requires a real EIP-712 hasher (domain separator plus struct hash)
in the lean `crypto` build, tested against published vectors. It also requires a
typed-data decoder that shows the actual amount, asset and payee on the approval card.
Today `approve` refuses `TypedData` outright (PBA-L4-007). That refusal stays in place for
every type except the pinned one, and it is lifted for the pinned type only by the S1.5 WP
under this ADR.

### D4. Budgets: storage, grant, revocation, after-the-fact visibility

**Shape.** Both budget kinds generalize the dormant `SessionBudget` but are separate
types, so neither can be widened into the other:

```text
WebSigningBudget { id, origin, chain_id: 40204, max_count, used_count,
                   expires_at, granted_decision_id, revoked_at }
X402Budget       { id, recipient, asset, per_signature_max, per_recipient_window_max,
                   max_count, used_count, expires_at, granted_decision_id, revoked_at }
GlobalSpendCap   { global_window_max }            // one per member
```

**Grant.** A budget is created only by an **HIC-1 ceremony** in the native ceremony window
(red-team correction 7: never inside webview content). The card shows the origin or
recipient, the caps and the expiry in plain language. Approving it writes a
`BudgetGranted` decision record. This ADR proposes that the grant itself is an
approval-card decision with **no wallet signature**, so that granting a budget does not
create another signed artifact that could be replayed. O-4 asks the reviewer to confirm
this or to require a member signature over the terms, as ADR-2026-08-29 had it. Budgets
are scoped to the **principal** that requested them (the in-app Hermes loop, or one
external node-MCP client). They are never inherited across principals (red-team
correction 6).

**Storage.** Budgets and their counters and ledgers live in core, in the app data
directory. The file is integrity-protected with a MAC under a device-sealed key, the same
keychain-sealing pattern as the comms key. A missing, unreadable or MAC-failing file
means **no budgets**: the system fails closed to HIC-1 and the failure is logged. Counters
and nonce ledgers persist across restarts, so a restart never resets a cap. The sidecar
has no read or write access to this file.

**Revocation.** Revocation is **immediate**. `budget_revoke(id)` and `budget_revoke_all()`
are async Tauri commands (via `crate::blocking::off_main`). They set `revoked_at` under the
**same budget lock** that every auto-approval takes (a dedicated lock held across the whole
check, reserve, record and sign sequence; not the pending-map lock, which `approve`
releases before signing), and they persist a tombstone before
returning. The point at which a revocation takes effect is acquisition of that lock. An
auto-approval that has not acquired the lock when the revoke commits cannot sign. All
budgets are also voided when:
- the member unlinks or replaces the active wallet (S1.11), or the wallet address changes;
- the chain allowlist entry changes (for example after a reroll, until re-verified);
- an allowlisted asset or a pinned recipient fails on-chain re-verification;
- a sidecar binary update changes the attested sidecar identity;
- the member uses "Stop all autonomy" (one button in the native window, which also calls
  `budget_revoke_all`).

**After-the-fact visibility.** Every auto-approved signature writes a **decision record**
*before* the signature is returned (write-ahead; if the record cannot be written, nothing
is signed):

```text
AutoSignRecord { record_id, budget_id, kind: Siwe|X402, principal, origin_or_recipient,
                 payload_digest (keccak256 of the signed bytes), decoded_summary,
                 statement / amount+asset, nonce, signer_address, signed_at,
                 remaining_after, prev_record_hash }
```

Records are hash-chained locally and shown in three places:
1. a non-modal "Signed for you" notification right after each auto-signature, with a
   "Revoke this budget" action;
2. Settings → Budgets: per-budget history, remaining caps, and a revoke control;
3. the Activity monitor (HIC-3 review), which shows the same records.

They are included in the nightly anchor batch (D5, `AnchorBatch`), so the member can later
prove which signatures were auto-approved and under which budget.

### D5. The anchor key (A-1)

- **Separate key.** It is generated independently of the wallet seed from the OS CSPRNG
  (not derived from the wallet mnemonic). It is sealed in the OS keychain by core, as the
  device-sealed comms key is. It never leaves core, and the sidecar never holds it.
- **No user funds.** The anchor key's address holds no member balance. Gas comes from one
  of two places, chosen in O-5: the existing EIP-2771 relayer (the anchor key signs a
  forwarder request), or a small gas float that the member tops up only through an HIC-1
  transfer, with a balance ceiling that core refuses to exceed.
- **Single-purpose signer.** The anchor signer encodes exactly one call:
  `AnchorRegistry.anchor(AnchorKind.NightlyMerkle, root)` on the pinned registry address
  and chain 40204. It
  refuses any other destination, selector, value above zero, SIWE, x402 or typed data.
  The restriction is in the signer's code, not in configuration.
- **Binding to the member.** One HIC-1 ceremony, signed by the wallet key, registers the
  anchor key as the member's anchor delegate. *Contract dependency:* the deployed
  `AnchorRegistry` (citrate-chain `contracts/src/cit_agent/AnchorRegistry.sol`) records
  `committer = msg.sender` and has no delegate registry and no EIP-2771 trusted-forwarder
  support. The delegate binding therefore needs new on-chain work (a delegate registry or
  an off-chain signed binding that verifiers check), and the relayer route proposed in O-5
  would record the forwarder, not the anchor key, as committer unless the contract is
  extended. S7.3 cannot start until this is decided. Rotating or revoking that delegate is also
  HIC-1. A compromised anchor key can at worst post wrong roots under the member's
  delegate, and those roots are detectable against the local hash chain. It cannot move
  funds or sign in anywhere.
- **Sidecar role.** The sidecar may call `anchor_propose` (node MCP). Core builds the
  batch from its own decision records and memory root. The sidecar does not supply the
  root.

### D6. What the sidecar may request, and what it never does

| The sidecar MAY | The sidecar NEVER |
|---|---|
| submit a SIWE request `{tab_id, message}` and receive either a signature or a pending CeremonyId | hold, derive, read or proxy any key (wallet, anchor, comms) |
| submit an x402 request `{quote_id, recipient, asset, amount, resource}` | supply the origin, a frame identity, raw typed-data bytes, or an x402 nonce |
| read budget status (exists, remaining, expiry) for its own principal | create, widen, extend or un-revoke a budget, or add an allowlist origin |
| ask the member to add an origin or grant a budget (raises an HIC-1 card) | approve or reject any ceremony, or name "the latest" one |
| call `anchor_propose` | call any signer directly, or choose the anchor root |
| see its own auto-sign records | read another principal's budgets or records, or the budget store file |

Requests from the sidecar to core use the existing authenticated loopback channel, and
core rate-limits ceremony requests per principal (red-team correction 6).

### D7. Where it hooks into the ceremony

All budget logic lives in `kit/src/ceremony.rs` next to the gated signers, so the property
"the signer is reachable only from the ceremony module" stays true.

- A new entry point, `SignatureCeremony::request_budgeted(req, ctx) -> BudgetOutcome`,
  where `BudgetOutcome = AutoSigned { sig, record_id } | Pending(CeremonyView)`. Under one
  lock acquisition it runs the D2 or D3 checks, the budget-live check and the
  reserve/ledger write, then writes the decision record, then signs through the same gated
  signer `approve` uses. If anything fails, it falls back to today's `request()` and
  returns a pending ceremony.
- `request()`, `approve()` and `approve_and_broadcast()` keep their current semantics
  exactly. `approve` gains no budget awareness.
- `IntentKind` gains no generic "budgeted" variant. The two budgetable forms are
  separate request types, so a `Transaction` or a generic `TypedData` intent cannot even
  be expressed as a budget request.
- The CORE-G2 tripwire is **updated in the same reviewed change** that wires
  `request_budgeted`. It continues to assert that `SessionBudget` (the generic primitive)
  is never consulted. A new tripwire asserts that `request_budgeted` accepts only the B-1
  and B-2 request types and that the generic `covers` path is not reachable from it.
- New Tauri commands (`budget_list`, `budget_grant_request`, `budget_revoke`,
  `budget_revoke_all`, `autosign_records`) are async via `off_main` and pass the
  main-thread tripwire. None of them returns key material (I-2).

### D8. Failure modes

| Failure | Behaviour |
|---|---|
| Vault locked | Fails closed: no signature. The request becomes a pending HIC-1 ceremony that also fails closed until unlocked. |
| Budget store missing, corrupt or MAC-invalid | No budgets. Everything is HIC-1, and an alarm appears in Settings → Budgets. |
| Origin cannot be attested (CDP session lost, frame detached) | HIC-1 |
| Any D2 or D3 check fails | HIC-1 (the person sees the request; nothing is silently dropped) |
| Cap, count or rate exhausted | HIC-1, with "budget exhausted" shown on the card |
| Wall clock jumps backwards by more than 5 min, or disagrees with the monotonic clock | Budgets suspended (HIC-1) until the clock is consistent |
| Decision-record write fails | No signature (write-ahead) |
| Crash after reservation, before signing | The reservation stays counted (over-count is safe). On recovery the record is marked `outcome_unknown`, not `not_signed`: a write-ahead record alone cannot tell this case from a crash just after signing, so the audit trail must not assert that no signature exists |
| Crash after signing, before returning | The record exists. The signature may be lost to the caller, which is acceptable. The cap stays charged. |
| Two concurrent requests against one budget | Serialized on the ceremony lock. Neither can exceed caps. |
| Revoke races an auto-sign | Linearized on the lock. After the revoke commits, no auto-sign for that budget. |
| Sidecar compromised | Bounded by allowlisted origins, pinned recipients and caps. It cannot add origins or budgets. Every action is recorded. "Stop all autonomy" ends it. |
| Anchor key compromised | Wrong roots only. No funds and no sign-in. The member rotates the delegate (HIC-1). |
| Taint present | B-2 always goes to HIC-1. B-1 goes to HIC-1 unless the O-3 exemption is accepted. |

### D9. Formal obligation: `WebSigningBudget.tla`

A TLA+ module in `src-tauri/formal/` (next to `ConsentGate.tla`, which it refines where
the gates overlap) must be TLC-green at small bounds before **any** B-1 or B-2 wiring
merges (gate0, red-team correction 14). It must be TLC-green at the WP's bounds before
HUP-S2.3 closes. The x402 spend accounting is shared with `SpendBudget.tla` (S1.5). This
module carries the signing-side invariants for both budget kinds.

*Model.* Variables: `budgets` (kind, scope, caps, used, expiresAt, revoked), `ledger`
(origin × nonce seen), `spent` (per recipient and global, over the rolling window),
`signed` (sequence of auto-sign events), `records`, `pending` (HIC-1 ceremonies), `clock`,
`taint`. Actions: `Grant`, `RequestSiwe(origin, frame, msg)`, `RequestX402(recipient,
asset, value)`, `AutoSign`, `FallToHIC1`, `Revoke(b)`, `RevokeAll`, `Tick`, `Taint`,
`Crash/Recover`.

*Invariants and properties (each one must be checked):*

| Name | Statement |
|---|---|
| `OnlyClosedList` | every auto-sign event has kind ∈ {Siwe, X402}. No transaction, permit or other typed data is ever auto-signed. Called `OnlySiwe` in 03_TLA_SPECS for the web budget class. |
| `OriginBound` | every Siwe auto-sign has `domain` = `uri.origin` = the attested top-frame origin ∈ allowlist |
| `TopFrameOnly` | no auto-sign originates from a non-top frame or in attach mode |
| `NonceUnique` | no (origin, nonce) pair appears twice in `signed` |
| `NoCapabilityDelegation` | no Siwe auto-sign carries a resource |
| `NeverExceedsCaps` | for every budget, `used ≤ max_count`. For every x402 event, `value ≤ per_signature_max`. Across every rolling window, the per-recipient sum ≤ `per_recipient_window_max` and the global sum ≤ `global_window_max`. |
| `BudgetMonotone` | `used` and `spent` never decrease, including across Crash/Recover. Caps never increase without a new `Grant`. |
| `RevokeImmediate` | once `Revoke(b)` (or `RevokeAll`) has occurred, no later `AutoSign` uses `b` (a safety property over the trace, checked as an invariant with a history variable) |
| `ExpiredInert` | no auto-sign under a budget with `clock ≥ expiresAt` |
| `TaintDowngrade` | `taint` (outside the O-3 exemption) ⇒ no auto-sign for the rest of the task |
| `RecordBeforeSignature` | every element of `signed` has a matching element of `records` written no later than it |
| `FallThroughLive` | a request that fails any check eventually becomes a pending HIC-1 ceremony; it is never dropped |

Small-bound config: 2 origins, 2 recipients, 1 asset, `max_count = 2`, window = 3 ticks,
2 nonces, 1 crash. TLC results are cited in `gates.yaml` per the planset.

## Alternatives considered and rejected

1. **No relaxation.** Every SIWE and every x402 payment stays HIC-1. This is the safest
   option and it remains the default: with no budget granted, behaviour is unchanged. It
   is rejected *as the only mode* because US-2.3 and the D-6 escalation flow would need a
   card on every login and every paid turn, which trains people to click through cards.
   That is a worse outcome for safety.
2. **Wire the generic `SessionBudget` over `IntentKind`.** Rejected. A budget over
   `TypedData` covers permits, Permit2 and marketplace orders, which are open-ended
   value-moving signatures, and a budget over `Transaction` relaxes the property Rule 3
   exists to protect. The closed list has to be enforced by types.
3. **A hot session key in the sidecar.** Rejected. It violates Rule 3 directly ("no
   sidecar ever holds a key"), and a sidecar compromise would become a key compromise.
4. **Smart-account session keys (ERC-4337 or ERC-7715-style permissions).** Deferred, not
   rejected forever. 40204 has no native paymaster or bundler path today (the EIP-2771
   relayer is the gasless route), and this would change the member account model. It is
   worth revisiting when on-chain permission scoping can replace off-chain caps.
5. **Use the wallet key under a budget for the nightly anchor.** Rejected. An unattended
   nightly signer must not be able to move member funds. A single-purpose, no-funds key
   bounds the damage.
6. **Accept EIP-5573 ReCap resources in budgeted SIWE.** Rejected. That turns sign-in into
   capability delegation, which needs its own review.
7. **Take the origin from the page, the model or the sidecar.** Rejected. All three are on
   the untrusted side of the boundary. Only a core-attested top-frame origin binds.
8. **Calendar-day spend windows.** Rejected in favour of rolling 24 h windows (no
   double allowance around midnight).
9. **Budgets for web x402 (HTTP 402 from arbitrary sites) in v1.** Deferred. Escalation
   has a pinned, on-chain-verified payee. Arbitrary sites do not, until a recipient-pinning
   UX exists.

## Consequences

- Rule 3 changes from "every signature needs a per-signature human approval" to "every
  signature needs a per-signature human approval, **except** the two kinds in D1 inside a
  member-granted, capped, revocable, recorded budget". The custody boundary is unchanged:
  one gated signer, reachable only from the ceremony module, and no key outside core.
- CLAUDE.md Rule 3 text gets a one-line pointer to this ADR **only after** acceptance.
  That edit belongs to the WP that wires the first budget, not to this ADR.
- ADR-2026-08-29 (session ceremony budget) is **narrowed**: the generic primitive stays
  dormant, and any future use of it needs its own ADR naming its kinds.
- New work created: an EIP-712 hasher and pinned typed-data decoder (S1.5), the budget
  store and commands (S2.3), the anchor key and single-purpose signer (S7.3), the
  `WebSigningBudget.tla` spec, and tripwire updates.
- Owner decisions O-1 to O-5 below must be answered before the dependent WP starts.

## Owner / @rule8 decisions requested

| Id | Question | Proposed answer |
|---|---|---|
| O-1 | Which asset is on the x402 allowlist, given that SALT is native? | A wrapped-SALT token that implements the pinned authorization type, address pinned from on-chain truth. Until it exists, B-2 is inert. |
| O-2 | Default cap values | The table in D3, plus a SIWE default of `max_count = 50` and `expires_at ≤ 30 days`. Final values come from the "default budget values" WP. |
| O-3 | Does content from the same allowlisted origin taint a SIWE sign-in to it? | No (exemption). The reviewer may strike it. |
| O-4 | Is the budget grant an approval-card decision (no signature) or a wallet signature over the terms? | Approval-card decision plus a decision record |
| O-5 | How is anchor gas paid? | The EIP-2771 relayer, which requires an `AnchorRegistry` change (see D5). Fallback, which works with the contract as deployed: a capped gas float topped up via HIC-1. |

## Sign-off

This ADR is **not accepted** until every row below is signed. Signatures are added by the
named people, not by an agent.

| Role | Name | Decision (accept / accept-with-changes / reject) | Date | Signature / commit |
|---|---|---|---|---|
| @rule8 security reviewer (T1 signing path) | | | | |
| Federation lead / owner | | | | |
| citrate-core maintainer (ceremony + custody) | | | | |
| citrate-agent-runtime maintainer (sidecar boundary) | | | | |
| Formal-methods reviewer (`WebSigningBudget.tla`) | | | | |
