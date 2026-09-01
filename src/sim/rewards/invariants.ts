// GROW-REWARDS — the PARAM-DEPENDENT fair-game invariants as evaluable predicates (pure), so the
// parameter sweep can score any (α,β,γ) and find the robust region. Each returns pass + a MARGIN (how
// comfortably it held) so the sweep can prefer parameters that pass with room, not just barely.
import { distributePool, memberSplit, rewardPerLocked, controllerReward, type Cluster, type RewardParams } from "./model";
import { honestCluster, honestMember, fundedSybilCluster, freeSybilCluster, washCluster, whaleMember } from "./scenarios";

export interface FairnessResult {
  name: string;
  pass: boolean;
  /** How comfortably it passed (>1 = room). */
  margin: number;
  /** true = an anti-farm SECURITY bound (sybil/whale/wash) where a big margin matters; false = a
   *  DIRECTIONAL property (effort-beats-size, positive-sum) that is correct at a small margin (e.g.
   *  one member is ~1/N of a cluster) — so it's not held to the robustness floor. */
  security: boolean;
}

function perMemberReward(net: Cluster[], id: string, P: RewardParams): number {
  const pool = distributePool(net, P);
  const c = net.find((x) => x.id === id)!;
  const r = pool.get(id) ?? 0;
  const active = c.members.filter((m) => m.active && m.assurance !== "A0").length;
  return active > 0 ? r / active : 0;
}
const ratio = (good: number, bound: number) => (bound > 0 ? good / bound : good > 0 ? Infinity : 0);

/** Evaluate the six score-based fairness invariants at parameters `P`. */
export function evaluateFairness(P: RewardParams): FairnessResult[] {
  const out: FairnessResult[] = [];

  // 1 — effort beats size: a small high-work cluster out-earns a large idle one, per member.
  {
    const net = [honestCluster("small", 50, 20), honestCluster("big", 500, 1)];
    const s = perMemberReward(net, "small", P);
    const b = perMemberReward(net, "big", P);
    out.push({ name: "effort-beats-size", pass: s > b, margin: ratio(s, b), security: false });
  }
  // 2 — funded sybil unprofitable: 2,000 staked-but-workless seats earn <5% the honest reward-per-locked.
  {
    const honest = honestCluster("h", 100, 10);
    const hc = honest.members[0].controllerId;
    const net = [honest, fundedSybilCluster("s", "ATK", 2000, 0.01)];
    const hr = rewardPerLocked(net, hc, P);
    const sr = rewardPerLocked(net, "ATK", P);
    out.push({ name: "funded-sybil-unprofitable", pass: sr < hr * 0.05, margin: ratio(hr * 0.05, sr), security: true });
  }
  // 3 — free sybil negligible: 5,000 A0 seats collect <1% of the pool.
  {
    const net = [honestCluster("h", 100, 10), freeSybilCluster("f", "FREE", 5000)];
    const fr = controllerReward(net, "FREE", P) / P.pool;
    out.push({ name: "free-sybil-negligible", pass: fr < 0.01, margin: ratio(0.01, fr), security: true });
  }
  // 4 — whale can't buy dominance: an honest high-work member out-earns a 50× whale.
  {
    const honest = honestCluster("workers", 60, 15);
    const whale: Cluster = { id: "whale", members: [whaleMember(50, 1)] };
    const net = [honest, whale];
    const pool = distributePool(net, P);
    const oneH = [...memberSplit(honest, pool.get("workers") ?? 0, P).values()][0] ?? 0;
    const w = [...memberSplit(whale, pool.get("whale") ?? 0, P).values()][0] ?? 0;
    out.push({ name: "whale-bounded", pass: oneH > w, margin: ratio(oneH, w), security: true });
  }
  // 5 — wash-trade negligible: common-control external work nets to 0 → <1% of the pool.
  {
    const net = [honestCluster("h", 100, 10), washCluster("w", "WASH", 200)];
    const wr = controllerReward(net, "WASH", P) / P.pool;
    out.push({ name: "wash-negligible", pass: wr < 0.01, margin: ratio(0.01, wr), security: true });
  }
  // 6 — positive-sum sharing: a real member raises your share; free padding does NOT beat baseline.
  {
    const base = honestCluster("c", 40, 10);
    const other = honestCluster("o", 100, 10);
    const withReal: Cluster[] = [{ id: "c", members: [...base.members, honestMember(10)] }, other];
    const withFakes: Cluster[] = [{ id: "c", members: [...base.members, ...freeSybilCluster("x", "F", 20).members] }, other];
    const sB = distributePool([base, other], P).get("c") ?? 0;
    const sR = distributePool(withReal, P).get("c") ?? 0;
    const sF = distributePool(withFakes, P).get("c") ?? 0;
    out.push({ name: "positive-sum-sharing", pass: sR > sB && sF <= sB + 1e-6, margin: ratio(sR, sB), security: false });
  }
  return out;
}

export const fairnessAllPass = (P: RewardParams): boolean => evaluateFairness(P).every((r) => r.pass);
export const fairnessMinMargin = (P: RewardParams): number => Math.min(...evaluateFairness(P).map((r) => r.margin));
/** The min margin over the SECURITY (anti-farm) invariants only — the robustness metric the sweep
 *  uses. Directional invariants (effort, positive-sum) are correct at small margins, so excluded. */
export const securityMinMargin = (P: RewardParams): number =>
  Math.min(...evaluateFairness(P).filter((r) => r.security).map((r) => r.margin));
