---
name: citrate-sidecar-consensus
description: How agreement is reached beside the chain rather than inside it. Covers the off-chain learning daemon that runs Belnap aggregation next to consensus, why learning can never change the ledger, and how Hermes's own keyless sidecar decides what may happen (verifiers, risk tiers, approval quorums, HIC, the SignatureCeremony). Use when asked who decides, who signs, what the sidecar may do, or how learning results reach the chain.
license: Apache-2.0
metadata:
  created: 2026-10-01
  branch: hup/n3-faucet-adr-literacy
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  wp: HUP-S7.7
  scope-note: Covers two sidecars, the chain-side learning daemon and the Hermes agent sidecar
  citrate-chain: 0aab474b437389ccd54c41c26378f1b3197ec4e5
  citrate-docs: 73ea7c56bc2087058ecc58eae5dbf734c86da072
  citrate-agent-runtime: 3d75efb0240689f557f5c71ae60b91b2bf060258
  citrate-core: 525b9ca9338ab941004ed0afdc9af151da3eaf0a
---

# Consensus beside the chain: the learning daemon and the agent sidecar

Citrate's block consensus is GhostDAG with VRF proposer election
(`citrate-docs:content/chain/consensus.md`). Two other kinds of agreement happen next to it, each
in a separate process (a sidecar), and both are designed so they can never corrupt the ledger or
sign for a member:

1. the **learning daemon**, which aggregates federated-learning contributions with Belnap logic
   and commits the result; and
2. the **Hermes agent sidecar**, which runs the agent loop and decides which actions may proceed,
   but holds no key and signs nothing.

## 1. The learning daemon

`citrate-chain:core/learning-daemon/src/lib.rs#Off-chain learning daemon` orchestrates the
federated loop off chain:

1. watch finalized blocks and advance a high-water mark that only moves forward;
2. decode `LearningCycleManager` events;
3. at cycle close, aggregate the cycle's embeddings with the Belnap algorithm and commit the
   result on chain;
4. retrain the routing model off chain so precompile `0x0111` can use it;
5. call `LearningCycleManager.finalizeCycle` exactly once per cycle;
6. serve a read-only dashboard API.

The aggregation step runs **the same code as precompile `0x0110`** in process
(`citrate-chain:core/learning-daemon/src/aggregator.rs#belnap::aggregate`), so its bytes match a
chain-side call; the precompile stays on chain so contracts can check the daemon's claim
independently. It is idempotent: a cycle already Computed or Committed is skipped.

The daemon's safety rules are a TLA+ spec,
`citrate-chain:specs/tla/learning/LearningDaemon.tla#FinalizeAtMostOnce`, with invariants
BlockHWMMonotonic, BlockHWMBoundedByChain, AggregationIdempotent, FinalizeAtMostOnce,
FinalizeRequiresCommit, NoFinalizeOfPending, RestartSafety and NoCommitWithoutAggregate. In words:
never go backwards, never claim a block the chain has not produced, aggregate the same cycle to the
same bytes, finalize at most once and only after the commit, and survive a crash and restart.

The crate's own header records which parts were scaffolded when; the public docs mark on-chain
learning rounds as specified, not yet a production feature
(`citrate-docs:content/research/learning.md`). Do not claim live rounds on 40204 from these sources.

## 2. Learning is a passenger on consensus

The learning engine reads from consensus (blue scores, finalized embeddings) and writes nothing
back into execution or block order. Its result is a separate `learning_root`, excluded from the
block hash, so a learning bug or disagreement cannot change the `state_root`
(`citrate-chain:core/consensus/src/types.rs#StateRootIndependent`; invariant INV-4 in
`citrate-chain:specs/tla/learning/StrobilationCheckpoint.tla`). Trust in learning comes from
consensus (softmax over blue scores), never the other way around. The `citrate-paraconsensus`
skill explains the four values and the aggregation.

## 3. The Hermes agent sidecar decides, core signs

The design rule is "brain in the sidecar, hands in core"
(`citrate-core:docs/adr/ADR-2026-09-30-hermes-loop-in-sidecar.md`). The agent loop
(`citrate-agent-runtime:agent-loop/src/lib.rs#pub enum HostKind`) runs in the sidecar; keys, the
SignatureCeremony, budgets and the signer stay in citrate-core.

**Who decides that a task succeeded.** Not the model. `RunOutcome::Answered` only means the model
produced a final message; success is a verifier's call
(`citrate-agent-runtime:agent-loop/src/lib.rs#Success is a verifier's call`).

**What a tool may do.** Every tool carries an `Effect` (None, Write, Spend, Sign) and a `Trust`
(Trusted, Untrusted); an unannotated tool is treated as effectful and untrusted
(`citrate-agent-runtime:agent-loop/src/lib.rs#pub enum Effect`). A Sign effect always goes through
core's SignatureCeremony, never the sidecar. Once a session has read untrusted content, every
effectful call needs an explicit member decision for the rest of the session until the member
clears it (the taint downgrade, `citrate-agent-runtime:agent-loop/src/lib.rs#pub struct TaintState`).

**How many approvals an action needs.** The runtime's approval gate maps a risk tier to a quorum
(`citrate-agent-runtime:agent/core/src/hitl/quorum.rs#pub fn for_tier`):

| Tier | Requirement |
|---|---|
| low | auto-approve, logged |
| medium | one signer from the manifest's required roles |
| high | two distinct roles from the required set (two signers holding the same role count once) |
| critical | Security Officer, Compliance Officer and Reviewer all sign |

The Auditor role can never approve (`citrate-agent-runtime:agent/core/src/hitl/roles.rs#pub fn can_approve`).
A pending call that nobody answers times out after 5 minutes, and a low-tier fast-path grant lasts
30 minutes (`citrate-agent-runtime:agent/core/src/hitl/mod.rs#AUTO_GRANT_TTL`).

**What the Hermes sidecar does with chain effects.** It is keyless by construction: a skill's chain
effect becomes a pending approval that citrate-core presents through its SignatureCeremony, and the
sidecar never signs or broadcasts (`citrate-agent-runtime:agent-sidecar/src/lib.rs#Keyless by construction`).
The sidecar exposes no quorum-signature route, so an effect at tier medium or above is refused at
once rather than parked where nobody can approve it.

**Who signs.** Only core's SignatureCeremony: it decodes the intent, stores it pending, and signs
only when the member approves that exact ceremony id; one approval gives one signature
(`citrate-core:kit/src/ceremony.rs#pub struct SignatureCeremony`). The closed list of signature
kinds that a member may pre-approve inside a budget is set by
`citrate-core:docs/adr/ADR-2026-09-30-rule3-budgetable-signatures.md`; everything else is HIC-1,
one explicit approval per signature.

## How to answer well

- "Consensus" alone means block consensus (GhostDAG). Say "learning aggregation" or "approval
  quorum" when you mean one of the sidecars.
- The learning daemon is chain infrastructure, not the member's agent. The Hermes sidecar is the
  member's agent and never holds a key.
- Use HIC vocabulary: HIC-1 is an explicit per-action approval, HIC-2 is an action inside a budget
  the member granted through an HIC-1 ceremony.
- If a question needs live state (open approvals, the current cycle), use the matching read tool
  or say the sources here do not hold it.
