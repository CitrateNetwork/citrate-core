// Review on #563: Node › Rewards draws Claim only where the claim the app builds pays the figure shown.
// On the bond path the reason replaces the button until MemberBond.isKycVerified() reads true
// (bondClaimBlocked, contract v1.2 §1). #556 (9328537) removed the separate rule for a key bonded
// straight from the member's wallet: its premise was wrong (the member's key is staked through its
// MemberBond), so bondClaimBlocked is the only reason and that rule must not come back here.
import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));

import { renderRewards } from "./nodePage.fixture";
import { freshState, type AppState } from "../../shell/state";
import type { Store } from "../../shell/store";

const WEI = 10n ** 18n;
const KEY = "0x" + "3f".repeat(32);
const stub = { claimRewards: vi.fn(async () => {}), refreshEarnings: async () => {}, toast: () => {} } as unknown as Store;
const claimButton = /<button[^>]*data-testid="claim"[^>]*>\s*Claim/;

// An activated MemberBond whose key has 20 SALT claimable now; KYC as read from the bond.
const bondKey = (over: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  signedIn: true,
  node: "synced",
  hasGrant: true,
  bondActivated: true,
  bondPubkey: KEY,
  bondKycVerified: true,
  bondedStake: 32_000,
  validatorPubkey: KEY,
  validatorActivation: { isActive: true, status: 1, actEpoch: 1, currentEpoch: 2 },
  rewards: { totalWei: (30n * WEI).toString(), claimableWei: (20n * WEI).toString() },
  ...over,
});

describe("Node › Rewards: Claim follows the bond's on-chain KYC flag", () => {
  it("shows the reason in place of Claim when MemberBond.isKycVerified() reads false, with the figures still shown", () => {
    const html = renderRewards(stub, bondKey({ bondKycVerified: false }));
    expect(html).not.toMatch(claimButton);
    expect(html).toContain('data-testid="claim-blocked"');
    expect(html).toContain("MemberBond.isKycVerified() reads false");
    expect(html).toContain('data-reward="claimable"');
  });

  it("a verified bond is offered Claim (control)", () => {
    const html = renderRewards(stub, bondKey());
    expect(html).toMatch(claimButton);
    expect(html).not.toContain('data-testid="claim-blocked"');
  });

  it("the Claim gate in NodeRewards is bondClaimBlocked alone (the removed wallet rule stays removed)", () => {
    const src = readFileSync("src/surfaces/node/NodeRewards.tsx", "utf8");
    expect(src).toMatch(/const kycBlock = bondClaimBlocked\(s\);/);
    expect(src).not.toMatch(/ownWallet/);
  });

  it("the dust note says 'at or above' at exactly 5 SALT", () => {
    const html = renderRewards(stub, bondKey({ rewards: { totalWei: (10n * WEI).toString(), claimableWei: (5n * WEI).toString() } }));
    expect(html).toContain("at or above the 5 SALT dust threshold");
    expect(html).not.toMatch(/is above the 5 SALT/);
  });
});
