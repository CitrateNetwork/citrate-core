// Owner decision 2026-10-19: activation only inside the bond lock. chain#331 (the contract side,
// activate() reverting once isUnlocked()) is not deployed, so UI truth contract v1.2 §1 (chain#354)
// allows no copy saying activation closes. The app's one activation gate (canOfferActivation:
// onboarding S6 "Stake my bond", Node "Activate validator", maybeAutoBond) offers activation only
// while the bond reads as LOCKED. Once it reads unlocked the word is "Unlocked · not activated"
// ("Only an activated bond can be withdrawn."), and while the flag is unread the word is "—".
// The test doubles below stand in for MemberBond.isUnlocked() (grant_status reads it).
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";

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

import { Node } from "./Node";
import { Onboarding } from "../onboarding/Onboarding";
import { store, memberBondFromGrant, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { canOfferActivation, validatorDash, validatorWord, type MemberBondState } from "../shell/chainTruth";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
// Registry status None for the member's key: never registered.
const NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 546 };
// MemberBond doubles. `unlocked` is MemberBond.isUnlocked().
const LOCKED: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: false };
const UNLOCKED: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: true };
const UNREAD: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: null };
const ABSENT: MemberBondState = { deployed: true, activated: false, pubkey: null };

function stubStore(over: Record<string, unknown> = {}): Store {
  return {
    walletIsLinked: () => true,
    refreshEarnings: async () => {},
    refreshWallet: () => {},
    refreshPendingWithdrawals: () => {},
    refreshActivity: () => {},
    refreshChainParams: () => {},
    refreshProducers: () => {},
    activateValidator: vi.fn(async () => true),
    linkWallet: () => {},
    toast: () => {},
    identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
    ...over,
  } as unknown as Store;
}

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", peers: 3, syncPct: 100, height: 546_000, hasGrant: true, s5StakeWei: wei(32000), bondedStake: 0, validatorActivation: NONE, ...over };
}

const grant: GrantStatus = {
  attributedStakeWei: wei(32000),
  attributedPrincipalWei: wei(32000),
  hasSbt: true,
  bondAddress: "0x" + "b".repeat(40),
  bondDeployed: true,
  bondedStakeWei: "0",
  hasValidator: false,
  bondActivated: false,
  unlockBlock: 16_000_000,
  isUnlocked: true,
  isKycVerified: true,
};

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ memberBond: null, bondedStake: 0, validatorActivation: null });
});

describe("activation only inside the lock (owner decision 2026-10-19; v1.2 §1 words)", () => {
  it("an unlocked bond is not offered activation, and its word is 'Unlocked · not activated'", () => {
    expect(canOfferActivation(0, NONE, true, UNLOCKED)).toBe(false);
    expect(validatorWord(0, NONE, true, UNLOCKED)).toBe("Unlocked · not activated");
  });

  it("an unread unlock flag fails closed, and the word is '—' with the reason", () => {
    for (const b of [UNREAD, ABSENT]) {
      expect(canOfferActivation(0, NONE, true, b)).toBe(false);
      expect(validatorDash(0, NONE, true, b)).toBe("— · bond lock not read yet");
    }
  });

  it("a bond that reads locked and not activated is still offered activation", () => {
    expect(canOfferActivation(0, NONE, true, LOCKED)).toBe(true);
    expect(validatorWord(0, NONE, true, LOCKED)).toBe("Bond locked · not staked");
  });

  it("the grant read carries MemberBond.isUnlocked() into the bond state; absent is unread", () => {
    expect(memberBondFromGrant({ ...grant, bondUnlocked: true }).unlocked).toBe(true);
    expect(memberBondFromGrant({ ...grant, bondUnlocked: false }).unlocked).toBe(false);
    expect(memberBondFromGrant(grant).unlocked ?? null).toBeNull();
  });

  it("onboarding S6 shows no 'Stake my bond' for an unlocked bond, says the word, and no window copy", () => {
    const html = renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", memberBond: UNLOCKED })} />);
    expect(html).not.toContain("Stake my bond");
    expect(html).toContain("Unlocked · not activated");
    expect(html).not.toMatch(/activation window/i);
  });

  it("the Node page shows no 'Activate validator' for an unlocked or unread bond, and says why", () => {
    for (const [b, why] of [[UNLOCKED, "Unlocked · not activated"], [UNREAD, "bond lock not read yet"]] as const) {
      const html = renderToStaticMarkup(<Node store={stubStore()} s={member({ memberBond: b })} />);
      expect(html).not.toContain("Activate validator");
      expect(html).toContain(why);
      expect(html).not.toMatch(/activation window/i);
    }
  });

  it("an unlocked, never-activated bond carries the v1.2 detail on S6 and the Node page", () => {
    for (const html of [
      renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", memberBond: UNLOCKED })} />),
      renderToStaticMarkup(<Node store={stubStore()} s={member({ memberBond: UNLOCKED })} />),
    ]) {
      expect(html).toContain("Only an activated bond can be withdrawn.");
    }
  });

  it("activateValidator itself refuses an unlocked bond: no arming, no ceremony", async () => {
    store.setState({ ...member({ memberBond: UNLOCKED }) });
    const arm = vi.spyOn(bridge.node, "armMining");
    const reg = vi.spyOn(bridge.node, "registerValidator");
    expect(await store.activateValidator()).toBe(false);
    expect(arm).not.toHaveBeenCalled();
    expect(reg).not.toHaveBeenCalled();
    expect(String(store.getSnapshot().toast)).toContain("Unlocked · not activated");
  });

  it("maybeAutoBond never arms or opens activation for an unlocked bond", () => {
    store.setState({ ...member({ stage: "s6", memberBond: UNLOCKED }) });
    vi.spyOn(store, "walletIsLinked").mockReturnValue(true);
    const arm = vi.spyOn(bridge.node, "armMining");
    const act = vi.spyOn(store, "activateValidator");
    (store as unknown as { maybeAutoBond(): void }).maybeAutoBond();
    expect(arm).not.toHaveBeenCalled();
    expect(act).not.toHaveBeenCalled();
  });
});

// Tripwire: the activation ceremony is built only by store.activateValidator, and that method checks
// canOfferActivation before anything else, so no entry point can bypass the gate.
describe("tripwire — every activation goes through canOfferActivation", () => {
  it("bridge.node.registerValidator is called only from activateValidator, after the gate", () => {
    const store = readFileSync("src/shell/store.ts", "utf8");
    const calls = store.split("bridge.node.registerValidator(").length - 1;
    expect(calls).toBe(1);
    const start = store.indexOf("async activateValidator(): Promise<boolean> {");
    expect(start).toBeGreaterThan(0);
    const body = store.slice(start, store.indexOf("bridge.node.registerValidator(", start));
    expect(body).toContain("canOfferActivation(");
    expect(body.indexOf("canOfferActivation(")).toBeLessThan(body.indexOf("armMining("));
    for (const f of ["src/surfaces/Node.tsx", "src/onboarding/Onboarding.tsx", "src/agent/harness.ts"]) {
      expect(readFileSync(f, "utf8"), f).not.toContain("registerValidator(");
    }
  });
});

// #366: an unread isUnlocked() keeps the activation gate closed, so it
// is retried like the activated flag: while unread, at most every BOND_READ_MS, node up or not.
describe("an unread isUnlocked() is retried", () => {
  const poll = async (memberBond: MemberBondState | null, answer: GrantStatus) => {
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 0, memberBond });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(answer);
    await store.refreshNode();
    return gs;
  };

  it("with the node stopped, an un-activated bond whose lock is unread is read again", async () => {
    const gs = await poll(UNREAD, { ...grant, isUnlocked: false, bondUnlocked: false });
    expect(gs).toHaveBeenCalled();
    expect(store.state.memberBond?.unlocked).toBe(false);
    expect(canOfferActivation(0, NONE, true, store.state.memberBond)).toBe(true);
  });

  it("an absent flag (older backend) is unread too, and is read again", async () => {
    const gs = await poll(ABSENT, { ...grant, isUnlocked: true, bondUnlocked: true });
    expect(gs).toHaveBeenCalled();
    expect(validatorWord(0, NONE, true, store.state.memberBond)).toBe("Unlocked · not activated");
  });

  it("once the lock has read, it is not re-read every poll", async () => {
    for (const b of [LOCKED, UNLOCKED]) {
      const gs = await poll(b, grant);
      expect(gs).not.toHaveBeenCalled();
      vi.restoreAllMocks();
    }
  });

  it("the retry is throttled to BOND_READ_MS", async () => {
    await poll(UNREAD, { ...grant, bondUnlocked: null });
    vi.restoreAllMocks();
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const again = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant);
    await store.refreshNode(); // immediately after: not due yet
    expect(again).not.toHaveBeenCalled();
  });
});
