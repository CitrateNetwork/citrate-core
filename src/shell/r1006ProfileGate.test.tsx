// K6 (RC52 D-2): the r1006 profile the 0.5.2 RC targets. Until the reroll values are filled
// (genesis hash, address-book digest, MIN_CHAIN_REV) the network gate blocks with the honest
// "not set" notice and never reads block 0. Filled with fixture values, the same profile passes
// on a matching genesis and is blocked on any other.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

const LIVE = { hash: null as string | null };
const calls: { query?: { enabled?: boolean } }[] = [];
vi.mock("wagmi", () => ({
  useBlock: (args: { query?: { enabled?: boolean } }) => {
    calls.push(args);
    return { data: args.query?.enabled && LIVE.hash ? { hash: LIVE.hash } : undefined };
  },
}));

import { NetworkCompatGate } from "./NetworkCompatGate";
import { PROFILES, gateGenesis, isReady, type NetworkProfile } from "../network/profile";

const FIXTURE_GENESIS = "0x" + "6a".repeat(32);
const r1006 = (): NetworkProfile => {
  const p = PROFILES.find((x) => x.id === "r1006");
  if (!p) throw new Error("no r1006 profile");
  return { ...p, pending: { ...p.pending } };
};
const filled = (): NetworkProfile => ({
  ...r1006(),
  genesisHash: FIXTURE_GENESIS,
  addressBookSha256: "5c".repeat(32),
  minChainRev: "1f".repeat(20),
  pending: {},
});
const render = (p: NetworkProfile) => renderToStaticMarkup(<NetworkCompatGate profile={p} />);

describe("r1006 network gate (K6, RC52)", () => {
  beforeEach(() => {
    calls.length = 0;
    LIVE.hash = null;
  });

  it("unfilled r1006 is not ready and blocks with the not-set notice, without reading block 0", () => {
    const p = r1006();
    expect(isReady(p)).toBe(false);
    expect(gateGenesis(p)).toBeNull();
    LIVE.hash = FIXTURE_GENESIS; // even a live chain cannot unblock an unset profile
    const html = render(p);
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain("does not have the network it should connect to set");
    expect(calls.every((c) => c.query?.enabled === false)).toBe(true);
  });

  it("each reroll value alone keeps it blocked", () => {
    for (const field of ["genesisHash", "addressBookSha256", "minChainRev"] as const) {
      const p = { ...filled(), [field]: null };
      expect(isReady(p), field).toBe(false);
      LIVE.hash = FIXTURE_GENESIS;
      expect(render(p), field).toContain('role="alertdialog"');
    }
  });

  it("filled with fixture values it passes on the matching genesis", () => {
    const p = filled();
    expect(isReady(p)).toBe(true);
    expect(gateGenesis(p)).toBe(FIXTURE_GENESIS);
    LIVE.hash = FIXTURE_GENESIS;
    expect(render(p)).toBe("");
    expect(calls.some((c) => c.query?.enabled === true)).toBe(true);
  });

  it("filled, it is blocked on a chain with another genesis (r1005)", () => {
    LIVE.hash = PROFILES.find((x) => x.id === "alpha-r1005")!.genesisHash;
    const html = render(filled());
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain("restarted with a new genesis");
  });
});
