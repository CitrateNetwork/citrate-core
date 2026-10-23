// K4 — Pins honesty (UI truth contract §1 Pins / §2 pin bond parameters / §4 dormant V3).
//
// IPFSIncentivesV3 is deployed but dormant and unfunded: the Pins component must say "Pin bonds
// not live yet", show BOND / REWARD / ROUNDS only as parameters, label every pin this node keeps
// "Local pin · no bond", never draw Pinned / Challenged / Released as live, and never show an
// unknown parameter as 0.
import { describe, it, expect, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { StoragePins } from "./StoragePins";
import { storageSlice } from "../shell/slices/storage";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";
import type { PinParams, PinRow } from "../bridge/domains";

const stubStore = { toast: () => {} } as unknown as Store;
const render = () => renderToStaticMarkup(<StoragePins store={stubStore} s={freshState("p1")} />);

const PARAMS: PinParams = {
  contract: "0xf5c115e6d88a960cc4f48129524c01004f067135",
  bondWei: "10000000000000000000",
  rewardWei: "4000000000000000000",
  rounds: 4,
  bondsLive: false,
  source: "IPFSIncentivesV3 BOND() / REWARD() / ROUNDS() via eth_call on https://rpc.citrate.ai",
  errors: [],
};

const row = (cid: string, pinState: PinRow["pinState"], bondSalt = "local"): PinRow => ({
  cid,
  sizeBytes: 2_000_000,
  bondSalt,
  pinState,
  addedAt: 1,
});

afterEach(() => {
  storageSlice.set({ pins: [], pinParams: null, pinParamsError: null, kuboDown: false, error: null });
});

describe("K4 Pins — parameters only, bonds not live", () => {
  it("shows the required wording and the three V3 parameters with their source", () => {
    storageSlice.set({ pinParams: PARAMS });
    const html = render();
    expect(html).toContain("Pin bonds not live yet");
    expect(html).toContain("Bond per pin (parameter)");
    expect(html).toContain("10 SALT");
    expect(html).toContain("4 SALT over 4 rounds");
    expect(html).toContain("via eth_call on https://rpc.citrate.ai");
    expect(html).toContain(PARAMS.contract);
  });

  it("an unknown parameter is a dash with its reason, never 0", () => {
    storageSlice.set({
      pinParams: { ...PARAMS, bondWei: null, rewardWei: null, rounds: null, errors: ["BOND(): the RPC could not be reached: timeout"] },
    });
    const html = render();
    expect(html).not.toMatch(/\b0 SALT\b/);
    expect(html).toContain("—");
    expect(html).toContain("BOND(): the RPC could not be reached");
  });

  it("a failed parameter read as a whole says so and shows no number", () => {
    storageSlice.set({ pinParams: null, pinParamsError: "Pin parameters are read from chain by the desktop app." });
    const html = render();
    expect(html).toContain("parameters unavailable");
    expect(html).not.toMatch(/\d+ SALT/);
  });
});

describe("K4 Pins — the local pin index", () => {
  it("labels kept pins 'Local pin · no bond' and never shows a live chain pin state", () => {
    storageSlice.set({
      pinParams: PARAMS,
      pins: [
        row("bafykept", "pinned"),
        row("bafygone", "unpinned"),
        // Reserved values the Rust side never emits today; still never shown as chain state.
        row("bafychal", "challenged", "salt-bond-42"),
        row("bafypend", "pinning"),
      ],
    });
    const html = render();
    expect(html).toContain("Local pin · no bond");
    expect(html).toContain("Not pinned on this node");
    expect(html).not.toMatch(/\bPinned\b/);
    expect(html).not.toMatch(/Challenged|Released|Slashed|bond claimable/);
    expect(html).not.toMatch(/\bbonded\b/i);
    expect(html).toContain("1 kept on this node");
  });

  it("offers local actions only: Pin locally / Remove pin, never a bond", () => {
    storageSlice.set({ pins: [row("bafykept", "pinned"), row("bafygone", "unpinned")] });
    const html = render();
    expect(html).toContain("Remove pin");
    expect(html).toContain("Pin locally");
    expect(html).not.toMatch(/Pin · bond|approve the bond|staked SALT bond/i);
  });

  it("an unreachable IPFS store is an honest alert, not an empty list", () => {
    storageSlice.set({ kuboDown: true, error: "kubo transport error: connection refused" });
    const html = render();
    expect(html).toContain("role=\"alert\"");
    expect(html).toContain("connection refused");
    expect(html).not.toContain("No pins yet");
  });

  it("names its data sources (Rule 7)", () => {
    const html = render();
    expect(html).toContain("local pin index reconciled with the IPFS daemon");
  });
});
