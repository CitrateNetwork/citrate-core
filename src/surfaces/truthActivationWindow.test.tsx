// Owner decision 2026-10-19: activation only inside the bond lock. chain#331 (MemberBond.activate()
// reverting ActivationWindowClosed once isUnlocked()) is the genesis contract side. UI truth contract
// v1.2 §1: once the lock reads open, a never-activated bond is "Unlocked · not activated" ("Only an
// activated bond can be withdrawn."), and no copy says that activation has closed. While
// MemberBond.isUnlocked() is unread the word is "—" and nothing is offered (rule 2).
//
// Ported from #366 onto main's bond model: the bond state is `bondActivated` (+ `bondUnlocked`,
// `bondPubkey`) from the grant read, and the S7 step (#354) has its own gate on its own read.
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
import { Onboarding } from "../onboarding/Onboarding";
import { activationGate, activationPhase, validatorState } from "../onboarding/activateValidator";
import { store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { canOfferActivation, validatorDash, validatorWord, type ChainReads } from "../shell/chainTruth";
import { bridge } from "../bridge";
import type { ActivationStatusView, GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
// Registry status None for the key: never registered.
const NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 546 };

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  refreshActivation: async () => true,
  linkWallet: () => {},
  activateValidator: async () => true,
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

const member = (over: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  signedIn: true,
  node: "synced",
  peers: 3,
  syncPct: 100,
  height: 546_000,
  hasGrant: true,
  bondedStake: 0,
  bondActivated: false,
  validatorActivation: NONE,
  ...over,
});

const grant = (over: Partial<GrantStatus> = {}): GrantStatus => ({
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
  ...over,
});

// The S7 step's own read (#354), ready in every other respect.
const s7 = (over: Partial<ActivationStatusView> = {}): ActivationStatusView =>
  ({
    bondAddress: "0x" + "b".repeat(40),
    bondDeployed: true,
    bondActivated: false,
    principalWei: wei(32000),
    unlockBlock: 16_000_000,
    unlockTimestamp: 1_830_000_000,
    memberTokenId: "4",
    membershipActive: true,
    validatorPubkey: "0x" + "07".repeat(32),
    pubkeySource: "node",
    nodeKeyReady: true,
    nodePubkey: "0x" + "07".repeat(32),
    bondPubkey: null,
    registryStatus: 0,
    activationEpoch: 0,
    registryActive: false,
    epochBlocks: 1000,
    currentEpoch: 546,
    headBlock: 546_000,
    blockTimeMs: 2000,
    localBlock: 546_000,
    localSyncing: false,
    tx: null,
    recordedTx: null,
    bondUnlocked: false,
    ...over,
  }) as ActivationStatusView;

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ bondActivated: null, bondUnlocked: null, bondPubkey: null, bondedStake: 0, validatorActivation: null, hasGrant: false });
});

describe("activation only inside the lock: the words and the one offer gate", () => {
  it("an unlocked bond is not offered activation, and its word is 'Unlocked · not activated'", () => {
    expect(canOfferActivation(0, NONE, false, true)).toBe(false);
    expect(validatorWord(0, NONE, true, false, true)).toBe("Unlocked · not activated");
  });

  it("an unread lock fails closed, and the word is '—' with the reason", () => {
    for (const unlocked of [null, undefined]) {
      expect(canOfferActivation(0, NONE, false, unlocked)).toBe(false);
      expect(validatorWord(0, NONE, true, false, unlocked)).toBeNull();
      expect(validatorDash(0, NONE, true, false, unlocked)).toBe("— · bond lock not read yet");
    }
  });

  it("a bond that reads locked and not activated is still offered activation", () => {
    expect(canOfferActivation(0, NONE, false, false)).toBe(true);
    expect(validatorWord(0, NONE, true, false, false)).toBe("Bond locked · not staked");
  });

  it("Exiting / Slashed still come first, whatever the lock", () => {
    expect(validatorWord(0, { ...NONE, status: 2 }, true, false, true)).toBe("Exiting");
    expect(validatorWord(0, { ...NONE, status: 3 }, true, false, null)).toBe("Slashed");
  });
});

describe("activation only inside the lock: surfaces", () => {
  it("onboarding S6 points to no activation for an unlocked bond, says the word and the v1.2 detail", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", bondUnlocked: true })} />);
    expect(html).not.toContain("next: activate your validator");
    expect(html).toContain("Unlocked · not activated");
    expect(html).toContain("Only an activated bond can be withdrawn.");
    expect(html).not.toMatch(/activation window/i);
  });

  it("onboarding S6 still points a locked bond to the activation step", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", bondUnlocked: false })} />);
    expect(html).toContain("next: activate your validator");
  });

  it("the Node page offers no 'Activate validator' for an unlocked or unread lock, and says why", () => {
    for (const [unlocked, why] of [
      [true, "Unlocked · not activated"],
      [null, "bond lock not read yet"],
    ] as const) {
      const html = renderToStaticMarkup(<Node store={stub} s={member({ bondUnlocked: unlocked })} />);
      expect(html).not.toContain("Activate validator");
      expect(html).toContain(why);
      expect(html).not.toMatch(/activation window/i);
    }
  });

  it("the Node page still offers activation for a locked, never-activated bond", () => {
    const html = renderToStaticMarkup(<Node store={stub} s={member({ bondUnlocked: false })} />);
    expect(html).toContain("Activate validator");
  });

  it("maybeAutoBond never arms for an unlocked bond", () => {
    store.setState({ ...member({ stage: "s6", bondUnlocked: true }) });
    vi.spyOn(store, "walletIsLinked").mockReturnValue(true);
    (store as unknown as { validatorArmed: boolean }).validatorArmed = false;
    const arm = vi.spyOn(bridge.node, "armMining").mockResolvedValue(true);
    (store as unknown as { maybeAutoBond(): void }).maybeAutoBond();
    expect(arm).not.toHaveBeenCalled();
  });
});

describe("activation only inside the lock: the S7 step's gate (#354)", () => {
  it("a lock that reads open blocks the gate, with the v1.2 word and no window copy", () => {
    const g = activationGate(s7({ bondUnlocked: true }), true);
    expect(g.ready).toBe(false);
    const lock = g.checks.find((c) => c.key === "lock");
    expect(lock?.ok).toBe(false);
    expect(lock?.detail).toContain("Unlocked · not activated");
    expect(lock?.detail).not.toMatch(/activation window|closed/i);
    expect(activationPhase(s7({ bondUnlocked: true }), true)).toBe("blocked");
    expect(validatorState(s7({ bondUnlocked: true })).word).toBe("Unlocked · not activated");
  });

  it("an unread lock is not 'no': the gate stays closed and the check reads unknown", () => {
    const g = activationGate(s7({ bondUnlocked: null }), true);
    expect(g.ready).toBe(false);
    expect(g.checks.find((c) => c.key === "lock")?.ok).toBeNull();
    expect(validatorState(s7({ bondUnlocked: null })).word).toBeNull();
  });

  it("a lock that reads closed lets the gate open", () => {
    const g = activationGate(s7({ bondUnlocked: false }), true);
    expect(g.checks.find((c) => c.key === "lock")?.ok).toBe(true);
    expect(g.ready).toBe(true);
    expect(validatorState(s7({ bondUnlocked: false })).word).toBe("Bond locked · not staked");
  });

  it("an activated bond needs no lock check (it is never offered activation)", () => {
    const g = activationGate(s7({ bondActivated: true, bondUnlocked: true, registryStatus: 1 }), true);
    expect(g.checks.find((c) => c.key === "lock")).toBeUndefined();
  });
});

describe("activation only inside the lock: the grant read carries MemberBond.isUnlocked()", () => {
  async function poll(g: GrantStatus, nodeState: "running" | "stopped" = "running") {
    const reads = {
      validatorActivation: vi.fn(async () => NONE),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
      validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
    };
    (store as unknown as { chainReads: ChainReads }).chainReads = reads as unknown as ChainReads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: nodeState, peers: 3, height: 546_000, syncPct: 100, miningArmed: true } as never);
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: "0x" + "07".repeat(32) });
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(g);
    await store.refreshNode();
    return gs;
  }

  it("folds bondUnlocked as read; absent from an older backend is unread", async () => {
    store.setState({ node: "synced", stage: "done", hasGrant: false, bondedStake: 0, bondActivated: null, bondUnlocked: null });
    await poll(grant({ bondUnlocked: true }));
    expect(store.state.bondUnlocked).toBe(true);
    store.setState({ hasGrant: false });
    await poll(grant({ bondUnlocked: false }));
    expect(store.state.bondUnlocked).toBe(false);
    store.setState({ hasGrant: false });
    await poll(grant());
    expect(store.state.bondUnlocked).toBeNull();
  });

  it("an unread lock on a never-activated bond is read again on the next due poll, node up or not", async () => {
    store.setState({ node: "off", stage: "done", hasGrant: true, bondedStake: 0, bondActivated: false, bondUnlocked: null });
    const gs = await poll(grant({ bondUnlocked: false }), "stopped");
    expect(gs).toHaveBeenCalledTimes(1);
    expect(store.state.bondUnlocked).toBe(false);
  });
});
