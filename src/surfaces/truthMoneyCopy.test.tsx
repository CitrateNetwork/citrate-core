// C1-V (UI truth contract v1, citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md) — money copy.
//
//   V5  three different explanations of grant custody -> one explanation everywhere (§5.4), with
//       the "not purchased, not an investment" line wherever the grant is mentioned (§5.7) and the
//       betanet carry-over line (§5.6). "Staked" only once the bond is activated.
//   V12 "$48" hard-coded in Onboarding and Settings -> one source (§5.3). Chain-team ruling (no
//       waiver): until core-membership serves a public price route, the app shows NO number and
//       links to the membership page ("See membership pricing") instead.
//   V13 Community promised referral rewards with no on-chain source, and "settled" (§5.5, §1).
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { Onboarding, S3, S5 } from "../onboarding/Onboarding";
import { Community } from "./Community";
import { MEMBERSHIP_PRICING_URL } from "../data/membership";
import { Wallet } from "./Wallet";
import { Node } from "./Node";
import { Settings } from "./Settings";
import { GRANT_DISCLAIMER, BETANET_CARRYOVER, grantExplanation } from "../data/grantCopy";
import { describeBond, store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { createDemoProvider, type AgentContext, type ToolCall } from "../agent/harness";
import { bridge } from "../bridge";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  claimAccountMismatch: () => false,
  openExternal: async () => {},
  toast: () => {},
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "sub-1", wallet: "0x" + "0".repeat(40), tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, tier: "pilot", hasGrant: true, hasSbt: true, s5StakeWei: wei(32000), s5UnlockBlock: 16_000_000, ...over };
}

// The old, conflicting custody stories. None may come back on any surface.
const OLD_STORIES = [
  "arrives staked",
  "staked for you",
  "vaulted until mainnet release",
  "stays vaulted until mainnet release",
  "until mainnet release policy unlocks vaulted grants",
  "funds this wallet directly",
  "a staked + time-locked model is in progress",
  "time-locked (about a year), recoverable validator bond",
  "LiquidStakingPool — your full membership grant",
  "staked to your validator",
];

function expectOneStory(html: string) {
  for (const old of OLD_STORIES) expect(html).not.toContain(old);
  expect(html).toContain(GRANT_DISCLAIMER);
  expect(html).toContain(BETANET_CARRYOVER);
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("V5 — one grant-custody explanation everywhere", () => {
  it("says the grant is locked in the bond and not staked until activation", () => {
    const t = grantExplanation({ amountSalt: "32,000", unlockBlock: 16_000_000, staked: false });
    expect(t).toContain("Your membership comes with a 32,000 SALT grant, locked in your bond until block 16,000,000.");
    expect(t).not.toMatch(/\bstaked\b(?! until)/);
    const staked = grantExplanation({ amountSalt: "32,000", unlockBlock: 16_000_000, staked: true });
    expect(staked).toContain("staked");
  });

  it("onboarding S3 (membership) uses it", () => {
    expectOneStory(renderToStaticMarkup(<S3 store={stub} s={member({ s3: "idle", hasGrant: false })} />));
  });

  it("onboarding S5 before and after the grant uses it", () => {
    expectOneStory(renderToStaticMarkup(<S5 store={stub} s={member({ s5: "idle", hasGrant: false })} />));
    const settled = renderToStaticMarkup(<S5 store={stub} s={member({ s5: "settled" })} />);
    expectOneStory(settled);
    expect(settled).not.toContain("SALT staked · settled");
  });

  it("the Wallet staking tab uses it", () => {
    expectOneStory(renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "staking" })} />));
  });

  it("the Node validator card uses it for a member with a grant", () => {
    expectOneStory(renderToStaticMarkup(<Node store={stub} s={member({ node: "synced", nTab: "ops" })} />));
  });

  it("the Settings grant record carries the disclaimer", () => {
    const html = renderToStaticMarkup(<Settings store={stub} s={member({ sSec: "billing" })} />);
    expect(html).toContain(GRANT_DISCLAIMER);
    expect(html).not.toContain("Grant · staked at issuance");
  });

  it("the Hermes context uses it", () => {
    store.setState({ hasGrant: true, s5StakeWei: wei(32000), s5UnlockBlock: 16_000_000, bondedStake: 0 });
    const note = (store.snapshot() as { membership: { note: string } }).membership.note;
    for (const old of OLD_STORIES) expect(note).not.toContain(old);
    expect(note).toContain("locked in your bond until block 16,000,000");
    expect(note).toContain(GRANT_DISCLAIMER);
  });

  it("the built-in agent's staking reply uses it, with no LiquidStakingPool or mainnet-release story", async () => {
    vi.useFakeTimers();
    const ctx: AgentContext = { height: 1, peers: 1, nodeState: "synced", staked: 0, liquid: 0, claimable: null, rewardsTotal: null, walletAddr: "0xabc0000000000000000000000000000000000001", tier: "pilot", grant: { amountSalt: 32_000, unlockBlock: 16_000_000, staked: false } };
    let text = "";
    const done = createDemoProvider(() => ctx).send({
      messages: [{ role: "user", content: "what's my staking position?" }],
      callbacks: { onStatus: () => {}, onToken: (t: string) => (text += t), onToolCall: async (_c: ToolCall) => "ok" },
    });
    await vi.runAllTimersAsync();
    await done;
    for (const old of OLD_STORIES) expect(text).not.toContain(old);
    expect(text).not.toContain("LiquidStakingPool");
    expect(text).not.toContain("mainnet release");
    expect(text).toContain(GRANT_DISCLAIMER);
  });

  it("the bond status uses the §1 membership words and says 'staked' only after activation", () => {
    const base = { bondDeployed: true, unlockBlock: 300_000, isUnlocked: false, isKycVerified: false, hasValidator: false, bondActivated: false as boolean | null };
    expect(describeBond(base)).toBe("Bond locked · not staked · until block 300,000");
    expect(describeBond({ ...base, hasValidator: true })).toBe("Staked · locked until block 300,000");
    expect(describeBond({ ...base, isUnlocked: true })).toBe("Unlocked · not activated");
    expect(describeBond({ ...base, isUnlocked: true, bondActivated: true })).toBe("Unlocked · verify identity to withdraw");
    expect(describeBond({ ...base, isUnlocked: true, isKycVerified: true, hasValidator: true })).toBe("Unlocked");
  });
});

describe("V12 — no in-app price until core-membership serves one", () => {
  const PRICE = /\$\s?\d/;

  it("S3 shows a 'See membership pricing' link and no price", () => {
    const html = renderToStaticMarkup(<S3 store={stub} s={member({ s3: "idle", hasGrant: false })} />);
    expect(html).not.toMatch(PRICE);
    expect(html).toContain("See membership pricing");
    expect(html).toContain(MEMBERSHIP_PRICING_URL);
    expect(html).toContain("does not buy SALT");
  });

  it("Settings shows the link and no price", () => {
    const html = renderToStaticMarkup(<Settings store={stub} s={member({ sSec: "billing" })} />);
    expect(html).not.toMatch(PRICE);
    expect(html).toContain("See membership pricing");
  });

  it("no surface, and not membership.ts, carries a price literal", () => {
    for (const f of ["src/onboarding/Onboarding.tsx", "src/surfaces/Settings.tsx", "src/surfaces/Commissary.tsx", "src/data/membership.ts"]) {
      const code = readFileSync(f, "utf8")
        .split("\n")
        .filter((l) => !/^\s*(\/\/|\*|\{\/\*)/.test(l))
        .join("\n");
      expect(code, f).not.toMatch(/\$\d|usd:\s*\d/);
    }
  });

  // #349 review (pysol-dev): membership.citrate.ai describes the membership service and shows no
  // price; the public page that states it is citrate.ai/membership. Not the checkout service either.
  it("the pricing link points at the public page that states the price", () => {
    expect(MEMBERSHIP_PRICING_URL).toBe("https://citrate.ai/membership");
    const host = new URL(MEMBERSHIP_PRICING_URL).host;
    expect(host).not.toBe("membership.citrate.ai");
    expect(host).not.toBe(new URL(freshState("p1").coreMembershipUrl).host);
  });
});

describe("#349 review — remaining grant mentions use the one explanation", () => {
  it("the claim banner and the S3 stalled-grant card say 'grant, locked in your bond' with the disclaimer", () => {
    const claim = renderToStaticMarkup(
      <Onboarding store={stub} s={member({ stage: "s3", s3: "idle", hasGrant: false, authEmail: "m@example.com", pendingClaim: { email: "m@example.com" } as never })} />,
    );
    expect(claim).not.toContain("SALT stake");
    expect(claim).toContain(GRANT_DISCLAIMER);
    const stalled = renderToStaticMarkup(<S3 store={stub} s={member({ s3: "paying", s3PollExhausted: true, hasGrant: false })} />);
    expect(stalled).not.toContain("SALT stake");
    expect(stalled).toContain(GRANT_DISCLAIMER);
  });

  it("a granted member whose unlock block is unread sees '—', not 'about a year'", () => {
    expect(grantExplanation({ amountSalt: "32,000", unlockBlock: null, staked: false, granted: true })).toContain("until block — (not read yet)");
    expect(grantExplanation({ amountSalt: "32,000", unlockBlock: null, staked: false })).toContain("for about a year");
  });

  it("the agent's staking reply branches on the real grant state and unlock block", async () => {
    vi.useFakeTimers();
    const ask = async (ctx: AgentContext) => {
      let text = "";
      const done = createDemoProvider(() => ctx).send({
        messages: [{ role: "user", content: "what's my staking position?" }],
        callbacks: { onStatus: () => {}, onToken: (t: string) => (text += t), onToolCall: async (_c: ToolCall) => "ok" },
      });
      await vi.runAllTimersAsync();
      await done;
      return text;
    };
    const base: AgentContext = { height: 1, peers: 1, nodeState: "synced", staked: 0, liquid: 0, claimable: null, rewardsTotal: null, walletAddr: "0xabc0000000000000000000000000000000000001", tier: "free", grant: null };
    const free = await ask(base);
    expect(free).not.toContain("Your membership comes with");
    expect(free).toContain("no membership grant");
    const granted = await ask({ ...base, tier: "pilot", grant: { amountSalt: 32_000, unlockBlock: 16_000_000, staked: false } });
    expect(granted).toContain("until block 16,000,000");
    expect(granted).toContain(GRANT_DISCLAIMER);
  });

  it("the Hermes context carries the grant facts", () => {
    store.setState({ hasGrant: true, s5StakeWei: wei(32000), s5UnlockBlock: 16_000_000, bondedStake: 0, memberBond: { deployed: true, activated: false, pubkey: null, unlocked: false } });
    expect(store.snapshot().grant).toEqual({ amountSalt: 32000, unlockBlock: 16_000_000, staked: false, activated: false });
    store.setState({ hasGrant: false });
    expect(store.snapshot().grant).toBeNull();
  });
});

describe("V13 — referrals describe only what is recorded", () => {
  it("Community promises no referral reward and says nothing 'settled'", () => {
    const html = renderToStaticMarkup(<Community store={stub} s={member()} />);
    expect(html.toLowerCase()).not.toMatch(/reward|settle/);
    expect(html).not.toContain("SALT grant to the whole group");
    expect(html).toContain("recorded");
  });
});

// #349 review (Codex round 2): the bond's unlock block is a chain read, never persisted. A member
// whose onboarding is settled must still get it on launch, or every grant explanation says
// "until block — (not read yet)" for good.
describe("#349 round 2 — the unlock block loads on every launch", () => {
  const settledGrant = {
    attributedStakeWei: wei(32000),
    attributedPrincipalWei: wei(32000),
    hasSbt: true,
    bondAddress: "0x" + "b".repeat(40),
    bondDeployed: true,
    bondedStakeWei: wei(32000),
    hasValidator: true,
    bondActivated: true,
    unlockBlock: 16_000_000,
    isUnlocked: false,
    isKycVerified: true,
  };

  it("the startup reconcile folds unlockBlock for a settled, fully bonded member", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(settledGrant as never);
    store.setState({ hasGrant: true, s3: "settled", s5: "settled", bondedStake: 32_000, s5UnlockBlock: null });
    await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
    expect(store.state.s5UnlockBlock).toBe(16_000_000);
    const text = grantExplanation({ amountSalt: "32,000", unlockBlock: store.state.s5UnlockBlock, staked: true, granted: true });
    expect(text).toContain("16,000,000");
    expect(text).not.toContain("not read yet");
  });

  it("a grant that isn't on chain leaves the unlock block as it was", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue({ ...settledGrant, attributedStakeWei: "0", hasSbt: false, bondDeployed: false, unlockBlock: null } as never);
    store.setState({ hasGrant: true, s3: "settled", s5: "settled", s5UnlockBlock: null });
    await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
    expect(store.state.s5UnlockBlock).toBeNull();
  });
});
