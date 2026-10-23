// #349 review (pysol-dev; DGX B-1 at bdce517). The bond words key on MemberBond.activated(), not on
// hasValidator (pubkeyOfStaker, which an exit or a slash clears):
//  - Locked: "Bond locked · not staked" only for activated() = false; an activated bond that exited,
//    or an unread flag, is the plain §1 lock word "Locked until block N" (#411's fix, ported).
//  - Unlocked: a never-activated bond is "Unlocked · not activated" (contract v1.2): requestRelease /
//    withdrawToMember revert NotActivated() for it, so no identity / withdraw wording. Unread is
//    "Unlocked · bond state not read yet".
//  - The grant copy never tells an exited member (activated, no stake) to "activate the bond".
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Wallet } from "./Wallet";
import { Node } from "./Node";
import { describeBond, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { grantCustody } from "../data/grantCopy";

const LOCKED = { bondDeployed: true, unlockBlock: 300_000, isUnlocked: false, isKycVerified: false, hasValidator: false };
const UNLOCKED = { ...LOCKED, isUnlocked: true };

describe("#349 review — describeBond keys on MemberBond.activated()", () => {
  it("locked: an activated bond that exited, or an unread flag, is the plain lock word", () => {
    expect(describeBond({ ...LOCKED, bondActivated: true })).toBe("Locked until block 300,000");
    expect(describeBond({ ...LOCKED, bondActivated: null })).toBe("Locked until block 300,000");
    expect(describeBond({ ...LOCKED, bondActivated: false })).toBe("Bond locked · not staked · until block 300,000");
    expect(describeBond({ ...LOCKED, hasValidator: true, bondActivated: true })).toBe("Staked · locked until block 300,000");
  });

  it("unlocked: a never-activated bond is 'Unlocked · not activated', with no withdraw wording", () => {
    for (const isKycVerified of [false, true]) {
      const w = describeBond({ ...UNLOCKED, isKycVerified, bondActivated: false });
      expect(w).toBe("Unlocked · not activated");
      expect(w).not.toMatch(/withdraw|verify identity/);
    }
  });

  it("unlocked: an activated bond keeps the identity / withdraw words; unread says so", () => {
    expect(describeBond({ ...UNLOCKED, bondActivated: true })).toBe("Unlocked · verify identity to withdraw");
    expect(describeBond({ ...UNLOCKED, bondActivated: true, isKycVerified: true })).toBe("Unlocked");
    expect(describeBond({ ...UNLOCKED, bondActivated: null })).toBe("Unlocked · bond state not read yet");
  });
});

describe("#349 review — the grant copy for an exited member", () => {
  it("an activated bond with no stake is never told to activate it", () => {
    const t = grantCustody({ amountSalt: "32,000", unlockBlock: 300_000, staked: false, granted: true, activated: true });
    expect(t).not.toMatch(/until you activate/);
    expect(t).toContain("Your bond was activated and is no longer staked in the ValidatorRegistry.");
  });

  it("an unread activated flag is not 'not staked until you activate' either", () => {
    const t = grantCustody({ amountSalt: "32,000", unlockBlock: 300_000, staked: false, granted: true, activated: null });
    expect(t).not.toMatch(/until you activate/);
    expect(t).toContain("Whether your bond has been activated has not been read yet.");
  });

  it("a bond that reads not activated keeps the activate sentence", () => {
    expect(grantCustody({ amountSalt: "32,000", unlockBlock: 300_000, staked: false, granted: true, activated: false })).toContain("It is not staked until you activate the bond.");
  });

  it("Wallet and Node pass the bond's activated flag: an exited member is not told to activate", () => {
    const stub = { walletIsLinked: () => true, refreshEarnings: () => {}, refreshWallet: () => {}, refreshPendingWithdrawals: () => {}, refreshActivity: () => {}, refreshChainParams: () => {}, refreshProducers: () => {}, toast: () => {}, linkWallet: () => {}, identity: () => ({ sub: "s", wallet: "0x" + "0".repeat(40) }) } as unknown as Store;
    const s: AppState = { ...freshState("p1"), signedIn: true, node: "synced", syncPct: 100, hasGrant: true, s5StakeWei: (32_000n * 10n ** 18n).toString(), s5UnlockBlock: 300_000, bondedStake: 0, bondActivated: true };
    for (const html of [
      renderToStaticMarkup(<Wallet store={stub} s={{ ...s, wTab: "staking" }} />),
      renderToStaticMarkup(<Node store={stub} s={{ ...s, nTab: "ops" }} />),
    ]) {
      expect(html).not.toMatch(/until you activate the bond/);
    }
  });
});
