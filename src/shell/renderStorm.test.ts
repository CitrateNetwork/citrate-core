// HUP-S0.6 — render storm. Every store notification re-renders the whole app (Root subscribes to
// the full state), so: (1) an empty patch must not notify; (2) the desktop build's 600 ms tick must
// not change anything while idle (it used to tick cosmetic countdowns → a full re-render ~1.7×/s
// forever); (3) streamed tokens are coalesced instead of one full render per token.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store, tickPatch } from "./store";
import type { ChatProvider } from "../agent/harness";

afterEach(() => vi.restoreAllMocks());

describe("HUP-S0.6 render storm", () => {
  it("an empty setState does not notify subscribers", () => {
    const cb = vi.fn();
    const off = store.subscribe(cb);
    store.setState({});
    store.setState(() => ({}));
    off();
    expect(cb).not.toHaveBeenCalled();
  });

  it("the desktop tick is a no-op while idle (no cosmetic countdowns outside the sim)", () => {
    const s = { ...store.state, node: "validating" as const, s5: "done" as never };
    expect(tickPatch(s, /* sim */ false, () => 0)).toEqual({});
  });

  it("the sim tick still animates the preview", () => {
    const s = { ...store.state, node: "validating" as const };
    expect(Object.keys(tickPatch(s, true, () => 0)).length).toBeGreaterThan(0);
  });

  it("a 200-token reply causes a handful of renders, not 200", async () => {
    store.setState({ chatMsgs: [], chatStatus: "ready" });
    store.provider = {
      kind: "local",
      label: "t",
      send: async ({ callbacks }) => {
        callbacks.onStatus?.("streaming");
        for (let i = 0; i < 200; i++) callbacks.onToken?.("w ");
        callbacks.onStatus?.("done");
        return { role: "assistant", content: "" };
      },
    } as ChatProvider;
    const cb = vi.fn();
    const off = store.subscribe(cb);
    await store.sendChat("hi");
    off();
    expect(store.state.chatMsgs.find((m) => m.who !== "You")?.text).toBe("w ".repeat(200));
    expect(cb.mock.calls.length).toBeLessThan(15);
  });
});
