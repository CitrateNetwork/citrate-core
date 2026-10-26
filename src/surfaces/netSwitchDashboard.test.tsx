// RC52 (UI truth contract): after a genesis switch the Dashboard's Staked tile waits for the first
// read on the new chain. It says "not read yet", never "no stake" (unknown is not zero).
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Dashboard } from "./Dashboard";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";
import type { NetRead } from "../shell/netScope";

const stub = { walletIsLinked: () => true, refreshEarnings: () => {}, toast: () => {}, identity: () => ({ wallet: "" }) } as unknown as Store;

describe("Dashboard Staked tile after a genesis switch", () => {
  it("says 'not read yet' until the grant and balances are read on the new chain", () => {
    const netUnread: NetRead[] = ["grant", "balances", "activity"];
    const html = renderToStaticMarkup(<Dashboard store={stub} s={{ ...freshState("p1"), netUnread }} />);
    expect(html).toContain("not read yet on this network");
    expect(html).not.toContain("no stake");
  });

  it("says 'no stake' once both reads found none", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={{ ...freshState("p1"), netUnread: [] }} />);
    expect(html).toContain("no stake");
    expect(html).not.toContain("not read yet on this network");
  });
});
