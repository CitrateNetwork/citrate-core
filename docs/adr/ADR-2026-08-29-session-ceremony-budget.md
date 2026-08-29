---
created: 2026-08-29T00:00:00Z
branch: feat/cx-s7.5-session-ceremony-budget
author: Saul + Claude Opus 4.8
status: proposed
---

# ADR-2026-08-29 — Session ceremony budget (CX-S7.5 / RT-5)

## Context

CX-S7 (gS-ia) makes citrate-core grandma-proof. RT-5 flagged that a *social* node breaks that: pins,
cluster joins, and (later) training settlement each fire a `SignatureCeremony` prompt, so a member
doing ordinary group things is buried in signing dialogs — "the opposite of grandma-proof."

The ceremony (`kit/src/ceremony.rs`) is deliberately **single-use**: `approve(id)` removes the
ceremony from the pending map, signs exactly once via the one gated signer (`wallet::sign_message`),
and consumes it. Rule 3 (CLAUDE.md): the key never signs without a per-request human approval, and
one approval yields exactly one signature. This is the on-chain-custody HITL invariant.

RT-5 needs *fewer prompts without weakening custody*.

## Decision

Introduce a **session ceremony budget**: a scoped, session-lived authorization the member approves
**once**, which then covers a bounded set of routine signatures so each does not demand a fresh
prompt.

- The budget is established by a **normal ceremony** — the member approves ONE `personal_sign` over
  the budget's terms (its scope + bounds). That single approval is the human-in-the-loop event.
- A later intent that the budget **covers** is signed through the **same one gated signer**
  (`wallet::sign_message`) *without* a fresh prompt; each covered signature **consumes** one op from
  the budget. An intent the budget does not cover falls back to the normal per-tx prompt.

### The bounds (fail-closed — every one must hold for `covers`)

`SessionBudget { origins, kinds, chain_id, max_ops, expires_at_ms, used_ops }`:

- **origins** — only intents from these origins (e.g. `local-user`, `surface:groups`) are covered; an
  agent/micro-app/unknown origin is never covered.
- **kinds** — only these `IntentKind`s (a budget for routine group ops need not cover `Transaction`
  value transfers unless the member scoped it so).
- **chain_id** — an intent on any other chain is never covered.
- **max_ops** — the budget authorizes at most this many signatures; each consumes one.
- **expires_at_ms** — a short session TTL; after it, the budget covers nothing.

`covers(intent, now)` is true only if: not expired ∧ ops remaining ∧ chain matches ∧ origin allowed ∧
kind allowed. Anything else → the normal prompt.

## The Rule-3 relaxation (explicit)

This **deliberately relaxes single-use**: one approval (the budget) authorizes up to `max_ops`
signatures. It is bounded and member-approved, and it does NOT relax the other Rule-3 invariants:

- The key still signs **only** through `SignatureCeremony::approve` / the one gated path — no sidecar,
  agent, or command signs directly.
- Undecodable / raw-ack intents are **never** budget-covered — they always prompt.
- The budget is scoped (origins + kinds + chain), bounded (max_ops), and ephemeral (expiry), so a
  leaked/misused budget can authorize only a small, pre-declared set of routine ops before it dies.
- The member can revoke the budget at any time (drop it → back to per-tx prompts).

This is the RT-5 trade: replace *many identical per-tx prompts* with *one scoped budget approval*.

## Status / rollout

- **This ADR + the `SessionBudget` primitive** (in `ceremony.rs`, with tests) are the vetted first
  step. The primitive is **default-off and NOT yet consulted by `request`/`approve`** — with no
  budget established, behavior is exactly today's (every intent prompts).
- **The live wiring** — `request` checks an established budget and, on `covers`, auto-approves
  (signs + `consume`s) instead of pending; a bridge command to establish/show/revoke the budget; the
  `slices/ceremony.ts` + UI surface — lands **only after this ADR is accepted and the change passes a
  @rule8 security sign-off** (T1 signing path). Building the guarded primitive first, and gating the
  live relaxation behind that review, is the same discipline used for the other T1 money/key surfaces
  this cycle (settlement, cluster admission).

## Consequences

- A social member approves a budget once ("allow up to N group operations for this session") and then
  pins/joins/co-pins without a prompt storm — grandma-proof (gS-ia / RT-5).
- The relaxation is bounded, scoped, ephemeral, revocable, and audit-visible; the custody boundary
  (one gated signer, no direct signing) is unchanged.
- Security review must sign off before the auto-approve path is wired to the live signer.
