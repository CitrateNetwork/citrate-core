// fxEARN (review P2-1 on #556): the built-in agent's rewards reply must not promise a claim the app
// refuses. When Node › Rewards shows a reason instead of Claim (bondClaimBlocked: on live r1005 the
// activated bond reads MemberBond.isKycVerified() = false), the reply gives that reason; below the
// 5 SALT dust threshold it says so; only otherwise does it describe the claim ceremony.
import { afterEach, describe, expect, it, vi } from "vitest";
import { createDemoProvider, type AgentContext, type ToolCall } from "./harness";

const KYC_REASON = "Claiming needs identity verification on chain: MemberBond.isKycVerified() reads false, and the bond's claimRewards() requires it.";
const base: AgentContext = { height: 877_400, peers: 4, nodeState: "synced", staked: 32_000, liquid: 10, claimable: 0, rewardsTotal: 0, walletAddr: "0x5aeb8c04c103b46d184307d29a6a58803832bd13", tier: "pilot" };

async function ask(ctx: AgentContext, q = "what are my rewards?"): Promise<string> {
  vi.useFakeTimers();
  let text = "";
  const done = createDemoProvider(() => ctx).send({
    messages: [{ role: "user", content: q }],
    callbacks: { onStatus: () => {}, onToken: (t: string) => (text += t), onToolCall: async (_c: ToolCall) => "ok" },
  });
  await vi.runAllTimersAsync();
  await done;
  return text;
}

afterEach(() => vi.useRealTimers());

describe("fxEARN — the rewards reply follows the claim gate", () => {
  it("a blocked claim: the reply gives the reason and promises no claim", async () => {
    const said = await ask({ ...base, claimable: 12, rewardsTotal: 20, claimBlocked: KYC_REASON });
    expect(said).toContain("12.00 SALT");
    expect(said).toContain("MemberBond.isKycVerified()");
    expect(said).not.toContain("Claiming signs a claimRewards() transaction");
  });

  it("the live r1005 member (0 rewards, KYC false): no claim is promised", async () => {
    const said = await ask({ ...base, claimBlocked: KYC_REASON });
    expect(said).toContain("0.00 SALT");
    expect(said).not.toContain("Claiming signs a claimRewards() transaction");
  });

  it("below the 5 SALT dust threshold: no claim is promised", async () => {
    const said = await ask({ ...base, claimable: 4.99, rewardsTotal: 6, claimBlocked: null });
    expect(said).toMatch(/5 SALT/);
    expect(said).not.toContain("Claiming signs a claimRewards() transaction");
  });

  it("claimable at or above 5 SALT and not blocked: the ceremony is described", async () => {
    const said = await ask({ ...base, claimable: 5, rewardsTotal: 6, claimBlocked: null });
    expect(said).toContain("Claiming signs a claimRewards() transaction");
  });
});
