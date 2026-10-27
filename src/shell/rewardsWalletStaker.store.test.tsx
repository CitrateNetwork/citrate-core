// fxEARN (2026-10-27: "make sure the earnings are actually being read"). A validator
// whose ValidatorRegistry staker is the member's own wallet (registered without a MemberBond, as
// a live r1005 key is) was never read: bondedStake comes only from the bond path
// (pubkeyOfStaker(bond) → stakeOf), so it stayed 0, the rewards read was skipped ("Not a
// validator"), and the node was never re-armed after a restart. This node's key IS read from the
// registry (validatorInfo), and when its staker is the member's wallet that registry stake is the
// member's stake, so the rewards are read for this key.
//
// Fixtures are recorded live from r1005 (https://rpc.citrate.ai, chain 40204, ValidatorRegistry
// 0xde4F…3BD9) on 2026-10-27 at head ~877,400:
//   validatorInfo(0xb4a6…f748) = (staker 0x88FDCcC0…d73B, bonded 32000e18, rewards 0, …, actEpoch 343,
//                                 status 1, claimableNow 0); currentEpoch() = 877
//   rewardsOf(0x3f67…a6aa)      = 0x…073da59b5d0514461c17fe ‖ 0x…073685fa94dd70197c17fe
//                               = (8753600000639448003647486, 8719960000639448003647486) wei
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Store, ownWalletValidator, ownWalletClaimBlocked } from "./store";
import { renderRewards } from "../surfaces/node/nodePage.fixture";
import { bridge } from "../bridge";
import type { ChainReads, ValidatorActivation } from "./chainTruth";
import type { GrantStatus } from "../bridge/domains";
import type { AppState } from "./state";

const WALLET = "0x88FDCcC0D0f6A3906803F339E20026b5d8d1d73B";
const MEMBER_VALIDATOR = "0xb4a646a56e3946943f0014459e88595c45737745e0421e201ac1d3f9320ff748";
const STAKE_WEI = (32_000n * 10n ** 18n).toString();
// The live r1005 reading of this key: Active, staker = the wallet, 32,000 SALT bonded.
const LIVE_MEMBER: ValidatorActivation = { isActive: true, status: 1, actEpoch: 343, currentEpoch: 877, staker: WALLET.toLowerCase(), bondedWei: STAKE_WEI };
// rewardsOf for this key on r1005: it has produced no block on the canonical chain.
const LIVE_MEMBER_REWARDS = { totalWei: "0", claimableWei: "0" };
// rewardsOf for the producing key 0x3f67…a6aa on r1005, decoded from the raw eth_call return.
const LIVE_PRODUCER_REWARDS = { totalWei: "8753600000639448003647486", claimableWei: "8719960000639448003647486" };

// A member with no MemberBond: bondOf(wallet) has no code on r1005 (bondDeployed false).
const NO_BOND_GRANT: GrantStatus = {
  attributedStakeWei: "0",
  attributedPrincipalWei: "0",
  hasSbt: false,
  bondAddress: "0xa93e679da2056747b268884131c1dde7a175400b",
  bondDeployed: false,
  bondedStakeWei: "0",
  hasValidator: false,
  unlockBlock: null,
  isUnlocked: false,
  isKycVerified: false,
} as unknown as GrantStatus;

let store: Store;
let arm: ReturnType<typeof vi.fn>;
let earnings: ReturnType<typeof vi.fn>;

async function poll(activation: ValidatorActivation | null, rewards = LIVE_MEMBER_REWARDS): Promise<void> {
  const reads = {
    validatorActivation: vi.fn(async () => activation),
    minStakeWei: async () => 32_000n * 10n ** 18n,
    blockSeconds: async () => 2,
    producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
    validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
  } as unknown as ChainReads;
  (store as unknown as { chainReads: ChainReads }).chainReads = reads;
  (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
  (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
  (store as unknown as { validatorArmed: boolean }).validatorArmed = false;
  vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 4, height: 877_400, syncPct: 100, miningArmed: false } as never);
  vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
  earnings = vi.fn(async (k?: string) => ({ ...rewards, proposerPubkey: k ?? MEMBER_VALIDATOR }));
  vi.spyOn(bridge.node, "validatorEarnings").mockImplementation(earnings as never);
  arm = vi.fn(async () => true);
  vi.spyOn(bridge.node, "armMining").mockImplementation(arm as never);
  vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(NO_BOND_GRANT);
  await store.refreshNode();
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(() => {
  store = new Store();
  store.setState({ signedIn: true, walletAddr: WALLET, hasGrant: false, bondedStake: 0, bondActivated: null, bondPubkey: null, validatorActivation: null, rewards: null, rewardsError: null, node: "synced" });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("fxEARN — a validator staked from the member's own wallet is read", () => {
  it("the registry stake of this node's key, staked by the wallet, is the member's stake", async () => {
    await poll(LIVE_MEMBER);
    expect(store.state.validatorActivation).toEqual(LIVE_MEMBER);
    expect(store.state.bondedStake).toBe(32_000);
  });

  it("its rewards are read for this node's key: the live r1005 reading is 0 and 0, shown as figures", async () => {
    await poll(LIVE_MEMBER);
    await store.refreshEarnings();
    expect(earnings).toHaveBeenCalled();
    expect(earnings.mock.calls.at(-1)?.[0]).toBeUndefined();
    expect(store.state.rewards).toEqual(LIVE_MEMBER_REWARDS);
    expect(store.state.rewardsError).toBeNull();
  });

  it("a producing key's live figures reach state exactly as the registry returned them", async () => {
    await poll(LIVE_MEMBER, LIVE_PRODUCER_REWARDS);
    await store.refreshEarnings();
    expect(store.state.rewards).toEqual(LIVE_PRODUCER_REWARDS);
  });

  it("the node is re-armed for the key after a restart, as a bonded member's is", async () => {
    await poll(LIVE_MEMBER);
    expect(arm).toHaveBeenCalled();
  });

  it("Node › Rewards shows the figures and the active word, not 'Not a validator'", async () => {
    await poll(LIVE_MEMBER, LIVE_PRODUCER_REWARDS);
    await store.refreshEarnings();
    const html = renderRewards(store, store.state);
    expect(html).not.toContain("Not a validator");
    expect(html).toContain("8,753,600");
    expect(html).toContain("8,719,960");
  });
});

describe("fxEARN — fails closed: only the member's own registry key counts", () => {
  const cases: [string, ValidatorActivation | null][] = [
    ["another staker", { ...LIVE_MEMBER, staker: "0x6f24c86bea62e4676217f1cec762c8ebaeff2880" }],
    ["staker not read", { ...LIVE_MEMBER, staker: null }],
    ["staker absent (older read)", { isActive: true, status: 1, actEpoch: 343, currentEpoch: 877 }],
    ["key never registered (status None)", { isActive: false, status: 0, actEpoch: 0, currentEpoch: 877, staker: "0x" + "0".repeat(40), bondedWei: "0" }],
    ["registry read failed", null],
    ["bonded not a number", { ...LIVE_MEMBER, bondedWei: "32k" }],
  ];
  for (const [name, act] of cases) {
    it(name + ": no stake, no rewards read, no re-arm", async () => {
      await poll(act);
      await store.refreshEarnings();
      expect(store.state.bondedStake, name).toBe(0);
      expect(store.state.rewards, name).toBeNull();
      expect(arm, name).not.toHaveBeenCalled();
    });
  }

  it("a member with a MemberBond keeps the bond rule: the wallet-staker rule never applies", () => {
    const base = { walletAddr: WALLET, validatorActivation: LIVE_MEMBER };
    expect(ownWalletValidator({ ...base, hasGrant: true, bondActivated: true, bondPubkey: MEMBER_VALIDATOR })).toBe(false);
    expect(ownWalletValidator({ ...base, hasGrant: true, bondActivated: null, bondPubkey: null })).toBe(false);
    expect(ownWalletValidator({ ...base, hasGrant: false, bondActivated: null, bondPubkey: null })).toBe(true);
  });

  it("the wallet comparison ignores letter case but not a different address", () => {
    const s = (staker: string): Pick<AppState, "hasGrant" | "bondActivated" | "bondPubkey" | "validatorActivation" | "walletAddr"> => ({
      hasGrant: false,
      bondActivated: null,
      bondPubkey: null,
      walletAddr: WALLET,
      validatorActivation: { ...LIVE_MEMBER, staker },
    });
    expect(ownWalletValidator(s(WALLET.toUpperCase().replace("0X", "0x")))).toBe(true);
    expect(ownWalletValidator(s(WALLET.slice(0, -1) + "c"))).toBe(false);
  });
});

describe("fxEARN — Claim is offered only where the figure is what the claim pays", () => {
  // The app's claim builds MemberBond.claimRewards() (bond path) or ContributionAccounting.claim()
  // (no bond). A wallet-staked key's rewards are paid only by ValidatorRegistry.claimRewards(key)
  // sent from the wallet, which this app does not build, so a Claim here would pay a different
  // figure than "Claimable now" shows.
  it("Node › Rewards shows the reason in place of the Claim button", async () => {
    await poll(LIVE_MEMBER, LIVE_PRODUCER_REWARDS);
    await store.refreshEarnings();
    const html = renderRewards(store, store.state);
    expect(html).toContain('data-testid="claim-blocked"');
    expect(html).toContain("ValidatorRegistry.claimRewards");
    expect(html).not.toMatch(/>Claim<\/button>/);
    expect(ownWalletClaimBlocked(store.state)).toContain("ValidatorRegistry.claimRewards");
  });

  it("claimRewards stops before any claim is built", async () => {
    await poll(LIVE_MEMBER, LIVE_PRODUCER_REWARDS);
    const claim = vi.spyOn(bridge.agent, "claim");
    (store as unknown as { assertPayoutKyc: () => Promise<boolean> }).assertPayoutKyc = async () => true;
    await store.claimRewards();
    expect(claim).not.toHaveBeenCalled();
    expect(String(store.state.toast)).toContain("ValidatorRegistry.claimRewards");
  });

  it("a member without a wallet-staked key is not blocked by this rule", () => {
    expect(ownWalletClaimBlocked({ hasGrant: false, bondActivated: null, bondPubkey: null, walletAddr: WALLET, validatorActivation: null })).toBeNull();
  });
});
