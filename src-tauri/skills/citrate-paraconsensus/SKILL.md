---
name: citrate-paraconsensus
description: Explains Citrate paraconsensus (Belnap four-valued, paraconsistent aggregation) with file citations. Use when asked what True, False, Both or Neither mean, how the knowledge and truth orders work, how learning contributions are classified and reduced, how the learning_root stays independent of the state_root, or how the memory CRDT and the ContradictionLedger use the same logic.
license: Apache-2.0
metadata:
  created: 2026-10-01
  branch: hup/n3-faucet-adr-literacy
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  wp: HUP-S7.7
  citrate-chain: 0aab474b437389ccd54c41c26378f1b3197ec4e5
  citrate-docs: 73ea7c56bc2087058ecc58eae5dbf734c86da072
  citrate-memories: 65742bcf138e26e0f42e9aebc3d73de9141ca6fe
---

# Citrate paraconsensus

"Paraconsensus" is the Citrate name for paraconsistent aggregation: when sources disagree, the
network records the disagreement as information instead of averaging it away. It rests on Nuel
Belnap's four-valued logic and appears in four places: the off-chain learning engine, the
on-chain `0x0110` precompile, the memory CRDT, and the ContradictionLedger contract. Each place
uses the same four values but not always the same classification rule. Say which one you mean.

Citations use `repo:path` or `repo:path#symbol`, pinned to the commits in this skill's
frontmatter. The plain-language companion page is
`citrate-docs:content/research/paraconsistent.md`.

## The four values

| Value | Reading | What it means for the network |
|---|---|---|
| True | known true | the trusted sources agree |
| False | known false | a source holds a position against the trusted majority |
| Both | true and false at once | sources of comparable trust genuinely disagree |
| Neither | no information | no source spoke with enough confidence |

In code: `citrate-chain:core/learning/src/belnap.rs#pub enum BelnapValue`. A mean collapses Both
and Neither into a number; the four-valued reduction keeps them. A `Both` says "contested", a
`Neither` says "unknown". Neither is the default value (`#[default]`).

## The two orders (the bilattice)

The four values form a bilattice with two partial orders
(`citrate-chain:core/learning/ARCHITECTURE.md#Definition 1: Belnap FOUR Bilattice`):

- **Knowledge order** (how much is known): Neither is below True and False, which are below
  Both. True and False are not comparable. `k_leq` implements it; `k_level` gives N=0, T=1,
  F=1, B=2 (`citrate-chain:core/learning/src/belnap.rs#pub fn k_leq`).
- **Truth order** (how true): False is below Neither and Both, which are below True. Neither and
  Both are not comparable (`citrate-chain:core/learning/src/belnap.rs#pub fn t_leq`).

Operations on the knowledge order:

- `join` (least upper bound, "combine evidence"): Neither is the identity, Both absorbs
  everything, and True joined with False is Both.
- `meet` (greatest lower bound, "keep only what both agree on"): Both is the identity, and True
  met with False is Neither.
- `negation` swaps True and False and leaves Both and Neither alone; applying it twice is the
  identity.

Property tests check commutativity, associativity, idempotence, absorption and double negation
(`citrate-chain:core/learning/tests/belnap_adversarial.rs`), and the TLA+ module
`citrate-chain:specs/tla/learning/BelnapLattice.tla` models the lattice.

## Off-chain aggregation (the learning engine)

`ParaconsistentAggregator::aggregate_paraconsistent`
(`citrate-chain:core/learning/src/aggregation.rs#aggregate_paraconsistent`) produces **two
outputs per dimension, computed independently**: a numeric aggregated embedding and a Belnap
state vector. The router reads both, so a dimension can be numerically near zero and still be
labelled Both.

1. **Trust weights from consensus.** Each source's GhostDAG blue score becomes a softmax weight,
   `softmax(blue_score / temperature)`
   (`citrate-chain:core/learning/src/belnap.rs#pub fn softmax_weights`). A node that cannot keep
   up with consensus carries little weight in learning.
2. **Classification** (`citrate-chain:core/learning/src/belnap.rs#pub fn classify_belnap`),
   per source and dimension, relative to the trust-weighted majority value:
   - confidence below the high threshold: Neither;
   - on the heavier side of the majority, or with no opposition: True;
   - the two sides weigh about the same: Both;
   - on the lighter side with a comparably trusted ally (weight at least half its own): Both;
   - alone on the lighter side: False.
   In this build only the high threshold is read; the low-threshold argument is unused
   (`_theta_low`).
3. **Reduction** across sources by `join`
   (`citrate-chain:core/learning/src/belnap.rs#pub fn reduce_belnap_states`): any Both, or a
   True next to a False, gives Both; all True gives True; Neither is absorbed by anything.

Defaults (`citrate-chain:core/learning/src/config.rs#min_participants: 3`): embedding dimension
768, at least 3 participants, thresholds 0.8 (high) and 0.3 (low), softmax temperature 1.0.
The config comments say the thresholds still need testnet calibration.

## On-chain aggregation (precompile `0x0110`)

`0x0110 BELNAP_AGGREGATE` is an integer, Q16 fixed-point version that every node computes
bit-identically (`citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub struct StandardBelnap`).
It is **not** the same rule as the off-chain engine, so do not assume byte equality:

- Classification is by **sign**, not by distance from a majority: with confidence at or above
  `threshold_pos`, an embedding of zero or more is "agree", a negative one is "oppose".
- Participants with weight zero or below are ignored for classification.
- The reduced state per dimension is Neither (no confident participant), True (only one side
  present) or Both (both sides present). **False never appears in the output**, and an
  all-negative dimension reduces to True: "True" here means "the confident sources are on one
  side", and the sign lives in the aggregated value.
- The aggregated value is a weighted **sum**, `sum(weight * embedding)`, saturating, with no
  division. Pass weights that sum to 1.0 if you want a mean.
- `threshold_neg` is parsed but not used by the current algorithm.

The `citrate-belnap-aggregate` skill has the wire format and a worked example checked against
chain 40204.

## Learning rides alongside consensus, never inside it

At each checkpoint the learning engine computes
`learning_root = SHA3-256(aggregated_embedding || state_vector || checkpoint_height)`
(`citrate-chain:core/learning/src/orchestration.rs#compute_learning_root`). The block header
carries `learning_root` as its own field, and `Block::compute_hash` deliberately excludes it
(`citrate-chain:core/consensus/src/types.rs#StateRootIndependent`). This is invariant INV-4,
StateRootIndependent, checked by `citrate-chain:specs/tla/learning/StrobilationCheckpoint.tla`.
Two consequences to state plainly:

- Learning can never change the ledger state or block order. It inherits the chain's safety and
  liveness, and a node can run with learning disabled and produce the same `state_root`.
- Below quorum the orchestrator records a zero `learning_root` instead of aggregating thin data
  (INV-5).

The docs mark the on-chain wiring of learning rounds as specified, not yet a production feature
(`citrate-docs:content/research/learning.md`). Do not tell a member that live federated rounds are
running on 40204 unless a current source says so.

## The memory CRDT

The member's memory graph merges replicas with the same lattice
(`citrate-memories:crates/mem-core/src/belnap.rs#pub fn join_confidence`). Each edge carries a
per-dimension confidence vector of Belnap values; merging two replicas joins them dimension by
dimension, with missing positions treated as Neither. Because `join` is commutative, associative
and idempotent, replicas that exchange bundles in any order converge (a state-based CRDT). A
`Both` is a contradiction the agent must stop and resolve, not auto-pick
(`citrate-memories:crates/mem-core/src/belnap.rs#is_contradiction`). The crate keeps a local copy
of the chain's lattice so it compiles on its own.

## The ContradictionLedger

When two attested sources disagree, `ContradictionLedger`
(`citrate-chain:contracts/src/rbac/ContradictionLedger.sol#contract ContradictionLedger`) records
the Belnap "B" state explicitly instead of reconciling it silently. A report needs two distinct
sources and two different values; a report always starts Open; resolution needs a non-zero
decision id that points at the AgentDecisionRegistry. For Hermes this is the escalation path for
an unresolved `Both` (planset 02_ARCHITECTURE section 8). Any write to it is a transaction and
goes through the member's SignatureCeremony.

## How to answer well

- Name which layer you mean: off-chain engine, `0x0110`, memory CRDT, or ContradictionLedger.
- Treat Both as information. Never "resolve" a Both by averaging or by picking the majority on
  the member's behalf.
- When code and docs disagree, the code at the pinned commit wins; say that you are reading
  code, and cite it.
- If a question needs a live number (how many rounds ran, current thresholds on chain), say it
  is not in the sources you have.
