// K3 (ported from #362, Codex P2 on #362 at 366fc4f): Node › Status's 10 s probe of node_status can
// answer while the store's own poll is stuck, and its success must not renew the freshness of the
// store's older height, peers and state. The page's age comes from the snapshot the store committed
// (nodeStatusAt); the probe can only report that node_status fails.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

const nodeStatus = vi.fn();
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: 510_032n, dataUpdatedAt: Date.now(), isError: false }),
}));
vi.mock("../../shell/networkCompat", () => ({ useNetworkCompat: () => ({ status: "compatible", live: null, expected: null }) }));
vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
vi.mock("../../bridge", () => ({ bindSimHost: () => {}, bridge: { mode: "tauri", node: { status: () => nodeStatus() } } }));

import { freshState, type AppState } from "../../shell/state";
import type { Store } from "../../shell/store";
import { Node } from "../Node";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  nodeStatus.mockReset();
});

const store = { go: () => {}, toast: () => {}, walletIsLinked: () => true, refreshProducers: vi.fn(async () => {}), refreshActivation: vi.fn(async () => true) } as unknown as Store;

async function mount(s: AppState): Promise<HTMLDivElement> {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<Node store={store} s={s} />);
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
  return host;
}

const at = (patch: Partial<AppState>): AppState => ({ ...freshState("p1"), stage: "done", node: "synced", height: 510_030, peers: 4, hasGrant: true, ...patch });

describe("Node › Status freshness", () => {
  it("a store snapshot older than 60 s is out of date even while the probe answers", async () => {
    nodeStatus.mockResolvedValue({ state: "running", height: 510_031, peers: 4, syncPct: 100 });
    const h = await mount(at({ nodeStatusAt: Date.now() - 61_000 }));
    expect(nodeStatus).toHaveBeenCalled();
    expect(h.querySelector('[data-state="error"]')?.textContent ?? "").toMatch(/out of date/);
    expect(h.querySelector('[data-testid="node-sentence"]')).toBeNull();
    expect(h.textContent).not.toContain("510,030");
  });

  it("a fresh store snapshot shows the store's values against the public head", async () => {
    nodeStatus.mockResolvedValue({ state: "running" });
    const h = await mount(at({ nodeStatusAt: Date.now() - 1_000 }));
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced");
    expect(h.textContent).toContain("510,030 · 2 behind the public head");
  });

  it("a failing node_status still shows the error state", async () => {
    nodeStatus.mockRejectedValue(new Error("supervisor not answering"));
    const h = await mount(at({ nodeStatusAt: Date.now() - 1_000 }));
    expect(h.querySelector('[data-state="error"]')?.textContent).toContain("supervisor not answering");
  });

  it("a paused node is not out of date: the store stops polling on purpose, and Start stays offered", async () => {
    nodeStatus.mockResolvedValue({ state: "stopped" });
    const h = await mount(at({ node: "paused", nodeStatusAt: Date.now() - 600_000 }));
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Off");
    expect([...h.querySelectorAll("button")].some((b) => b.textContent === "Start node")).toBe(true);
  });

  it("opening Status reads the producer sample and the activation record (#406)", async () => {
    nodeStatus.mockResolvedValue({ state: "running" });
    await mount(at({ nodeStatusAt: Date.now() - 1_000 }));
    expect(store.refreshProducers).toHaveBeenCalled();
    expect(store.refreshActivation).toHaveBeenCalled();
  });
});
