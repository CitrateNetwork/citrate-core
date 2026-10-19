// C1-V (UI truth contract v1, citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md) — money copy.
//
//   V5  three different explanations of grant custody -> one explanation everywhere (§5.4), with
//       the "not purchased, not an investment" line wherever the grant is mentioned (§5.7) and the
//       betanet carry-over line (§5.6). "Staked" only once the bond is activated.
//   V12 "$48" hard-coded in Onboarding and Settings -> one source (§5.3). Here the price source is
//       swapped for $60 to prove every surface reads it rather than a literal.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("../data/membership", async (orig) => {
  const real = await orig<typeof import("../data/membership")>();
  return { ...real, MEMBERSHIP_PRICE: { ...real.MEMBERSHIP_PRICE, usd: 60 }, membershipPrice: () => "$60", membershipPricePerYear: () => "$60/year" };
});

import { S3, S5 } from "../onboarding/Onboarding";
import { Wallet } from "./Wallet";
import { Node } from "./Node";
import { Settings } from "./Settings";
import { GRANT_DISCLAIMER, BETANET_CARRYOVER, grantExplanation } from "../data/grantCopy";
import { describeBond, store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { createDemoProvider, type AgentContext, type ToolCall } from "../agent/harness";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
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
    const ctx: AgentContext = { height: 1, peers: 1, nodeState: "synced", staked: 0, liquid: 0, claimable: null, rewardsTotal: null, walletAddr: "0xabc0000000000000000000000000000000000001", tier: "pilot" };
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
    const base = { bondDeployed: true, unlockBlock: 300_000, isUnlocked: false, isKycVerified: false, hasValidator: false };
    expect(describeBond(base)).toBe("Bond locked · not staked · until block 300,000");
    expect(describeBond({ ...base, hasValidator: true })).toBe("Staked · locked until block 300,000");
    expect(describeBond({ ...base, isUnlocked: true })).toBe("Unlocked · verify identity to withdraw");
    expect(describeBond({ ...base, isUnlocked: true, isKycVerified: true, hasValidator: true })).toBe("Unlocked");
  });
});

describe("V12 — the membership price has one source", () => {
  it("S3 shows the price from the membership source, never a literal $48", () => {
    const html = renderToStaticMarkup(<S3 store={stub} s={member({ s3: "idle", hasGrant: false })} />);
    expect(html).toContain("$60");
    expect(html).not.toContain("$48");
    expect(html).toContain("does not buy SALT");
  });

  it("Settings shows the price from the membership source", () => {
    const html = renderToStaticMarkup(<Settings store={stub} s={member({ sSec: "billing" })} />);
    expect(html).toContain("$60/year");
    expect(html).not.toContain("$48");
  });

  it("no surface hard-codes a $48 string", () => {
    for (const f of ["src/onboarding/Onboarding.tsx", "src/surfaces/Settings.tsx", "src/surfaces/Commissary.tsx"]) {
      const code = readFileSync(f, "utf8")
        .split("\n")
        .filter((l) => !/^\s*(\/\/|\*|\{\/\*)/.test(l))
        .join("\n");
      expect(code, f).not.toContain("$48");
    }
  });
});
