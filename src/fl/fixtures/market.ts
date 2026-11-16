// Test fixture (0.5.4 Hermes x FL): a 0.6.0 network with WorkMarket and payouts open, and one
// joinable offer, in the exact shapes networkState.ts and core's fl_market_offers return. Imported
// only by the gate tripwires; on r1007 no WorkMarket is pinned and no shipped profile opens payouts.
import realBook from "../../../src-tauri/addresses/40204.json";
import { readFlNetworkState, type FlNetworkState } from "../networkState";
import type { MarketView } from "../market";
import { ACTIVE_PROFILE, type NetworkProfile } from "../../network/profile";

const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);

export function payoutsOpenNetwork(): Promise<FlNetworkState> {
  return readFlNetworkState(
    { addresses: { ...realBook.addresses, WorkMarket: WM } },
    { codeSize: async () => 1000, viewCall: async () => ZERO32 },
    { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile,
  );
}

export function oneOfferMarket(): MarketView {
  return {
    available: true,
    reason: null,
    market: WM,
    offers: [
      {
        opened: { id: 1, kind: "flRound", poster: "0x" + "11".repeat(20), groupHash: ZERO32, pricePerUnit: "1500000000000000000", budget: "600000000000000000000", deadline: 4_102_444_800, requirementsCid: "bafkreifixture" },
        openedBlock: 100,
        closed: null,
        terms: { state: "open", jobSpecHash: ZERO32, requirementsHash: ZERO32, allowlistRoot: ZERO32, maxWorkers: 8, workers: 3, joinBond: "100000000000000000", coordinator: "0x" + "cd".repeat(20), disputeWindow: 100 },
        requirements: { status: "verified", requirements: { v: 1, dataClasses: [], architectures: [], minVramGb: 8, estHours: 2 } },
        hidden: null,
      },
    ],
  };
}

/** A tier report whose facts give this machine 24 GB of VRAM. */
export const VRAM_24GB_REPORT = { facts: { gpuVramBytes: 24 * 1024 ** 3, unifiedMemory: false, totalRamBytes: 64 * 1024 ** 3 } } as never;
