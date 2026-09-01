// GROW-REWARDS — scenario generators + a candidate parameter set. TEST-ONLY analysis tooling.
import type { Cluster, Member, RewardParams } from "./model";
import { MEMBERSHIP_BOND } from "./model";

/** A candidate parameter set to validate. The exponents make WORK dominant (γ=1, multiplicative) while
 *  size and stake are concave floors (α,β<1) — so no single lever farms, and no-work ⇒ no reward. */
export const CANDIDATE_PARAMS: RewardParams = {
  alpha: 0.5,
  beta: 0.4,
  gamma: 1.0,
  assurance: { A0: 0.02, A1: 1.0, A2: 1.5, A3: 2.5 },
  tiers: [
    { name: "Seed", minMembers: 0, minPerMemberWork: 0, minStake: 0, multiplier: 1.0 },
    { name: "Circle", minMembers: 25, minPerMemberWork: 3, minStake: 25 * MEMBERSHIP_BOND, multiplier: 1.3 },
    { name: "Guild", minMembers: 250, minPerMemberWork: 6, minStake: 250 * MEMBERSHIP_BOND, multiplier: 1.7 },
    { name: "Flagship", minMembers: 2000, minPerMemberWork: 10, minStake: 2000 * MEMBERSHIP_BOND, multiplier: 2.2 },
  ],
  pool: 1_000_000,
  baseFraction: 0.3,
};

let seq = 0;
const uid = (p: string) => `${p}-${seq++}`;

/** An honest member: a real person, own controller, A1 (32k bonded), real external work. */
export function honestMember(work: number, assurance: Member["assurance"] = "A1"): Member {
  const id = uid("m");
  return { id, controllerId: uid("ctrl"), assurance, stake: MEMBERSHIP_BOND, externalWork: work, active: true };
}

/** An honest cluster of `n` distinct real people, each doing `workPer` verified external work. */
export function honestCluster(id: string, n: number, workPer: number, assurance: Member["assurance"] = "A1"): Cluster {
  return { id, members: Array.from({ length: n }, () => honestMember(workPer, assurance)) };
}

/** A funded-sybil cluster: ONE controller, `n` A1 seats each 32k-bonded (real cost!), ~no real
 *  external work (fake volume nets to 0). This is the F1 attack under the 32k floor. */
export function fundedSybilCluster(id: string, controllerId: string, n: number, fakeWorkPer = 0): Cluster {
  return {
    id,
    members: Array.from({ length: n }, () => ({
      id: uid("s"),
      controllerId,
      assurance: "A1" as const,
      stake: MEMBERSHIP_BOND,
      externalWork: fakeWorkPer, // externally-demanded work a sybil can't manufacture → ~0
      active: true,
    })),
  };
}

/** A free-sybil cluster: ONE controller, `n` A0 seats (no stake, no work). Costs nothing; earns ~0. */
export function freeSybilCluster(id: string, controllerId: string, n: number): Cluster {
  return {
    id,
    members: Array.from({ length: n }, () => ({
      id: uid("f"),
      controllerId,
      assurance: "A0" as const,
      stake: 0,
      externalWork: 0,
      active: true,
    })),
  };
}

/** A wash-trade cluster: members all under common control "buying/selling" to each other → the
 *  external-demand netting makes their verified external work 0. */
export function washCluster(id: string, controllerId: string, n: number): Cluster {
  return {
    id,
    members: Array.from({ length: n }, () => ({
      id: uid("w"),
      controllerId, // common control → intra-cluster volume is NOT external → nets to 0
      assurance: "A1" as const,
      stake: MEMBERSHIP_BOND,
      externalWork: 0,
      active: true,
    })),
  };
}

/** A whale: one controller over-bonds a single seat (many multiples of 32k) but does little work. */
export function whaleMember(stakeMultiples: number, work: number): Member {
  const id = uid("whale");
  return { id, controllerId: uid("whalectrl"), assurance: "A1", stake: stakeMultiples * MEMBERSHIP_BOND, externalWork: work, active: true };
}

import type { Referral, ReferralParams } from "./model";

/** Candidate referral params: a small flat bounty, capped, so recruitment can never be the money. */
export const REFERRAL_PARAMS: ReferralParams = { bounty: 500, welcome: 500, capPerEpoch: 10 };

let rseq = 0;
const ruid = (p: string) => `${p}-r${rseq++}`;

/** A real activated invitee (staked A1 + real work) under a distinct controller. */
function activatedInvitee(work = 5): Member {
  return { id: ruid("inv"), controllerId: ruid("ctrl"), assurance: "A1", stake: MEMBERSHIP_BOND, externalWork: work, active: true };
}

/** A referral FARM: one attacker controls the inviter + `n` invitee seats, each 32k-staked with a
 *  sliver of work so they "activate". Returns the members + the direct referral pairs. The attacker
 *  pays `n × 32k` locked to collect a capped flat bounty — the invariant proves that ROI is terrible. */
export function referralFarm(n: number): { inviterId: string; members: Member[]; referrals: Referral[] } {
  const inviterId = ruid("farmer-inviter");
  const inviter: Member = { id: inviterId, controllerId: "FARMER", assurance: "A1", stake: MEMBERSHIP_BOND, externalWork: 5, active: true };
  const invitees: Member[] = Array.from({ length: n }, () => ({
    id: ruid("fake"),
    controllerId: "FARMER",
    assurance: "A1" as const,
    stake: MEMBERSHIP_BOND,
    externalWork: 0.01, // just enough to "activate" — real external demand a farmer can't fake
    active: true,
  }));
  const referrals: Referral[] = invitees.map((iv) => ({ inviterId, inviteeId: iv.id }));
  return { inviterId, members: [inviter, ...invitees], referrals };
}

/** A 2-hop chain A→B→C, to prove single-level: A is credited for B (if activated), NEVER for C. */
export function referralChain(): { a: string; b: string; c: Member; members: Member[]; referrals: Referral[] } {
  const A = activatedInvitee();
  const B = activatedInvitee();
  const C = activatedInvitee();
  return {
    a: A.id,
    b: B.id,
    c: C,
    members: [A, B, C],
    referrals: [
      { inviterId: A.id, inviteeId: B.id },
      { inviterId: B.id, inviteeId: C.id },
    ],
  };
}

export { activatedInvitee };
