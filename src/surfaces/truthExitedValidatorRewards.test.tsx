// #348 review (Codex). UI truth contract v1.1 §1 Rewards: "Validator rewards" / "Claimable now" are
// ValidatorRegistry.rewardsOf(pubkey).
//
// After a full exit (or a slash) the registry zeroes the stake and clears pubkeyOfStaker, so
// stakeOf and hasValidator both read 0 / false. MemberBond.activated() stays true, and the bond can
// still claim that key's matured rewards (MemberBond.claimRewards checks only `activated`;
// ValidatorRegistry.claimRewards has no status check). So the rewards are read, shown and claimed
// through the bond for an activated bond, whatever the stake.
//
// Also: S6 never offers "Stake my bond" for a registered stake, and the Wallet reads the rewards it
// shows.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
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
import { Onboarding } from "../onboarding/Onboarding";
import { store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import type { ChainReads } from "../shell/chainTruth";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();

function stubStore(over: Record<string, unknown> = {}): Store {
  return {
    walletIsLinked: () => true,
    refreshEarnings: vi.fn(async () => {}),
    refreshWallet: () => {},
    refreshPendingWithdrawals: () => {},
    refreshActivity: () => {},
    refreshChainParams: () => {},
    refreshProducers: () => {},
    activateValidator: vi.fn(async () => true),
    toast: () => {},
    identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
    ...over,
  } as unknown as Store;
}

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", peers: 3, syncPct: 100, height: 546_000, hasGrant: true, s5StakeWei: wei(32000), ...over };
}

// Registered this epoch, active from the next (contract v1.1 §1).
const PENDING = { isActive: true, status: 1, actEpoch: 547, currentEpoch: 546 };

// The member's bond as the grant read reports it (state.memberBond); `activated` outlives the exit.
// `unlocked: false`: still inside the lock, so activation is open (chain#331, MemberBond.sol:219).
const bond = (activated: boolean | null) => ({ deployed: true, activated, pubkey: null, unlocked: false });

const exitedGrant: GrantStatus = {
  attributedStakeWei: wei(32000),
  attributedPrincipalWei: wei(32000),
  hasSbt: true,
  bondAddress: "0x" + "b".repeat(40),
  bondDeployed: true,
  bondedStakeWei: "0",
  hasValidator: false,
  bondActivated: true,
  unlockBlock: 16_000_000,
  isUnlocked: true,
  isKycVerified: true,
};

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ bondedStake: 0, memberBond: null, rewards: null });
});

describe("#348 review — an exited validator's rewards are read, shown and claimed through the bond", () => {
  it("refreshEarnings reads rewardsOf for an activated bond whose registry stake is 0", async () => {
    // #391: the read is shown only for the bond's own validator key (MemberBond.pubkey()).
    store.setState({ bondedStake: 0, memberBond: { ...bond(true), pubkey: "0x" + "22".repeat(32) }, rewards: null });
    const read = vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: wei(30), claimableWei: wei(20), proposerPubkey: "0x" + "22".repeat(32) });
    await store.refreshEarnings();
    expect(read).toHaveBeenCalled();
    expect(store.state.rewards).toEqual({ totalWei: wei(30), claimableWei: wei(20) });
  });

  it("refreshEarnings reads nothing for a bond that was never activated, or an unread one, with stake 0", async () => {
    const read = vi.spyOn(bridge.node, "validatorEarnings");
    for (const memberBond of [bond(false), bond(null), null]) {
      store.setState({ bondedStake: 0, memberBond, rewards: { totalWei: "1", claimableWei: "1" } });
      await store.refreshEarnings();
      expect(store.state.rewards).toBeNull();
    }
    expect(read).not.toHaveBeenCalled();
  });

  it("the poll folds MemberBond.activated() from the grant read", async () => {
    const reads = {
      validatorActivation: vi.fn(async () => ({ isActive: false, status: 0, actEpoch: 0, currentEpoch: 546 })),
      validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
    } as unknown as ChainReads;
    (store as unknown as { chainReads: ChainReads }).chainReads = reads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    store.setState({ node: "synced", hasGrant: true, bondedStake: 0, memberBond: null });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 546_000, syncPct: 100, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(exitedGrant);
    await store.refreshNode();
    expect(store.state.bondedStake).toBe(0);
    expect(store.state.memberBond?.activated).toBe(true);
  });

  it("claimRewards takes the bond (validator) path for an activated bond with no registry binding", async () => {
    vi.spyOn(store as never, "assertPayoutKyc").mockResolvedValue(true as never);
    // #394: the pre-check reads rewardsOf for the bond's own key, the key MemberBond claims for.
    const bondKey = "0x" + "22".repeat(32);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue({ ...exitedGrant, bondPubkey: bondKey });
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(claim).toHaveBeenCalledWith({ validator: true, pubkey: bondKey });
  });

  it("claimRewards keeps the non-validator path for a bond that was never activated, or an unread flag", async () => {
    vi.spyOn(store as never, "assertPayoutKyc").mockResolvedValue(true as never);
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    const gs = vi.spyOn(bridge.membership, "grantStatus");
    for (const bondActivated of [false, null, undefined]) {
      gs.mockResolvedValueOnce({ ...exitedGrant, bondActivated });
      await store.claimRewards();
      expect(claim).toHaveBeenLastCalledWith({ validator: false });
    }
  });
});

describe("#348 review — S6 never offers 'Stake my bond' for a registered stake", () => {
  it("registered stake not in the active set yet shows the §1 word and no 'Stake my bond'", () => {
    const html = renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", node: "synced", bondedStake: 32_000, validatorActivation: PENDING })} />);
    expect(html).toContain("Registered · active from epoch 547");
    expect(html).not.toContain("Bond locked · not staked");
    expect(html).not.toContain("Stake my bond");
  });

  it("registered stake with the registry unread shows '—' and no 'Stake my bond'", () => {
    const html = renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", node: "synced", bondedStake: 32_000, validatorActivation: null })} />);
    expect(html).toContain("—");
    expect(html).not.toContain("Bond locked · not staked");
    expect(html).not.toContain("Stake my bond");
  });

  it("a granted bond with no registered stake still gets 'Stake my bond'", () => {
    const html = renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", node: "synced", bondedStake: 0, validatorActivation: null, memberBond: bond(false) })} />);
    expect(html).toContain("Bond locked · not staked");
    expect(html).toContain("Stake my bond");
  });
});

describe("#348 review — the Wallet reads the validator rewards it shows", () => {
  let root: Root | null = null;
  afterEach(() => {
    act(() => root?.unmount());
    root = null;
  });

  it("opening the Wallet reads rewardsOf, and reads again once the stake reconciles after mount", async () => {
    const s0 = stubStore();
    const host = document.createElement("div");
    root = createRoot(host);
    act(() => root?.render(<Wallet store={s0} s={member({ wTab: "staking", bondedStake: 0, memberBond: null })} />));
    const calls0 = (s0.refreshEarnings as unknown as ReturnType<typeof vi.fn>).mock.calls.length;
    expect(calls0).toBeGreaterThanOrEqual(1);
    act(() => root?.render(<Wallet store={s0} s={member({ wTab: "staking", bondedStake: 32_000, memberBond: bond(true) })} />));
    expect((s0.refreshEarnings as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThan(calls0);
  });
});

// #348 review, Codex round 2: the exited bond's activated flag must load whatever the node is
// doing and whether or not onboarding is settled, and the Rewards tab must read again once it does.
describe("#348 round 2 — MemberBond.activated() loads after a restart, and Rewards follows it", () => {
  let root: Root | null = null;
  afterEach(() => {
    act(() => root?.unmount());
    root = null;
  });

  it("the startup reconcile folds activated() even when onboarding is already settled", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(exitedGrant);
    store.setState({ hasGrant: true, s3: "settled", s5: "settled", bondedStake: 0, memberBond: null });
    await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
    expect(store.state.memberBond?.activated).toBe(true);
  });

  it("a failed startup read leaves activated() unread, never false", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue({ ...exitedGrant, bondActivated: null });
    store.setState({ hasGrant: true, s3: "settled", s5: "settled", bondedStake: 0, memberBond: null });
    await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
    expect(store.state.memberBond?.activated).toBeNull();
  });

  it("with the node stopped and the grant restored from disk, the poll reads an unread activated() flag", async () => {
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 0, memberBond: null });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(exitedGrant);
    await store.refreshNode();
    expect(gs).toHaveBeenCalled();
    expect(store.state.memberBond?.activated).toBe(true);
    await store.refreshEarnings();
  });

  // #366 Codex P2: activated() failed but pubkey() read, so the bond state has a key and an unread
  // flag. That flag is still unread: the poll must retry it (node stopped too), and once it reads
  // true the exited member's rewards are read again.
  it("an unread activated() flag is retried even when the bond key read", async () => {
    const KEY = "0x" + "33".repeat(32);
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 0, rewards: null, memberBond: { deployed: true, activated: null, pubkey: KEY } });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue({ ...exitedGrant, bondPubkey: KEY });
    await store.refreshNode();
    expect(gs).toHaveBeenCalled();
    expect(store.state.memberBond?.activated).toBe(true);
    const read = vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: wei(30), claimableWei: wei(20), proposerPubkey: KEY });
    await store.refreshEarnings();
    expect(read).toHaveBeenCalled();
    expect(store.state.rewards).toEqual({ totalWei: wei(30), claimableWei: wei(20) });
  });

  it("once activated() has read, a stopped node with a settled grant does not re-read it every poll", async () => {
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 0, memberBond: bond(false) });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(exitedGrant);
    await store.refreshNode();
    expect(gs).not.toHaveBeenCalled();
  });
});
