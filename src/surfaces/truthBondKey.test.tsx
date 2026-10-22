// C1-V4 / C1-V5 — UI truth contract v1.1 §1 Validators (chain#326).
//
//   C1-V4 With validatorInfo.status UNREAD the app must not fall back to stake words: an exiting
//         member (stake 0, status not read yet) was offered "Stake my bond". Unread is "—" plus the
//         reason, and no stake or activate action is offered.
//         "Bond locked · not staked" needs MemberBond.activated() = false; "Not a validator" needs
//         status None. Status None with a registered stake (or an activated bond) is
//         "— · registry state inconsistent".
//   C1-V5 The validator key is MemberBond.pubkey() where the bond defines it (it survives an exit,
//         unlike pubkeyOfStaker); the node's proposer key is used only when no bond exists.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: 6n }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Node } from "./Node";
import { Onboarding } from "../onboarding/Onboarding";
import { canOfferActivation, validatorDash, validatorWord, type ChainReads, type MemberBondState, type ValidatorActivation } from "../shell/chainTruth";
import { store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
const NODE_KEY = ("0x" + "11".repeat(32)) as `0x${string}`;
const BOND_KEY = ("0x" + "33".repeat(32)) as `0x${string}`;

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  linkWallet: () => {},
  activateValidator: () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", peers: 3, syncPct: 100, height: 499_058, hasGrant: true, s5StakeWei: wei(32000), ...over };
}

const none: ValidatorActivation = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 511 };
const active: ValidatorActivation = { isActive: true, status: 1, actEpoch: 480, currentEpoch: 511 };
const notActivated: MemberBondState = { deployed: true, activated: false, pubkey: null };
const activatedBond: MemberBondState = { deployed: true, activated: true, pubkey: BOND_KEY };
const bondUnread: MemberBondState = { deployed: true, activated: null, pubkey: null };
const noBond: MemberBondState = { deployed: false, activated: null, pubkey: null };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("C1-V4 — an unread status is '—' with the reason, never a stake word", () => {
  it("status unread with an activated bond (e.g. an exiting member) is unread, not 'Bond locked'", () => {
    expect(validatorWord(0, null, true, activatedBond)).toBeNull();
    expect(validatorDash(0, null, true, activatedBond)).toBe("— · registry not read yet");
    expect(canOfferActivation(0, null, true, activatedBond)).toBe(false);
  });

  it("status unread and bond state unread is unread", () => {
    expect(validatorWord(0, null, true, bondUnread)).toBeNull();
    expect(validatorWord(0, null, true, null)).toBeNull();
    expect(validatorWord(0, null, false, null)).toBeNull();
    expect(canOfferActivation(0, null, true, null)).toBe(false);
  });

  it("'Bond locked · not staked' comes from MemberBond.activated() = false", () => {
    expect(validatorWord(0, null, true, notActivated)).toBe("Bond locked · not staked");
    expect(validatorWord(0, none, true, notActivated)).toBe("Bond locked · not staked");
    expect(canOfferActivation(0, none, true, notActivated)).toBe(true);
  });

  it("'Not a validator' needs status None and no grant", () => {
    expect(validatorWord(0, none, false, noBond)).toBe("Not a validator");
    expect(canOfferActivation(0, none, false, noBond)).toBe(false);
  });

  it("status None with a registered stake or an activated bond is 'registry state inconsistent'", () => {
    expect(validatorWord(32_000, none, true, activatedBond)).toBeNull();
    expect(validatorDash(32_000, none, true, activatedBond)).toBe("— · registry state inconsistent");
    expect(validatorWord(0, none, true, activatedBond)).toBeNull();
    expect(validatorDash(0, none, true, activatedBond)).toBe("— · registry state inconsistent");
  });

  it("status None with the bond state unread says which read is missing", () => {
    expect(validatorDash(0, none, true, bondUnread)).toBe("— · bond state not read yet");
  });

  it("onboarding S6 offers no 'Stake my bond' while the status is unread", () => {
    const html = renderToStaticMarkup(
      <Onboarding store={stub} s={member({ stage: "s6", node: "synced", bondedStake: 0, validatorActivation: null, memberBond: activatedBond })} />,
    );
    expect(html).not.toContain("Stake my bond");
    expect(html).not.toContain("Bond locked · not staked");
    expect(html).toContain("— · registry not read yet");
  });

  it("onboarding S6 offers 'Stake my bond' only for a bond that is not activated", () => {
    const html = renderToStaticMarkup(
      <Onboarding store={stub} s={member({ stage: "s6", node: "synced", bondedStake: 0, validatorActivation: none, memberBond: notActivated })} />,
    );
    expect(html).toContain("Bond locked · not staked");
    expect(html).toContain("Stake my bond");
  });

  it("the Node page offers no activation while the status is unread", () => {
    const html = renderToStaticMarkup(
      <Node store={stub} s={member({ bondedStake: 0, validatorActivation: null, memberBond: activatedBond })} />,
    );
    expect(html).not.toContain("Activate validator");
    expect(html).toContain("— · registry not read yet");
  });

  it("the Node page offers activation for a bond that is not activated", () => {
    const html = renderToStaticMarkup(
      <Node store={stub} s={member({ bondedStake: 0, validatorActivation: none, memberBond: notActivated })} />,
    );
    expect(html).toContain("Activate validator");
  });

  it("onboarding never auto-opens activation while the status is unread", async () => {
    const open = vi.spyOn(store, "activateValidator").mockResolvedValue(true);
    vi.spyOn(store, "walletIsLinked").mockReturnValue(true);
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    (store as unknown as { autoBondAttempted: boolean }).autoBondAttempted = false;
    store.setState({ stage: "s6", hasGrant: true, bondedStake: 0, walletReview: null, validatorActivation: null, memberBond: activatedBond });
    (store as unknown as { maybeAutoBond: () => void }).maybeAutoBond();
    expect(open).not.toHaveBeenCalled();
    store.setState({ validatorActivation: none, memberBond: notActivated });
    (store as unknown as { maybeAutoBond: () => void }).maybeAutoBond();
    expect(open).toHaveBeenCalledTimes(1);
  });
});

describe("C1-V5 — the validator key comes from MemberBond.pubkey() where the bond defines it", () => {
  function grant(over: Partial<GrantStatus>): GrantStatus {
    return {
      attributedStakeWei: wei(32000),
      attributedPrincipalWei: wei(32000),
      hasSbt: true,
      bondAddress: "0x" + "b".repeat(40),
      bondDeployed: true,
      bondedStakeWei: "0",
      hasValidator: false,
      unlockBlock: 16_000_000,
      isUnlocked: false,
      isKycVerified: false,
      bondActivated: true,
      bondPubkey: BOND_KEY,
      ...over,
    };
  }

  async function poll(g: GrantStatus) {
    const reads = {
      validatorActivation: vi.fn(async () => none),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
      validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
    };
    (store as unknown as { chainReads: ChainReads }).chainReads = reads as unknown as ChainReads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    (store as unknown as { validatorPubkey: string | null }).validatorPubkey = null;
    store.setState({ node: "synced", stage: "done", hasGrant: false, bondedStake: 0, selfStake: 0, validatorActivation: null, memberBond: null });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 499_058, syncPct: 100, miningArmed: true });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: NODE_KEY });
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(g);
    await store.refreshNode();
    return reads;
  }

  it("reads the registry for the bond's key, even when the node holds another key", async () => {
    const reads = await poll(grant({}));
    expect(reads.validatorActivation).toHaveBeenCalledWith(BOND_KEY);
    expect(reads.validatorActivation).not.toHaveBeenCalledWith(NODE_KEY);
    expect(store.state.memberBond).toEqual({ deployed: true, activated: true, pubkey: BOND_KEY });
  });

  it("falls back to the node's key only when no bond exists", async () => {
    const reads = await poll(grant({ bondDeployed: false, bondActivated: null, bondPubkey: null, hasSbt: false, attributedStakeWei: "0", attributedPrincipalWei: "0" }));
    expect(reads.validatorActivation).toHaveBeenCalledWith(NODE_KEY);
  });

  it("a bond that is not activated has no key yet: no registry read, and the word is 'Bond locked · not staked'", async () => {
    const reads = await poll(grant({ bondActivated: false, bondPubkey: null }));
    expect(reads.validatorActivation).not.toHaveBeenCalled();
    expect(validatorWord(store.state.bondedStake, store.state.validatorActivation, store.state.hasGrant, store.state.memberBond)).toBe(
      "Bond locked · not staked",
    );
  });

  it("an activated bond whose key could not be read is unread, never the node's key", async () => {
    const reads = await poll(grant({ bondActivated: true, bondPubkey: null }));
    expect(reads.validatorActivation).not.toHaveBeenCalled();
    expect(store.state.validatorActivation).toBeNull();
  });

  it("an activated validator still reads 'Validator active' through the bond's key", () => {
    expect(validatorWord(32_000, active, true, activatedBond)).toBe("Validator active");
  });
});
