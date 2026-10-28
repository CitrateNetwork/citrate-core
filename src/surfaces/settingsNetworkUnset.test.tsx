// K6-3 (K6e): with a network profile that is not set (the genesis-2026-12 profile before its
// values are recorded), Settings › Network says "Update required" and names what is missing. It
// shows no RPC, explorer or bootnode of another network in its place.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../network/profile", async (orig) => {
  const real = await orig<typeof import("../network/profile")>();
  const next = real.PROFILES.find((p) => p.id === "genesis-2026-12")!;
  return {
    ...real,
    ACTIVE_PROFILE_ID: next.id,
    ACTIVE_PROFILE: next,
    isReady: (p = next) => real.isReady(p),
    gateGenesis: (p = next) => real.gateGenesis(p),
    expectedGenesis: (p = next) => real.expectedGenesis(p),
  };
});

import { Settings } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "s", wallet: "", tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

function settings(sSec: AppState["sSec"], patch: Partial<AppState> = {}): string {
  const s = { ...freshState("p1"), sSec, signedIn: true, ...patch };
  return renderToStaticMarkup(<Settings store={stubStore} s={s} />);
}

describe("Settings › Network with the profile not set (K6-3)", () => {
  it("says Update required and names the missing values", () => {
    const html = settings("node");
    expect(html).toContain("Update required");
    expect(html).toContain("no network set");
    expect(html).toContain("rpcUrl not set");
  });

  it("shows no bootnode or RPC of another network", () => {
    const html = settings("node");
    expect(html).not.toContain("boot1.citrate.ai");
    expect(html).not.toContain("boot-eu1");
    expect(html).not.toContain("rpc.citrate.ai");
  });

  it("API endpoints: the public RPC reads not set, not rpc.citrate.ai", () => {
    const html = settings("api", { rpc: "public" });
    expect(html).not.toContain("rpc.citrate.ai");
    expect(html).toContain("not set");
  });
});
