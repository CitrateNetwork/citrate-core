// The mining gate (src-tauri/src/mining_gate.rs) holds desktop block production unless the
// network tip is recent, this node is caught up and on the same branch, it has peers, and at
// least two other producers are live. `node_status.miningHold` says why production is held;
// the store keeps it in state for the Node surface, and a producer the backend disarmed is no
// longer treated as armed here.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { store } from "./store";
import { bridge } from "../bridge";
import { freshState } from "./state";
import { renderStatus } from "../surfaces/node/nodePage.fixture";
import type { Store } from "./store";

const HOLD = "Block production is off: 1 other producer(s) are live, at least 2 are needed first.";

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ toast: null, miningHold: null, node: "off" });
  (store as unknown as { validatorArmed: boolean }).validatorArmed = false;
});

describe("mining gate hold", () => {
  it("folds the hold reason into state", async () => {
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 10, syncPct: 100, miningHold: HOLD });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    await store.refreshNode();
    expect(store.state.miningHold).toBe(HOLD);
  });

  it("forgets a local arm once the backend disarmed the producer", async () => {
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 10, syncPct: 100, miningHold: HOLD });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    await store.refreshNode();
    expect((store as unknown as { validatorArmed: boolean }).validatorArmed).toBe(false);
  });

  it("keeps an armed producer armed and clears the hold", async () => {
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    store.setState({ miningHold: HOLD });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 10, syncPct: 100, miningArmed: true });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    await store.refreshNode();
    expect(store.state.miningHold).toBeNull();
    expect((store as unknown as { validatorArmed: boolean }).validatorArmed).toBe(true);
  });

  it("is not persisted across launches", async () => {
    const { PERSIST_KEYS } = await import("./state");
    expect(PERSIST_KEYS).not.toContain("miningHold");
    expect(freshState("p1").miningHold).toBeNull();
  });

  it("the Node surface shows the reason", () => {
    const stub = { toast: () => {}, identity: () => ({ wallet: "" }), refreshEarnings: () => {} } as unknown as Store;
    const html = renderStatus(stub, { ...freshState("p1"), miningHold: HOLD });
    expect(html).toContain("1 other producer(s) are live");
    const none = renderStatus(stub, { ...freshState("p1"), miningHold: null });
    expect(none).not.toContain("Block production is off");
  });
});
