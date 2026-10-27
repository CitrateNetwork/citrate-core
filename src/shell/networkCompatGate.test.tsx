// K6 (Codex P2 on core#363): the network gate blocks on ANY profile that is not fully ready,
// matching the Rust `NetworkProfile::ready` rule that refuses node starts and every signature,
// not only on a profile with no genesis.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

const LIVE = { hash: null as string | null };
const calls: { query?: { enabled?: boolean } }[] = [];
vi.mock("wagmi", () => ({
  useBlock: (args: { query?: { enabled?: boolean } }) => {
    calls.push(args);
    // A disabled query never answers; an enabled one answers with the live block 0.
    return { data: args.query?.enabled && LIVE.hash ? { hash: LIVE.hash } : undefined };
  },
}));

import { NetworkCompatGate } from "./NetworkCompatGate";
import { ACTIVE_PROFILE, expectedGenesis, type NetworkProfile } from "../network/profile";

const ready = (): NetworkProfile => ({ ...ACTIVE_PROFILE!, pending: { ...ACTIVE_PROFILE!.pending } });
const OTHER = "0x98e0d72f422049606a6b29ca0a9bcfd2300753fd36526d2c8e9f9a2531b70c73";

function render(profile: NetworkProfile | null): string {
  return renderToStaticMarkup(<NetworkCompatGate profile={profile} />);
}

describe("NetworkCompatGate (K6)", () => {
  beforeEach(() => {
    calls.length = 0;
    LIVE.hash = null;
  });

  it("blocks a profile with a valid genesis but a missing addressBook, without reading block 0", () => {
    const p = { ...ready(), addressBook: null };
    expect(expectedGenesis(p)).not.toBeNull(); // the genesis alone looks fine
    LIVE.hash = expectedGenesis(p); // even a live chain that matches cannot unblock it
    const html = render(p);
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain("does not have the network it should connect to set");
    expect(calls.every((c) => c.query?.enabled === false)).toBe(true);
  });

  it("blocks every other partial or malformed profile the Rust ready() rule refuses", () => {
    const base = ready();
    for (const p of [
      { ...base, rpcUrl: null },
      { ...base, bootnodes: null },
      { ...base, bootnodes: [] },
      { ...base, bootnodes: ["boot1.citrate.ai:30303"] },
      { ...base, chainId: 0 },
      { ...base, nodeNetwork: "has space" },
      { ...base, explorerUrl: "http://explorer.example.org" },
      { ...base, faucetUrl: "https://" },
      { ...base, addressBook: "" },
      { ...base, genesisHash: null },
    ]) {
      LIVE.hash = expectedGenesis(base);
      expect(render(p), JSON.stringify(p)).toContain('role="alertdialog"');
    }
    expect(render(null)).toContain('role="alertdialog"');
  });

  it("a fully ready profile reads block 0 and passes on a match", () => {
    const p = ready();
    LIVE.hash = expectedGenesis(p);
    expect(render(p)).toBe("");
    expect(calls.some((c) => c.query?.enabled === true)).toBe(true);
  });

  it("a fully ready profile is blocked on a definite different genesis", () => {
    LIVE.hash = OTHER;
    const html = render(ready());
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain("restarted with a new genesis");
  });

  it("a fully ready profile does not block while block 0 is unknown", () => {
    expect(render(ready())).toBe("");
  });
});
