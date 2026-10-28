// fxEARN (review P1-1 on #556): a granted member whose bond was never activated is not told the
// bond is activated, even when this node's key is registered with the member's wallet as staker.
// Only the bond's own registry stake (pubkeyOfStaker(bond) → stakeOf) counts as staked.
import { afterEach, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Node } from "./Node";
import { Store } from "../shell/store";
import { bridge } from "../bridge";
import type { ChainReads } from "../shell/chainTruth";
import type { GrantStatus } from "../bridge/domains";

const WALLET = "0x5aeb8c04c103b46d184307d29a6a58803832bd13";

afterEach(() => vi.restoreAllMocks());

it("granted, bond not activated, node key wallet-staked: bondedStake stays 0 and the copy says not activated", async () => {
  const store = new Store();
  store.setState({ signedIn: true, walletAddr: WALLET, node: "synced", hasGrant: true, s5StakeWei: (32_000n * 10n ** 18n).toString(), s5UnlockBlock: 16_200_000, bondedStake: 0, bondActivated: null });
  const grant = {
    attributedStakeWei: (32_000n * 10n ** 18n).toString(),
    attributedPrincipalWei: (32_000n * 10n ** 18n).toString(),
    hasSbt: true,
    bondAddress: "0x" + "b".repeat(40),
    bondDeployed: true,
    bondedStakeWei: "0",
    hasValidator: false,
    unlockBlock: 16_200_000,
    isUnlocked: false,
    isKycVerified: false,
    bondActivated: false,
    bondPubkey: null,
    bondUnlocked: false,
  } as unknown as GrantStatus;
  // The registry read for this node's key, shaped as the live answer with the wallet as staker.
  const reads = {
    validatorActivation: async () => ({ isActive: true, status: 1, actEpoch: 343, currentEpoch: 877, staker: WALLET, bondedWei: (32_000n * 10n ** 18n).toString() }),
    minStakeWei: async () => 32_000n * 10n ** 18n,
    blockSeconds: async () => 2,
    producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
    validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
  } as unknown as ChainReads;
  (store as unknown as { chainReads: ChainReads }).chainReads = reads;
  vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 4, height: 877_400, syncPct: 100, miningArmed: false } as never);
  vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
  vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: "0x" + "11".repeat(32) });
  vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant);
  await store.refreshNode();
  expect(store.state.bondActivated).toBe(false);
  expect(store.state.bondedStake).toBe(0);
  const html = renderToStaticMarkup(<Node store={store} s={{ ...store.state, nTab: "status" }} />);
  expect(html).not.toContain("Your bond is activated");
});
