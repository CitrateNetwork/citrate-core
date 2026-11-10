// K3 (ported from #362, Codex P2 on #362 at 366fc4f): Node › Status's freshness is the node_status
// answer the store's poll actually committed (AppState.nodeStatusAt), never a separate probe, and it
// is observed state: never persisted or restored.
import { beforeEach, describe, expect, it, vi } from "vitest";

const nodeStatus = vi.fn();
const grantStatus = vi.fn();

vi.mock("../../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: {
      status: () => nodeStatus(),
      logs: async () => [],
      armMining: async () => false,
      validatorEarnings: async () => {
        throw new Error("proposer key not minted yet");
      },
    },
    membership: { grantStatus: () => grantStatus() },
  },
}));
vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "../../shell/store";

const RUNNING = { state: "running", peers: 3, height: 510_030, syncPct: 100 };

function newStore(): Store {
  const store = new Store();
  store.chainReads = {
    validatorActivation: async () => ({ isActive: false, status: 0, actEpoch: 0, currentEpoch: 40 }),
    validatorCounts: async () => ({ registered: 1, eligible: 1, pending: 0 }),
    minStakeWei: async () => 0n,
    blockSeconds: async () => null,
    producerSample: async () => ({ head: 1, window: 1, proposers: [] }),
  };
  return store;
}

beforeEach(() => {
  nodeStatus.mockReset();
  grantStatus.mockReset();
  grantStatus.mockRejectedValue(new Error("no rpc in tests"));
  nodeStatus.mockResolvedValue(RUNNING);
});

describe("Status › freshness is the committed store snapshot", () => {
  it("a completed node poll records when its node_status answer was read", async () => {
    const store = newStore();
    store.setState({ node: "synced", hasGrant: true });
    const before = Date.now();
    await store.refreshNode();
    expect(typeof store.state.nodeStatusAt).toBe("number");
    expect(store.state.nodeStatusAt as number).toBeGreaterThanOrEqual(before);
    expect(store.state.height).toBe(510_030);
  });

  it("a poll stuck after node_status (grant read hanging) does not renew freshness", async () => {
    const store = newStore();
    store.setState({ node: "synced", hasGrant: false, nodeStatusAt: 1_000 });
    grantStatus.mockImplementation(() => new Promise(() => {}));
    void store.refreshNode();
    for (let i = 0; i < 10; i++) await Promise.resolve();
    expect(nodeStatus).toHaveBeenCalled();
    expect(store.state.nodeStatusAt).toBe(1_000);
  });

  it("a failed node_status read leaves the last committed time to age", async () => {
    const store = newStore();
    store.setState({ node: "synced", nodeStatusAt: 1_000 });
    nodeStatus.mockRejectedValue(new Error("supervisor not answering"));
    await store.refreshNode();
    expect(store.state.nodeStatusAt).toBe(1_000);
  });

  it("is never persisted or restored", async () => {
    const { PERSIST_KEYS, OBSERVED_NOT_PERSISTED, freshState } = await import("../../shell/state");
    expect(PERSIST_KEYS).not.toContain("nodeStatusAt");
    expect(OBSERVED_NOT_PERSISTED).toContain("nodeStatusAt");
    expect(freshState("p1").nodeStatusAt).toBeNull();
  });

  it("a 0.5.1 save's Node tab switch and seeded crash rows are dropped on load", async () => {
    const { loadState, STORAGE_KEY } = await import("../../shell/state");
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", nTab: "earn", crashes: [{ when: "2026-07-06 03:12 · exit 137", note: "OOM" }], nodeStatusAt: 5 }));
    try {
      const s = loadState() as unknown as Record<string, unknown>;
      expect("nTab" in s).toBe(false);
      expect("crashes" in s).toBe(false);
      expect(s.nodeStatusAt).toBeNull();
    } finally {
      localStorage.removeItem(STORAGE_KEY);
    }
  });
});
