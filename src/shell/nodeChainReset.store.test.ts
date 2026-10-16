// 40204 reroll: when the node's start deleted another genesis's chain data, `node_status` carries a
// one-line notice. The store shows it once per session, and never without the notice.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";

const NOTICE = "Citrate Network was upgraded to a new chain; your node is resyncing from the start";

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ toast: null });
});

describe("node chain-reset notice", () => {
  it("toasts the notice once, then stays quiet on later polls", async () => {
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 1, height: 3, syncPct: 1, notice: NOTICE });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    store.setState({ node: "syncing", toast: null });
    await store.refreshNode();
    expect(store.state.toast).toBe(NOTICE);
    store.setState({ toast: null });
    await store.refreshNode();
    expect(store.state.toast).toBeNull();
  });
});
