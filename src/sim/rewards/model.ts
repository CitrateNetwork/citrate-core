// GROW-REWARDS — reward-model math (pure). Analysis tooling for docs/CLUSTER_REWARDS_SPEC.md.
//
// TEST-ONLY: nothing in the app imports this; it exists so the reward mechanism's fair-game invariants
// can be simulated + asserted numerically (the red-team hard-gate #1) before any SALT is paid. The
// formula is the spec's blend: Score = Tier × Members_eff^α × Stake^β × Work^γ, with concave α,β and a
// dominant γ. The membership bond (32k SALT/seat) is the A1 sybil floor.

export const MEMBERSHIP_BOND = 32_000; // SALT bonded per A1 seat (LOCKED — the existing membership)

export type Assurance = "A0" | "A1" | "A2" | "A3";

export interface Member {
  id: string;
  /** Who REALLY controls this seat — seats sharing a controllerId are common-control (sybil/wash). */
  controllerId: string;
  assurance: Assurance;
  /** SALT bonded on this seat (0 for A0). */
  stake: number;
  /** Verified, EXTERNALLY-demanded work this epoch. Wash/self-dealt volume is already netted to 0. */
  externalWork: number;
  active: boolean;
}

export interface Cluster {
  id: string;
  members: Member[];
}

export interface RewardParams {
  alpha: number; // members exponent (concave, <1)
  beta: number; // stake exponent (concave, <1)
  gamma: number; // work exponent (dominant, ~1)
  assurance: Record<Assurance, number>; // reward weight per assurance tier
  tiers: TierGate[]; // ordered low→high; the highest whose gate passes applies
  pool: number; // SALT distributed this epoch
  /** Member split: equal participation floor as a fraction of a member's cluster reward. */
  baseFraction: number;
}

export interface TierGate {
  name: string;
  minMembers: number; // A1+ active members
  minPerMemberWork: number; // work / A1+active members
  minStake: number; // total bonded
  multiplier: number;
}

const EPS = 1e-9;

const a1plus = (m: Member) => m.assurance !== "A0";

/** Effective members: Σ assurance-weight over ACTIVE A1+ members. A0 (free identity) is EXCLUDED —
 *  the sybil floor: a free seat adds neither reward (memberSplit is A1+-only) nor size, so padding a
 *  working cluster with free seats cannot raise its pool share (fair-game invariant 7). */
export function membersEff(c: Cluster, p: RewardParams): number {
  return c.members.filter((m) => m.active && a1plus(m)).reduce((s, m) => s + p.assurance[m.assurance], 0);
}

export function stakeTotal(c: Cluster): number {
  return c.members.reduce((s, m) => s + m.stake, 0);
}

/** Cluster work = Σ externally-demanded verified work of active members (wash already netted to 0). */
export function clusterWork(c: Cluster): number {
  return c.members.filter((m) => m.active).reduce((s, m) => s + m.externalWork, 0);
}

/** The highest tier whose BLENDED gate (members AND per-member work AND stake) all pass. */
export function clusterTier(c: Cluster, p: RewardParams): TierGate {
  const active = c.members.filter((m) => m.active && a1plus(m));
  const n = active.length;
  const work = clusterWork(c);
  const perMember = n > 0 ? work / n : 0;
  const stake = stakeTotal(c);
  let best = p.tiers[0];
  for (const t of p.tiers) {
    if (n >= t.minMembers && perMember >= t.minPerMemberWork && stake >= t.minStake) best = t;
  }
  return best;
}

/** Score(c) = Tier × Members_eff^α × Stake^β × Work^γ. */
export function score(c: Cluster, p: RewardParams): number {
  const tier = clusterTier(c, p);
  const me = Math.pow(Math.max(membersEff(c, p), EPS), p.alpha);
  const st = Math.pow(Math.max(stakeTotal(c), 1), p.beta);
  const wk = Math.pow(Math.max(clusterWork(c), EPS), p.gamma);
  return tier.multiplier * me * st * wk;
}

/** Distribute the epoch pool across clusters proportional to Score. */
export function distributePool(clusters: Cluster[], p: RewardParams): Map<string, number> {
  const scores = clusters.map((c) => ({ c, s: score(c, p) }));
  const total = scores.reduce((s, x) => s + x.s, 0);
  const out = new Map<string, number>();
  for (const { c, s } of scores) out.set(c.id, total > EPS ? (s / total) * p.pool : 0);
  return out;
}

/** Member split of a cluster's reward: an equal participation base (newcomer-fair) + a
 *  contribution-weighted remainder. Only active A1+ members share. */
export function memberSplit(c: Cluster, clusterReward: number, p: RewardParams): Map<string, number> {
  const out = new Map<string, number>();
  const eligible = c.members.filter((m) => m.active && a1plus(m));
  // Guard undefined/NaN/≤0 (a missing pool entry must yield 0, never NaN that silently beats a compare).
  if (eligible.length === 0 || !(clusterReward > 0)) {
    for (const m of c.members) out.set(m.id, 0);
    return out;
  }
  const basePot = clusterReward * p.baseFraction;
  const contribPot = clusterReward - basePot;
  const totalWork = eligible.reduce((s, m) => s + m.externalWork, 0);
  for (const m of c.members) {
    if (!m.active || !a1plus(m)) {
      out.set(m.id, 0);
      continue;
    }
    const base = basePot / eligible.length;
    const contrib = totalWork > EPS ? (m.externalWork / totalWork) * contribPot : contribPot / eligible.length;
    out.set(m.id, base + contrib);
  }
  return out;
}

/** Total SALT a controller has locked across all their seats (the denominator for reward-per-locked). */
export function controllerLockedStake(clusters: Cluster[], controllerId: string): number {
  let s = 0;
  for (const c of clusters) for (const m of c.members) if (m.controllerId === controllerId) s += m.stake;
  return s;
}

/** Total reward a controller earns across all their seats this epoch. */
export function controllerReward(clusters: Cluster[], controllerId: string, p: RewardParams): number {
  const pool = distributePool(clusters, p);
  let r = 0;
  for (const c of clusters) {
    const cr = pool.get(c.id) ?? 0;
    const split = memberSplit(c, cr, p);
    for (const m of c.members) if (m.controllerId === controllerId) r += split.get(m.id) ?? 0;
  }
  return r;
}

/** Reward-per-32k-locked — the anti-farm metric. Honest work → high; sybil (stake, no work) → ~0. */
export function rewardPerLocked(clusters: Cluster[], controllerId: string, p: RewardParams): number {
  const locked = controllerLockedStake(clusters, controllerId);
  const reward = controllerReward(clusters, controllerId, p);
  return locked > 0 ? reward / (locked / MEMBERSHIP_BOND) : reward; // reward per 32k-seat locked
}

// ---------------------------------------------------------------------------
// GROW-S1b — the referral SPARK (separate from Score): a flat, one-time,
// single-level, capped bounty paid ONLY when an invitee ACTIVATES (staked A1+ and
// did real work). Structurally un-pyramidable: only direct (inviter, invitee)
// pairs are credited — an A→B→C chain never credits A for C.
// ---------------------------------------------------------------------------

export interface ReferralParams {
  /** Flat bounty to the inviter per ACTIVATED invitee (SALT). */
  bounty: number;
  /** One-time welcome grant to the invitee on activation (SALT). */
  welcome: number;
  /** Max activated invitees an inviter is paid for per epoch (anti-farm cap). */
  capPerEpoch: number;
}

export interface Referral {
  inviterId: string;
  inviteeId: string;
}

/** An invitee "activates" when they're a real staked member (A1+) AND produced verified work — NOT on
 *  signup. This is what makes the bounty reward a real participant, never headcount. */
export function isActivated(m: Member): boolean {
  return m.assurance !== "A0" && m.stake > 0 && m.externalWork > 0;
}

/** Referral income per inviter this epoch: count ACTIVATED direct invitees, cap it, × bounty. SINGLE
 *  LEVEL — a referral is a direct pair; nothing credits an inviter for their invitee's invitees. */
export function referralIncome(
  referrals: Referral[],
  members: Member[],
  rp: ReferralParams,
): Map<string, number> {
  const byId = new Map(members.map((m) => [m.id, m]));
  const activated = new Map<string, number>();
  for (const r of referrals) {
    const invitee = byId.get(r.inviteeId);
    if (invitee && isActivated(invitee)) activated.set(r.inviterId, (activated.get(r.inviterId) ?? 0) + 1);
  }
  const out = new Map<string, number>();
  for (const [inviter, n] of activated) out.set(inviter, Math.min(n, rp.capPerEpoch) * rp.bounty);
  return out;
}
