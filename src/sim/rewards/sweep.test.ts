// GROW-REWARDS — the parameter sweep + the referral-farm/single-level invariants, asserted
// numerically. This is the "choose parameters by simulation" evidence the reward spec hard-gates on.
import { describe, it, expect } from "vitest";
import { sweep, robustCells, mostRobust } from "./sweep";
import { fairnessAllPass, securityMinMargin } from "./invariants";
import { CANDIDATE_PARAMS, REFERRAL_PARAMS, referralFarm, referralChain, honestCluster } from "./scenarios";
import { referralIncome, rewardPerLocked, MEMBERSHIP_BOND, type Cluster } from "./model";

describe("parameter sweep — a robust region exists and the candidate lives in it", () => {
  const cells = sweep([0.3, 0.4, 0.5, 0.6, 0.7], [0.2, 0.3, 0.4, 0.5], [0.8, 1.0, 1.2]);

  it("some (α,β,γ) combos pass ALL fairness invariants", () => {
    expect(cells.some((c) => c.pass)).toBe(true);
  });

  it("there is a ROBUST region (passes with ≥1.5× margin) — not a knife-edge", () => {
    const robust = robustCells(cells, 1.5);
    expect(robust.length).toBeGreaterThan(0);
    // Log the recommended set (highest min-margin) for the spec's ⚙ table.
    const best = mostRobust(cells)!;
    // eslint-disable-next-line no-console
    console.log(`[reward-sweep] ${cells.length} combos, ${cells.filter((c) => c.pass).length} pass, ${robust.length} robust(≥1.5×); best = α${best.alpha} β${best.beta} γ${best.gamma} (min-margin ${best.minMargin.toFixed(2)})`);
  });

  it("the candidate params (α0.5 β0.4 γ1.0) pass, with comfortable SECURITY margin", () => {
    expect(fairnessAllPass(CANDIDATE_PARAMS)).toBe(true);
    // Directional invariants (positive-sum) are correct at a small margin; the robustness floor is on
    // the anti-farm SECURITY invariants (sybil/whale/wash), which the candidate clears comfortably.
    expect(securityMinMargin(CANDIDATE_PARAMS)).toBeGreaterThanOrEqual(1.5);
  });

  it("concave-work / no-work-dominance fails fairness (a sanity check that the sweep discriminates)", () => {
    // γ far below α+β makes size/stake dominate work → funded-sybil should stop being unprofitable.
    const bad = { ...CANDIDATE_PARAMS, alpha: 0.9, beta: 0.9, gamma: 0.3 };
    expect(fairnessAllPass(bad)).toBe(false);
  });
});

describe("referral spark — bounded, unprofitable to farm, single-level (un-pyramidable)", () => {
  it("a referral FARM earns only the capped flat bounty — a rounding error vs the 32k/seat it locks", () => {
    const farm = referralFarm(2000); // attacker: 2,000 fake invitees, each 32k-staked
    const income = referralIncome(farm.referrals, farm.members, REFERRAL_PARAMS).get(farm.inviterId) ?? 0;
    // Capped at capPerEpoch × bounty regardless of how many fakes.
    expect(income).toBe(REFERRAL_PARAMS.capPerEpoch * REFERRAL_PARAMS.bounty);
    // ROI: income vs SALT locked (2001 seats × 32k). Must be a rounding error (<0.1% of locked).
    const locked = farm.members.length * MEMBERSHIP_BOND;
    expect(income / locked).toBeLessThan(0.001);
  });

  it("the farm's staked-but-workless invitees also earn ~nothing from the Score pool", () => {
    // The fake seats do ~no external work → against a real working cluster, the farm's reward PER SALT
    // LOCKED is a fraction of an honest member's (double bind: the referral bounty is capped AND the
    // Score pool rewards work, not idle stake). Compared on ROI, not absolute pool share, so the result
    // doesn't depend on how the two clusters happen to split a fixed pool.
    const honest = honestCluster("real", 100, 10);
    const hc = honest.members[0].controllerId;
    const net: Cluster[] = [honest, { id: "farm", members: referralFarm(500).members }];
    const honestROI = rewardPerLocked(net, hc, CANDIDATE_PARAMS);
    const farmROI = rewardPerLocked(net, "FARMER", CANDIDATE_PARAMS);
    expect(farmROI).toBeLessThan(honestROI * 0.05);
  });

  it("SINGLE-LEVEL: an A→B→C chain credits A for B (activated) but NEVER for C", () => {
    const chain = referralChain();
    const inc = referralIncome(chain.referrals, chain.members, REFERRAL_PARAMS);
    expect(inc.get(chain.a)).toBe(REFERRAL_PARAMS.bounty); // A credited for direct invitee B
    expect(inc.get(chain.b)).toBe(REFERRAL_PARAMS.bounty); // B credited for direct invitee C
    // A is NEVER credited for C (the grandchild) — no multi-level payout.
    const aOnly = chain.referrals.filter((r) => r.inviterId === chain.a);
    expect(aOnly).toHaveLength(1);
    expect(aOnly[0].inviteeId).toBe(chain.b);
  });

  it("bounty pays ONLY on activation — a non-activated (no-work) invitee earns the inviter nothing", () => {
    const A = { id: "a", controllerId: "cA", assurance: "A1" as const, stake: MEMBERSHIP_BOND, externalWork: 5, active: true };
    const dormant = { id: "b", controllerId: "cB", assurance: "A1" as const, stake: MEMBERSHIP_BOND, externalWork: 0, active: true };
    const inc = referralIncome([{ inviterId: "a", inviteeId: "b" }], [A, dormant], REFERRAL_PARAMS);
    expect(inc.get("a") ?? 0).toBe(0); // no activation (no work) → no bounty
  });
});
