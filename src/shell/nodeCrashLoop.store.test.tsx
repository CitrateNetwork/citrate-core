// 0.5.7: a node that keeps stopping on start with the same error (`node_status.crashLoop`, from the
// supervisor's crash records) is kept in state and named on Node › Status with the node's own line
// and, for the open-file limit, the guidance. It clears once the status no longer reports a loop.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import { freshState } from "./state";
import { renderStatus } from "../surfaces/node/nodePage.fixture";
import { CRASH_LOOP_WORD } from "../surfaces/node/nodeWords";
import type { NodeCrashLoopView } from "../bridge/domains";
import type { Store } from "./store";

const LOOP: NodeCrashLoopView = {
  kind: "tooManyOpenFiles",
  line: "Error: IO error: While open a file for random read: /x/node/007679.sst: Too many open files",
  count: 4,
  firstAtUnixMs: 1_000,
  lastAtUnixMs: 91_000,
};

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ toast: null, nodeCrashLoop: null, node: "off" });
});

describe("node crash loop (0.5.7)", () => {
  it("folds node_status.crashLoop into state while the supervisor restarts", async () => {
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "restarting", peers: 0, height: 0, syncPct: 0, crashLoop: LOOP });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    store.setState({ node: "off", nodeCrashLoop: null });
    await store.refreshNode();
    expect(store.state.nodeCrashLoop).toEqual(LOOP);
  });

  it("clears once the status no longer reports a loop", async () => {
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 10, syncPct: 50 });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    store.setState({ node: "prov", nodeCrashLoop: LOOP });
    await store.refreshNode();
    expect(store.state.nodeCrashLoop).toBeNull();
  });

  it("is not persisted across launches", async () => {
    const { PERSIST_KEYS } = await import("./state");
    expect(PERSIST_KEYS).not.toContain("nodeCrashLoop");
    expect(freshState("p1").nodeCrashLoop).toBeNull();
  });

  it("Node › Status names the loop, the line and the guidance instead of Starting", () => {
    const stub = { toast: () => {}, identity: () => ({ wallet: "" }), refreshEarnings: () => {} } as unknown as Store;
    const html = renderStatus(stub, { ...freshState("p1"), node: "prov", nodeCrashLoop: LOOP });
    expect(html).toContain(CRASH_LOOP_WORD);
    expect(html).toContain("007679.sst: Too many open files");
    expect(html).toContain("open-file limit is too low for the node");
    expect(html).toContain('role="alert"');
    const none = renderStatus(stub, { ...freshState("p1"), node: "prov", nodeCrashLoop: null });
    expect(none).not.toContain(CRASH_LOOP_WORD);
  });
});
