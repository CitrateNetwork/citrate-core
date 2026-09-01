# Cluster-reward simulation harness (GROW-REWARDS)

Test-only analysis tooling for `docs/CLUSTER_REWARDS_SPEC.md`. It exists to satisfy the reward spec's
**hard gate #1**: red-team the mechanism *numerically* — run the sybil / whale / wash-trade / recruit
adversaries against the blend and prove the fair-game invariants hold — **before any SALT is paid**.

Not imported by the app (vitest runs it; vite never bundles it).

## Run

```
npx vitest run src/sim/rewards
```

## What it models

- `model.ts` — the pure reward math from the spec:
  `Score(c) = Tier × Members_eff^α × Stake^β × Work^γ`, pool distribution, member split
  (equal base + contribution), and `rewardPerLocked` (reward per 32k-seat bonded — the anti-farm metric).
- `scenarios.ts` — actor generators (honest cluster, funded sybil, free sybil, wash cluster, whale) and
  the **candidate parameters** (`CANDIDATE_PARAMS`): α=0.5, β=0.4, γ=1.0; assurance weights A0≈0.02 →
  A3=2.5; Seed/Circle/Guild/Flagship tier gates keyed to the 32k membership bond.
- `rewards.sim.test.ts` — the seven fair-game invariants + a value-conservation / non-negativity check
  (the whole pool distributes, each cluster reward fully splits, no negative payouts), asserted
  numerically. If a parameter change
  breaks fairness, this goes red.

## What it proves at the candidate parameters (all green)

1. **Effort beats size** — a 50-member high-work cluster out-earns a 500-member low-work one, per member.
2. **Funded sybil is unprofitable** — 2,000 staked-but-workless seats (64M SALT bonded!) earn <5% the
   reward-per-32k-locked of an honest worker. The 32k floor + dominant multiplicative work term means
   fake seats are pure cost.
3. **Free sybil ≈ 0** — 5,000 A0 seats collect <1% of the pool.
4. **Whale can't buy dominance** — an honest high-work member out-earns a low-work whale bonded 50×.
5. **Wash-trading ≈ 0** — a common-control cluster's external work nets to 0 → <1% of the pool.
6. **Newcomer-fair split** — equal work → equal reward this epoch; more verified work → more reward.
7. **Positive-sum sharing** — adding a *real* working member raises your pool share; padding with free
   (A0) seats does not — they're excluded from size, so padding is neutral, never a lever. Inviting
   real contributors is the only lever — the property that makes
   sharing healthy instead of tacky.

## Still to do before these numbers are final (spec hard-gates)

- **Parameter sweep**: this validates ONE candidate set. Add a sweep over α/β/γ + tier thresholds to
  find the robust region where all invariants hold with margin, then record the chosen values in the
  spec's ⚙ table.
- **Richer adversaries**: mixed strategies (sybil + some real work to clear a tier), collusion across
  clusters, referral-bounty farming (needs the referral term added to the model), and dynamic
  multi-epoch behavior (stake lock-up, churn).
- **Securities-counsel review** of the referral/reward mechanism (independent of this harness).
