// C1-V2(a) — UI truth contract v1.1 §1 Validators: "Exiting" / "Slashed" are registry status 2 / 3
// (`validatorInfo(pubkey).status`). On a full exit the registry moves the bonded stake to escrow, so
// `stakeOf` reads 0 while the status is Exiting. The status is therefore read and branched on
// BEFORE any stake check: a member with stake 0 and status Exiting/Slashed sees that word, never
// "Not a validator" or "Bond locked · not staked".
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
import { validatorWord, type ChainReads, type ValidatorActivation } from "../shell/chainTruth";
import { store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
const KEY = ("0x" + "22".repeat(32)) as `0x${string}`;

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", peers: 3, syncPct: 100, height: 499_058, hasGrant: true, s5StakeWei: wei(32000), ...over };
}

// What the registry returns after a full exit / a slash: isActive false, the stake gone.
const exiting: ValidatorActivation = { isActive: false, status: 2, actEpoch: 480, currentEpoch: 511 };
const slashed: ValidatorActivation = { isActive: false, status: 3, actEpoch: 480, currentEpoch: 511 };
const none: ValidatorActivation = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 511 };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("C1-V2(a) — the word follows validatorInfo.status before any stake check", () => {
  it("stake 0 with status Exiting reads 'Exiting', with or without a grant", () => {
    expect(validatorWord(0, exiting, true)).toBe("Exiting");
    expect(validatorWord(0, exiting, false)).toBe("Exiting");
  });

  it("stake 0 with status Slashed reads 'Slashed', with or without a grant", () => {
    expect(validatorWord(0, slashed, true)).toBe("Slashed");
    expect(validatorWord(0, slashed, false)).toBe("Slashed");
  });

  it("stake 0 with status None keeps the bond / not-a-validator words; unread is unread (C1-V4)", () => {
    expect(validatorWord(0, none, true, { deployed: true, activated: false, pubkey: null, unlocked: false })).toBe("Bond locked · not staked");
    expect(validatorWord(0, none, false, { deployed: false, activated: null, pubkey: null })).toBe("Not a validator");
    expect(validatorWord(0, null, true)).toBeNull();
    expect(validatorWord(0, null, false)).toBeNull();
  });

  it("the poll reads validatorInfo for a known key even when the registry stake is 0", async () => {
    const reads = {
      validatorActivation: vi.fn(async () => exiting),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
      validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
    } as unknown as ChainReads;
    (store as unknown as { chainReads: ChainReads }).chainReads = reads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    (store as unknown as { validatorPubkey: string | null }).validatorPubkey = null;
    store.setState({ node: "synced", hasGrant: false, bondedStake: 0, selfStake: 0, validatorActivation: null });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 511_000, syncPct: 100, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: KEY });
    const g: GrantStatus = {
      attributedStakeWei: wei(32000),
      attributedPrincipalWei: wei(32000),
      hasSbt: true,
      bondAddress: "0x" + "b".repeat(40),
      bondDeployed: true,
      bondedStakeWei: "0",
      hasValidator: true,
      unlockBlock: 16_000_000,
      isUnlocked: false,
      isKycVerified: false,
      // C1-V5: after the exit the bond keeps its pubkey (pubkeyOfStaker is cleared).
      bondActivated: true,
      bondPubkey: KEY,
    };
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(g);
    await store.refreshNode();
    expect((reads as unknown as { validatorActivation: ReturnType<typeof vi.fn> }).validatorActivation).toHaveBeenCalledWith(KEY);
    expect(store.state.bondedStake).toBe(0);
    expect(store.state.validatorActivation).toEqual(exiting);
    expect(validatorWord(store.state.bondedStake, store.state.validatorActivation, store.state.hasGrant)).toBe("Exiting");
  });

  it("Node shows 'Exiting' / 'Slashed' for a member with stake 0, and offers no activation", () => {
    for (const [a, word] of [[exiting, "Exiting"], [slashed, "Slashed"]] as const) {
      const html = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 0, validatorActivation: a })} />);
      expect(html).toContain(word);
      expect(html).not.toContain("Bond locked · not staked");
      expect(html).not.toContain("Activate validator");
    }
  });

  it("onboarding S6 shows 'Exiting' / 'Slashed' for a synced member with stake 0", () => {
    for (const [a, word] of [[exiting, "Exiting"], [slashed, "Slashed"]] as const) {
      const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "synced", bondedStake: 0, validatorActivation: a })} />);
      expect(html).toContain("Synced · " + word);
      expect(html).not.toContain("Bond locked · not staked");
    }
  });
});
