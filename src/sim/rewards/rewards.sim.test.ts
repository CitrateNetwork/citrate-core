// GROW-REWARDS — the fair-game invariants, asserted NUMERICALLY against the candidate parameters.
// This IS the red-team-by-simulation the reward spec hard-gates on: it runs the sybil/whale/wash/
// recruit adversaries against the blend and proves the seven fair-game invariants (plus value
// conservation + non-negativity) hold. If a param change
// breaks fairness, this test goes red. Run: `npx vitest run src/sim/rewards`.
import { describe, it, expect } from "vitest";
import {
  distributePool,
  memberSplit,
  rewardPerLocked,
  controllerReward,
  type Cluster,
} from "./model";
import {
  CANDIDATE_PARAMS as P,
  honestCluster,
  honestMember,
  fundedSybilCluster,
  freeSybilCluster,
  washCluster,
  whaleMember,
} from "./scenarios";

/** Per-member reward for a single-cluster metric: cluster reward / active member count. */
function perMemberReward(net: Cluster[], clusterId: string): number {
  const pool = distributePool(net, P);
  const c = net.find((x) => x.id === clusterId)!;
  const r = pool.get(clusterId) ?? 0;
  const active = c.members.filter((m) => m.active && m.assurance !== "A0").length;
  return active > 0 ? r / active : 0;
}

describe("fair-game invariant 1 — effort beats size", () => {
  it("a small high-work cluster out-earns a large low-work cluster, per member", () => {
    const net = [honestCluster("small", 50, 20), honestCluster("big", 500, 1)];
    expect(perMemberReward(net, "small")).toBeGreaterThan(perMemberReward(net, "big"));
  });
});

describe("fair-game invariant 2 — funded sybil is unprofitable (the F1 attack under the 32k floor)", () => {
  it("2,000 staked-but-workless seats earn far less reward-per-32k-locked than an honest worker", () => {
    const honest = honestCluster("honest", 100, 10);
    const honestCtrl = honest.members[0].controllerId;
    // Attacker bonds 2,000 × 32k = 64M SALT into fake seats that produce ~no external work.
    const sybil = fundedSybilCluster("sybil", "ATTACKER", 2000, 0.01);
    const net = [honest, sybil];
    const honestRPL = rewardPerLocked(net, honestCtrl, P);
    const sybilRPL = rewardPerLocked(net, "ATTACKER", P);
    // The attacker's return on locked capital is a tiny fraction of an honest worker's.
    expect(sybilRPL).toBeLessThan(honestRPL * 0.05);
  });
});

describe("fair-game invariant 3 — free sybil earns ~nothing (A0 is the floor)", () => {
  it("thousands of A0 seats collect a negligible share", () => {
    const honest = honestCluster("honest", 100, 10);
    const free = freeSybilCluster("free", "FREELOADER", 5000);
    const net = [honest, free];
    const freeReward = controllerReward(net, "FREELOADER", P);
    const honestTotal = P.pool - freeReward;
    expect(freeReward / P.pool).toBeLessThan(0.01); // <1% of the pool for 5,000 free seats
    expect(honestTotal / P.pool).toBeGreaterThan(0.99);
  });
});

describe("fair-game invariant 4 — whale can't buy dominance (work dominates stake)", () => {
  it("an honest high-work modest-stake member out-earns a low-work whale bonded 50×", () => {
    const honest = honestCluster("workers", 60, 15); // 60 real workers
    const whale: Cluster = { id: "whale", members: [whaleMember(50, 1)] }; // 50×32k, work 1
    const net = [honest, whale];
    const pool = distributePool(net, P);
    const honestSplit = memberSplit(honest, pool.get("workers")!, P);
    const oneHonest = honestSplit.get(honest.members[0].id)!;
    const whaleSplit = memberSplit(whale, pool.get("whale")!, P);
    const whaleReward = whaleSplit.get(whale.members[0].id)!;
    expect(oneHonest).toBeGreaterThan(whaleReward);
  });
});

describe("fair-game invariant 5 — wash-trading earns ~nothing (external-demand netting)", () => {
  it("a common-control wash cluster's external work nets to 0 → negligible reward", () => {
    const honest = honestCluster("honest", 100, 10);
    const wash = washCluster("wash", "WASHER", 200); // staked, but all intra-control → externalWork 0
    const net = [honest, wash];
    const washReward = controllerReward(net, "WASHER", P);
    expect(washReward / P.pool).toBeLessThan(0.01);
  });
});

describe("fair-game invariant 6 — newcomer-fair split (no founder oligarchy)", () => {
  it("a newcomer doing equal work earns equal to a founder this epoch", () => {
    // Same work → same reward (v1 has no tenure term; tenure is capped when added).
    const founder = honestMember(10);
    const newcomer = honestMember(10);
    const c: Cluster = { id: "c", members: [founder, newcomer] };
    const split = memberSplit(c, 1000, P);
    expect(split.get(founder.id)).toBeCloseTo(split.get(newcomer.id)!, 6);
  });
  it("a member who does more verified work earns more (contribution is rewarded)", () => {
    const low = honestMember(2);
    const high = honestMember(20);
    const c: Cluster = { id: "c", members: [low, high] };
    const split = memberSplit(c, 1000, P);
    expect(split.get(high.id)).toBeGreaterThan(split.get(low.id)!);
  });
});

describe("fair-game invariant 7 — positive-sum sharing (only real contributors raise your reward)", () => {
  it("adding a REAL working member raises the cluster's pool share; adding fake A0 seats does not", () => {
    const base = honestCluster("c", 40, 10);
    const other = honestCluster("other", 100, 10); // a comparison cluster so the pool is shared
    const withReal: Cluster[] = [
      { id: "c", members: [...base.members, honestMember(10)] },
      other,
    ];
    const withFakes: Cluster[] = [
      { id: "c", members: [...base.members, ...freeSybilCluster("x", "F", 20).members] },
      other,
    ];
    const shareReal = distributePool(withReal, P).get("c")!;
    const shareFakes = distributePool(withFakes, P).get("c")!;
    const shareBase = distributePool([base, other], P).get("c")!;
    expect(shareReal).toBeGreaterThan(shareBase); // a real contributor grows your share
    expect(shareFakes).toBeLessThan(shareReal); // padding with fakes does not
    // ...and does not beat baseline at all — free A0 seats are excluded from size, so padding is
    // NEUTRAL, not a lever (guards the exact regression: A0 counted toward membersEff → free bump).
    expect(shareFakes).toBeLessThanOrEqual(shareBase + 1e-6);
  });
});

describe("foundational — value conservation + non-negativity (no free money, no negative pay)", () => {
  it("the whole pool is distributed; each cluster reward fully splits to its members; nothing is negative", () => {
    const net: Cluster[] = [
      honestCluster("a", 40, 10),
      honestCluster("b", 100, 8),
      fundedSybilCluster("s", "F", 50, 1), // still has A1+ seats to split to
    ];
    const pool = distributePool(net, P);
    const totalDist = Array.from(pool.values()).reduce((a, b) => a + b, 0);
    expect(totalDist).toBeCloseTo(P.pool, 4); // no free money, no leak — exactly the epoch pool
    for (const v of pool.values()) expect(v).toBeGreaterThanOrEqual(0);
    for (const c of net) {
      const cr = pool.get(c.id) ?? 0;
      const split = memberSplit(c, cr, P);
      const totalSplit = Array.from(split.values()).reduce((a, b) => a + b, 0);
      expect(totalSplit).toBeCloseTo(cr, 6); // a cluster's reward is fully accounted to its members
      for (const v of split.values()) expect(v).toBeGreaterThanOrEqual(0); // no negative payout
    }
  });
});
