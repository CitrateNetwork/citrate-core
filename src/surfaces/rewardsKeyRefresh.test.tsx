// fxEARN (Codex P2 on #556, refreshEarnings): Node › Rewards and Wallet re-read the rewards when the
// member's validator key resolves while they are mounted. An activated bond whose MemberBond.pubkey()
// reads after mount used to leave the effect's inputs unchanged, so the rewards stayed "—" until a
// tab change or a remount.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { NodeRewards } from "./node/NodeRewards";
import { Wallet } from "./Wallet";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// K3 (#563): Node › Rewards is its own tab component; the desktop app renders it in tauri mode.
const Node = (p: { store: Store; s: AppState }) => createElement(NodeRewards, { ...p, mode: "tauri" });

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const BOND_PUB = "0xb4a646a56e3946943f0014459e88595c45737745e0421e201ac1d3f9320ff748";

function stubStore(refreshEarnings: () => void): Store {
  return new Proxy({} as Store, {
    get(_t, p) {
      if (p === "refreshEarnings") return refreshEarnings;
      if (p === "identity") return () => ({ sub: "s", wallet: "0x" + "0".repeat(40), name: "M", initials: "M", email: "—", tier: "pilot", role: "member", org: null, real: true });
      if (p === "walletIsLinked") return () => true;
      return () => Promise.resolve(undefined);
    },
  });
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
});

function state(bondPubkey: string | null): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", nTab: "earn", hasGrant: true, bondActivated: true, bondPubkey, bondedStake: 32_000 } as AppState;
}

for (const [name, Surface] of [["Node", Node], ["Wallet", Wallet]] as const) {
  it(name + " re-reads the rewards when the bond key resolves after mount", () => {
    const refresh = vi.fn();
    const store = stubStore(refresh);
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => root?.render(createElement(Surface as never, { store, s: state(null) })));
    const before = refresh.mock.calls.length;
    expect(before).toBeGreaterThan(0);
    act(() => root?.render(createElement(Surface as never, { store, s: state(BOND_PUB) })));
    expect(refresh.mock.calls.length).toBeGreaterThan(before);
  });
}

describe("fxEARN — the same key does not re-read", () => {
  it("Node: a re-render with the same resolved key reads once", () => {
    const refresh = vi.fn();
    const store = stubStore(refresh);
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => root?.render(createElement(Node as never, { store, s: state(BOND_PUB) })));
    const n = refresh.mock.calls.length;
    act(() => root?.render(createElement(Node as never, { store, s: { ...state(BOND_PUB), height: 5 } })));
    expect(refresh.mock.calls.length).toBe(n);
  });
});
