---
name: citrate-fl-diagnose
description: How Hermes diagnoses federated round problems from real records: why a round or lease was rejected, a coordinator that does not answer, a reverted transaction and a send whose outcome is unknown. Use when a member asks why something in a round failed.
license: Apache-2.0
metadata:
  created: 2026-11-16
  branch: feat/054-hermes-hd-fl-skills
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  wp: Hermes x FL WP-H11
  reserved: citrate-fl names are reserved; skill_write and learned skills cannot shadow them
  citrate-core: f29eedcee183cc56cadbbca92e11518a09b8a000
  citrate-chain: b2e91167bda2b81d52658c58f6327063a25cbedc
---

# Diagnosing rounds

Work from records, never guesses. Name the source of every fact: the round ledger on chain, the
coordinator (say "coordinator-reported"), or this device.

## A rejected round

The round ledger records why a round was rejected
(`citrate-chain:contracts/src/FederatedRoundLedger.sol#RoundRejected`). The faults are:

- OutputMismatch: a replayed chunk did not produce the committed output;
- RowMismatch: a row did not match the committed data;
- RuleViolation: the round broke its cluster's rules;
- ParticipantOrder: the participants were not in the committed order.

Read them with `contract_view` or by explaining the transaction with `explain_tx`; decode events
with `decode_log`. Contract addresses come from `address_book_lookup`, never from memory.

## A coordinator that does not answer

`fl_round_plan` reports when the coordinator cannot be reached. Say so, and that no round can
start until it answers; do not retry in a loop.

## A transaction

- Reverted: report the revert reason the receipt shows. Nothing changed on chain.
- Sent, outcome unknown: the transaction left the device but no receipt was read yet. Do not say it
  succeeded or failed; read it again with `explain_tx` once it has a receipt.
