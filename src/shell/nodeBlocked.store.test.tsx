// SCL-S0.6 (US-0.4): when the node start is refused because another process holds the chain
// database or a node port, `node_status.blocked` names the holder (pid, path) and the action.
// The store keeps that message in state (the Node surface shows it) and shows the node as an
// error, not as a silent "off"; the message clears once the start is no longer blocked.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { store } from "./store";
import { bridge } from "../bridge";
import { freshState } from "./state";
import { Node } from "../surfaces/Node";
import type { Store } from "./store";

const MESSAGE =
  "An older Citrate node is still running (process 4242, /Applications/Citrate Core.app/Contents/MacOS/citrate) " +
  "and holds the chain database in /x/node. The node was not started and no chain data was deleted. " +
  "Quit that process (or restart your computer), then start the node again.";

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ toast: null, nodeBlocked: null, node: "off" });
});

describe("node blocked by a holder (S0.6)", () => {
  it("folds the blocker message into state and shows the node as an error", async () => {
    vi.spyOn(bridge.node, "status").mockResolvedValue({
      state: "stopped",
      peers: 0,
      height: 0,
      syncPct: 0,
      blocked: { resource: "chainDatabase", pid: 4242, path: "/Applications/Citrate Core.app/Contents/MacOS/citrate", message: MESSAGE },
    });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    store.setState({ node: "off", nodeBlocked: null });
    await store.refreshNode();
    expect(store.state.nodeBlocked).toBe(MESSAGE);
    expect(store.state.node).toBe("error");
  });

  it("clears the message once the status no longer reports a holder", async () => {
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0 });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    store.setState({ node: "error", nodeBlocked: MESSAGE });
    await store.refreshNode();
    expect(store.state.nodeBlocked).toBeNull();
  });

  it("is not persisted across launches", async () => {
    const { PERSIST_KEYS } = await import("./state");
    expect(PERSIST_KEYS).not.toContain("nodeBlocked");
    expect(freshState("p1").nodeBlocked).toBeNull();
  });

  it("the Node surface names the holder and the action", () => {
    const stub = { toast: () => {}, identity: () => ({ wallet: "" }), refreshEarnings: () => {} } as unknown as Store;
    const s = { ...freshState("p1"), node: "error" as const, nodeBlocked: MESSAGE };
    const html = renderToStaticMarkup(<Node store={stub} s={s} />);
    expect(html).toContain("process 4242");
    expect(html).toContain("/Applications/Citrate Core.app/Contents/MacOS/citrate");
    expect(html).toContain("Quit that process");
    const none = renderToStaticMarkup(<Node store={stub} s={{ ...freshState("p1"), nodeBlocked: null }} />);
    expect(none).not.toContain("Quit that process");
  });
});
