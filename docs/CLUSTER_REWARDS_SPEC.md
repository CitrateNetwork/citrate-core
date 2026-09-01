---
created: 2026-09-01
branch: docs/cluster-rewards-spec
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (Stage-1 draft — T1 MONEY; NOT built; needs its own red-team + securities counsel)
planset: cluster-growth
code: GROW-REWARDS
repo: citrate-core (surfacing) + rewards accountant (DGX/DO) + on-chain settlement (citrate-chain)
companions:
  - docs/CLUSTER_GROWTH_PLANSET.md   # gates GROW-S5 on this spec + counsel review
gates: hard-blocks GROW-S5; every parameter marked ⚙ is owner + counsel + red-team's to set
---

# Cluster rewards — a blended, tiered, fair game

## The one sentence

**Reward flows to real, verified work and staked participation — never to headcount or recruitment —
and everyone climbs the same tiers by doing scarce, verifiable things, so growing your cluster and
growing a healthy network are the same action.**

This spec exists because the GROW red-team found the original size-weighted idea was scriptably
farmable (F1), Ponzi-adjacent (F2), and wash-tradeable (F7). Every mechanism below is built to make the
honest path and the profitable path identical.

## Design invariants (what "fair game" means — these are testable properties)

1. **Effort beats size.** Reward-per-member is dominated by *verified work*; a small high-contribution
   cluster can out-earn a large idle one. (Kills "just get big.")
2. **Diminishing returns on every farmable lever.** Size and stake enter through concave curves (√/log)
   — no single axis runs away; you must blend. (Kills single-axis farming.)
3. **Sybil- and whale-neutral.** A seat counts only if *staked + assurance-verified + contributing*;
   capital alone hits a concave wall. Neither a botnet nor a whale dominates.
4. **Newcomer-fair split.** Within a cluster: an equal participation base + contribution weight +
   bounded tenure. A new contributor earns fairly this epoch; founders don't extract everything.
5. **Positive-sum sharing.** The ONLY way inviting raises your reward is inviting people who stake,
   verify, and contribute. Inviting fakes = 0. Sharing == building a real network.
6. **Transparent & predictable.** A member sees their score breakdown and their cluster's tier + the
   next threshold. A game with visible rules, not a black box. (Dignity/trust; Rule 1 — honest numbers.)

## Tier everything

### Member assurance tiers (the sybil gate, as a ladder anyone can climb)
Reward *weight* scales with how much a member has proven they're a distinct, real participant. Sybils
stay at the bottom earning ~nothing; there is no PII floor to *participate*, only to earn more.

| Assurance | Proof | Reward weight ⚙ |
|---|---|---|
| A0 device | wallet-derived identity only (S5) | ~0 (participates, earns ~nothing — this is the sybil floor) |
| **A1 member** | A0 + **the 32k-SALT membership bond in a block-producing node** (LOCKED) — i.e. a real member; "buy the membership." **Gift-able** (a sponsor stakes an invitee's seat). | low-base |
| A2 attested | A1 + hardware attestation (TPM/Secure Enclave — one-per-device) | medium |
| A3 verified | A2 + proof-of-personhood **or** PoW-VRAM (real GPU, the ALF-grant model) **or** KYC | full |

**LOCKED (owner, 2026-09-01):** min stake = **32,000 SALT bonded in a block-producing node** — the
existing membership bond IS the A1 floor. This is a strong economic sybil gate: each counted seat costs
32k SALT locked (and slashable), so 2,000 fake seats = 64M SALT bonded — the simulation must confirm the
farm ROI is deeply negative. The 32k is a per-**seat/member** bond (each member runs a producing node),
not a per-cluster bond. A2/A3 proofs above are confirmed as the top-tier weight unlocks.

Note: A0 alone earns essentially nothing — that is the answer to F1 (a free mnemonic buys no reward).
KYC is required only to reach the *top* earning weight, so growth to size stays low-friction while the
*reward-bearing* subset is sybil-hard. Which proofs count for A2/A3, and min stake, are ⚙.

### Cluster tiers (unlocked by the BLEND, never by size alone)

| Tier | Gate (ALL required) ⚙ | Multiplier ⚙ | Perks |
|---|---|---|---|
| Seed | default | 1.0× | — |
| Circle | ≥ N₁ A1+ members **and** per-member work ≥ w₁ | ~1.3× | verified badge |
| Guild | ≥ N₂ members **and** per-member work ≥ w₂ **and** total stake ≥ s₂ | ~1.7× | featured slot, sub-groups |
| Flagship | ≥ N₃ (~2k) members **and** *sustained* per-member work ≥ w₃ **and** stake ≥ s₃ **and** liveness | ~2.2× | top pool share, campaign hosting |

The per-member-work gate is the point: a 2,000-seat "Flagship" MUST be 2,000 real contributors, so the
2k target becomes a target for *real* participation (answers F1/F8). You cannot reach a tier with dead
seats.

## Organizations = groups (LOCKED, owner 2026-09-01)

A business or organization is **just a cluster** in this ecosystem — same tiers, same blend, same
member assurance ladder, no privileged org path. An org's employees/nodes are its members (each a
32k-bonded seat); the org "buying memberships" for its team is exactly the gift-membership flow at
scale. This keeps the game fair (a company can't out-rank a guild of individuals except by staking +
doing more verified work) and avoids a separate, gameable org track.

## The blend — how a cluster's epoch score is computed

For an epoch, a cluster `c`'s share of the reward pool is proportional to its **Score(c)**:

```
Score(c) = Tier(c) × Members_eff(c)^α × Stake(c)^β × Work(c)^γ

  Members_eff(c) = Σ over members m in c of  assuranceWeight(m) · isActiveThisEpoch(m)
                   (a member counts only if staked + verified + produced verified work this epoch)
  Stake(c)       = total bonded SALT across c's seats           (concave via β)
  Work(c)        = Σ verified, EXTERNALLY-demanded contribution  (the dominant term)
  Tier(c)        = the cluster-tier multiplier above

  weights ⚙:  α ≈ 0.5 (concave — size has diminishing returns)
              β ≈ 0.4 (concave — stake is a floor, not plutocracy)
              γ ≈ 1.0 (≥ α+β — WORK is the dominant, product-like axis)
```

**Work(c) — what counts (verified, external, wash-proof):**
- **Storage**: proof-of-retrievability on data that *external* members actually requested (not self-pinned junk → answers the F7 "2000 unique 1KB blobs" farm).
- **Inference**: requests served + validated against held-out challenges, from demand *originating outside the cluster / outside common control* (nets out A-buys-from-B wash-trading, F7).
- **Training**: gradient/round contributions validated by the federated-learning verifier.
- **Liveness**: attested uptime (a small multiplier, capped — presence isn't the same as work).

Self-dealing exclusion: buyer↔seller pairs under common control (shared stake origin, shared attestation
device, or graph heuristics) are netted out of Work. Per-seat Work is capped ⚙ so wash volume saturates
cheaply.

## The referral spark (bounded so it can never be a pyramid)

- When an invitee reaches **A1 (staked) + first verified work**, the inviter receives a **flat, one-time
  activation bounty** ⚙, and the invitee receives a **welcome grant** ⚙. Both sides win once, framed as
  "you brought a real contributor in."
- **Gift memberships (LOCKED, owner 2026-09-01).** Alongside an invite, a sponsor may **gift the 32k
  membership bond** for the invitee — they stake the invitee's A1 seat. This is the generous form of the
  invite: "I'll cover your seat." It is NOT extra reward leverage for the sponsor (the gifted seat earns
  for the *invitee*, not the gifter), so it grows real staked participation without becoming a
  buy-your-own-army farm (the gifter still locks 32k per gifted seat — the same economic floor). Gifting
  is ceremony-signed and the gifted stake is bonded under the invitee's seat.
- **Single-level only** — no reward from invitee-of-invitee. **Non-compounding** — never a share of the
  invitee's ongoing earnings. **Capped per inviter per epoch** ⚙ (anti-farm). Attribution is private
  (D-7) and, where feasible, blinded so the relay can't reconstruct the referral graph (F4).
- This is the ONLY recruitment-shaped reward, and it pays on *verified activation*, not signup — so it
  rewards bringing in a real participant, never headcount. (Answers F2; counsel reviews before ship.)

## The member split (fair within a cluster)

A member m's share of cluster c's reward this epoch:

```
share(m) = base(c)                                   # equal participation floor — everyone active gets some
         + contribution(m) / Σ contribution           # weighted by YOUR verified work
         + tenure(m)   [bounded/capped ⚙]              # small loyalty factor, capped so founders can't extract
```

`base` is the newcomer-fairness floor (invariant 4): a member who staked, verified, and did real work
this epoch earns a fair share even if they joined yesterday. Tenure is capped so it rewards loyalty
without becoming founder-oligarchy.

## Anti-abuse → red-team mapping

| Red-team finding | How this spec answers it |
|---|---|
| F1 sybil size farm | A0 (free identity) earns ~0; a counted seat needs stake + assurance + verified work; Members_eff is concave; tiers gate on per-member work |
| F2 recruit-to-earn / Ponzi | referral = flat, one-time, single-level, non-compounding, capped, paid on *verified activation*; dominant axis is Work; counsel review is a hard gate |
| F7 wash-trade / junk uniqueness | Work counts only externally-demanded, verified contribution; common-control pairs netted out; per-seat caps; "uniqueness" = useful-and-requested, never novelty-of-bytes |
| whale capture | stake enters concave (β≈0.4) + tiers gate on work, not capital |
| F4 metadata | reward accountant is a SEPARATE trust domain from the rendezvous relay; usage via member-attested signed receipts, aggregated; attribution blinded where feasible |

## Settlement

- Epoch-based. Rewards computed by a **reward accountant** (separate trust domain from the relay, F4)
  from signed, member-attested receipts + on-chain stake reads + the assurance registry.
- Distribution to the cluster, split per the member formula, **settled through each member's
  SignatureCeremony** (nothing paid without a signature — Rule 3; consistent with the existing Community
  "settles through your ceremony" promise).
- On-chain vs off-chain-ledger-for-alpha is ⚙ (an off-chain accountant may soak faster; settle on-chain
  at mainnet).

## Open parameters — owner + securities counsel + red-team (all ⚙ above)

1. **Min stake per A1 seat**, and whether stake is per-member or per-cluster-bonded.
2. **Which proofs count for A2/A3** (hardware attestation availability on Mac; PoP provider; PoW-VRAM
   spec; whether KYC is required for top weight given the no-day-one-cash constraint).
3. **The weights α/β/γ and tier thresholds N/w/s** — set by simulation against sybil/whale/wash
   adversaries, not by feel.
4. **Referral bounty + welcome grant sizes and per-epoch caps.**
5. **Epoch length + pool size + on-chain vs off-chain settlement.**
6. **"Externally-demanded" boundary** — the precise common-control heuristic for netting wash-trades.

## Required before GROW-S5 build (hard gates)

- [ ] This spec **red-teamed as its own T1 money doc** (adversarial simulation of the blend against
      sybil, whale, wash-trade, and recruit-farm strategies — prove the invariants hold numerically).
- [ ] **Securities counsel** review of the reward + referral mechanism (Howey / pyramid exposure).
- [ ] The **identity ADR** (GROW F9) frozen — assurance tiers depend on one canonical identity model.
- [ ] The **reward accountant** trust-domain + attested-receipt design (F4) specced with DGX.
- [ ] Parameters chosen by **simulation**, recorded here, before any SALT is paid.
