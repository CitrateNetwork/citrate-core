// #348 review (pysol-dev; DGX B-1 at dd581c2). Contract v1.1 §1: "Bond locked · not staked" means
// MemberBond.activated() = false. An unread flag (null) is not that: rule 2 says unread is "—" with a
// reason, and no "Stake my bond" / "Activate validator" is offered on it. (The same gate #366's
// validatorState applies.)
//
// Upgrade review: a 0.5.1 save still carries the retired earnVal / earnPin / earnComp / earnToday
// keys; loading must drop them, never fold them into state.
import { describe, it, expect, vi, afterEach } from "vitest";
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

import { Node } from "./Node";
import { Wallet } from "./Wallet";
import { Dashboard } from "./Dashboard";
import { Onboarding } from "../onboarding/Onboarding";
import { store, type Store } from "../shell/store";
import { freshState, loadState, STORAGE_KEY, type AppState } from "../shell/state";
import { validatorWord } from "../shell/chainTruth";
import { bridge } from "../bridge";

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  linkWallet: () => {},
  activateValidator: async () => true,
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

const NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 546 };
const member = (over: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  signedIn: true,
  node: "synced",
  peers: 3,
  syncPct: 100,
  height: 546_000,
  hasGrant: true,
  bondedStake: 0,
  memberBond: { deployed: true, activated: null, pubkey: null, unlocked: false },
  ...over,
});

afterEach(() => vi.restoreAllMocks());

describe("#348 review — an unread MemberBond.activated() is '—', never 'Bond locked · not staked'", () => {
  it("validatorWord needs activated() = false for the bond-locked word", () => {
    expect(validatorWord(0, null, true, { deployed: true, activated: null, pubkey: null, unlocked: false })).toBeNull();
    expect(validatorWord(0, NONE, true, { deployed: true, activated: null, pubkey: null, unlocked: false })).toBeNull();
    expect(validatorWord(0, null, true, undefined)).toBeNull();
    expect(validatorWord(0, null, true, { deployed: true, activated: false, pubkey: null, unlocked: false })).toBe("Bond locked · not staked");
    expect(validatorWord(0, NONE, false, null)).toBe("Not a validator");
  });

  for (const validatorActivation of [null, NONE]) {
    const which = validatorActivation ? "status None" : "registry unread";
    it(`onboarding S6 shows '— · bond state not read yet' and no 'Stake my bond' (${which})`, () => {
      const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", validatorActivation })} />);
      // #366's validatorState names the unread part (bond state or validator key).
      expect(html).toMatch(/— · (bond state not read yet|reading validator key)/);
      expect(html).not.toContain("Bond locked · not staked");
      expect(html).not.toContain("Stake my bond");
    });

    it(`the Node page offers no activation and shows no bond-locked word (${which})`, () => {
      const html = renderToStaticMarkup(<Node store={stub} s={member({ validatorActivation })} />);
      expect(html).not.toContain("Activate validator");
      expect(html).not.toContain("Bond locked · not staked");
    });
  }

  it("Wallet and Dashboard show no bond-locked word while activated() is unread", () => {
    for (const html of [
      renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "staking" })} />),
      renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "overview" })} />),
      renderToStaticMarkup(<Dashboard store={stub} s={member()} />),
    ]) {
      expect(html).not.toMatch(/bond locked · not staked/i);
    }
  });

  it("a bond that reads not activated still gets the word and the offer", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", validatorActivation: null, memberBond: { deployed: true, activated: false, pubkey: null, unlocked: false } })} />);
    expect(html).toContain("Bond locked · not staked");
    expect(html).toContain("Stake my bond");
  });

  it("maybeAutoBond never arms or opens activation while activated() is unread", () => {
    store.setState(member({ stage: "s6", validatorActivation: null }));
    vi.spyOn(store, "walletIsLinked").mockReturnValue(true);
    const arm = vi.spyOn(bridge.node, "armMining");
    const act = vi.spyOn(store, "activateValidator");
    (store as unknown as { maybeAutoBond(): void }).maybeAutoBond();
    expect(arm).not.toHaveBeenCalled();
    expect(act).not.toHaveBeenCalled();
  });
});

describe("upgrade review — retired earn* keys are dropped when a save loads", () => {
  it("a 0.5.1 save's earnVal / earnPin / earnComp / earnToday never reach state", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", earnVal: 12.5, earnPin: 3, earnComp: 4, earnToday: 1.2, liquid: 7 }));
    const s = loadState() as unknown as Record<string, unknown>;
    for (const k of ["earnVal", "earnPin", "earnComp", "earnToday"]) expect(s[k], k).toBeUndefined();
    expect(s.liquid).toBe(7);
    localStorage.removeItem(STORAGE_KEY);
  });
});
