// K6-3 (K6e): Settings › Network renders bootnodes, RPC and explorer from the network profile
// (src/network/profile.ts), and the hard-coded bootnode line (with boot-eu1) is gone.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import { ACTIVE_PROFILE } from "../network/profile";

const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "s", wallet: "", tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

function settings(sSec: AppState["sSec"], patch: Partial<AppState> = {}): string {
  const s = { ...freshState("p1"), sSec, signedIn: true, ...patch };
  return renderToStaticMarkup(<Settings store={stubStore} s={s} />);
}

const host = (url: string) => new URL(url).host;

describe("Settings › Network on the profile (K6-3)", () => {
  it("Node configuration lists the profile's bootnodes and no hard-coded boot-eu1", () => {
    const html = settings("node");
    expect(html).not.toContain("boot-eu1");
    for (const b of ACTIVE_PROFILE!.bootnodes!) expect(html).toContain(b.slice(b.indexOf("@") + 1));
  });

  it("Node configuration names the network, its RPC and its explorer from the profile", () => {
    const html = settings("node");
    expect(html).toContain(ACTIVE_PROFILE!.label);
    expect(html).toContain(ACTIVE_PROFILE!.rpcUrl!);
    expect(html).toContain(ACTIVE_PROFILE!.explorerUrl!);
  });

  it("the network button names the profile's node network and chain id", () => {
    const html = settings("node");
    expect(html).toContain(`${ACTIVE_PROFILE!.nodeNetwork} · ${ACTIVE_PROFILE!.chainId}`);
  });

  it("API endpoints: the public RPC is the profile's RPC host", () => {
    const html = settings("api", { rpc: "public" });
    expect(html).toContain(host(ACTIVE_PROFILE!.rpcUrl!));
  });
});
