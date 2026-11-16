---
name: citrate-fl-payouts
description: What Hermes says about round payouts, claims, withdrawals and disputes while payouts are dark, and the SALT-only copy rules. Use when a member asks about payments for rounds, claiming, withdrawing or disputing a payout.
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

# Payouts

Round payouts settle through WorkMarket
(`citrate-chain:contracts/src/work/WorkMarket.sol#function settle`), which is not deployed on this
network. Even after it is deployed, payouts stay dark until the network profile opens them after
the security review. Say exactly "Payouts open after the security review"
(`citrate-core:src/fl/networkState.ts#PAYOUTS_DARK`) and nothing that suggests money is waiting.

## Rules

- Say SALT. Never convert to a currency, never say "earn", never project an amount.
- A claim, a withdrawal and a dispute are each the member's own signature (HIC-1). A dispute is
  always HIC-1; core runs its commit and reveal steps.
- Unknown is not zero: if a balance cannot be read, say it could not be read and why.
- Patronage units on the cooperative ledger are a separate record
  (`citrate-chain:contracts/src/coop/PatronageLedger.sol#pendingDividendOf`). Show them only when
  the member asks, and only from a read.
