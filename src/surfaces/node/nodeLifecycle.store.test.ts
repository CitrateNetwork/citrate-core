// K3 Node › Services and Status against the real Store (Codex on #362 at 366fc4f, both P2).
//
// P2 (Services): Restart's live adapters must report a failed start. Store.startNode() returns
// before its bridge call settles and startMemoryDaemon() swallowed its failure, so a failed start
// read "Started.". A Restart also must not be skipped because an older in-flight poll folded
// "running" into the store after the stop.
// P2 (Status): freshness comes from the store snapshot actually committed (nodeStatusAt), never
// from a separate probe that answers while the store's own poll is stuck.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const nodeStart = vi.fn<() => Promise<void>>();
const nodeStatus = vi.fn();
const grantStatus = vi.fn();
const memoryStart = vi.fn<() => Promise<void>>();
const validatorEarnings = vi.fn();

vi.mock("../../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: {
      start: () => nodeStart(),
      stop: async () => {},
      startIpfs: async () => {},
      status: () => nodeStatus(),
      logs: async () => [],
      armMining: async () => false,
      validatorEarnings: () => validatorEarnings(),
    },
    membership: { grantStatus: () => grantStatus() },
    memory: { start: () => memoryStart(), stop: async () => {}, status: async () => ({ state: "stopped" }) },
  },
}));
vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "../../shell/store";
import { liveServiceControls } from "./NodeServices";
import { restartService } from "./services";

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
  (store as unknown as { importKnowledge: () => Promise<void> }).importKnowledge = async () => {};
  return store;
}

beforeEach(() => {
  nodeStart.mockReset();
  nodeStatus.mockReset();
  grantStatus.mockReset();
  memoryStart.mockReset();
  validatorEarnings.mockReset();
  validatorEarnings.mockRejectedValue(new Error("proposer key not minted yet"));
  grantStatus.mockRejectedValue(new Error("no rpc in tests"));
  nodeStatus.mockResolvedValue(RUNNING);
});
afterEach(() => {
  vi.useRealTimers();
});

describe("Services › live adapters report what really happened", () => {
  it("a failed node start rejects with the supervisor's reason (not 'Started.')", async () => {
    const store = newStore();
    store.setState({ node: "off" });
    nodeStart.mockRejectedValue("No space left on device");
    await expect(liveServiceControls(store).node.start()).rejects.toThrow("No space left on device");
    expect(store.state.node).toBe("error");
  });

  it("a node start resolves only after node_start answered", async () => {
    const store = newStore();
    store.setState({ node: "off" });
    let answer: (() => void) | null = null;
    nodeStart.mockImplementation(() => new Promise<void>((r) => (answer = r)));
    let done = false;
    const p = liveServiceControls(store).node.start().then(() => (done = true));
    await Promise.resolve();
    await Promise.resolve();
    expect(done).toBe(false);
    answer?.();
    await p;
    expect(done).toBe(true);
  });

  it("an older poll that folded 'running' after the stop does not turn the start into a no-op", async () => {
    const store = newStore();
    // Restart confirmed the node stopped with its own status read; a poll from before the stop
    // then folded "synced" into the store.
    store.setState({ node: "synced", height: 510_030 });
    nodeStart.mockResolvedValue(undefined);
    await liveServiceControls(store).node.start();
    expect(nodeStart).toHaveBeenCalledTimes(1);
  });

  it("restartService with the live node adapter says the start failed", async () => {
    const store = newStore();
    store.setState({ node: "synced", height: 510_030 });
    nodeStart.mockRejectedValue(new Error("RocksDB LOCK held"));
    const out = await restartService("node", liveServiceControls(store), async () => "stopped", { pollMs: 1, timeoutMs: 50 }, "alive");
    expect(out.ok).toBe(false);
    expect(out.message).toBe("Start failed: RocksDB LOCK held");
  });

  it("a failed memory start rejects with its error (it no longer resolves as if it started)", async () => {
    const store = newStore();
    memoryStart.mockRejectedValue(new Error("BinaryNotFound: mem-mcp"));
    await expect(liveServiceControls(store).memory.start()).rejects.toThrow("BinaryNotFound: mem-mcp");
    expect(store.state.memDaemon).toBe("error");
  });

  it("startMemoryDaemon still records its failure in state for the Storage surface", async () => {
    const store = newStore();
    memoryStart.mockRejectedValue(new Error("BinaryNotFound: mem-mcp"));
    await store.startMemoryDaemon();
    expect(store.state.memDaemon).toBe("error");
    expect(store.state.memDaemonError).toContain("BinaryNotFound");
  });

  it("startNode keeps its idempotent, fire-and-forget behaviour for its other callers", async () => {
    const store = newStore();
    store.setState({ node: "synced", height: 510_030 });
    store.startNode();
    await Promise.resolve();
    expect(nodeStart).not.toHaveBeenCalled();
    expect(store.state.nodeIntent).toBe("run");
  });
});

describe("Status › freshness is the committed store snapshot", () => {
  it("a completed node poll records when its node_status answer was read", async () => {
    const store = newStore();
    store.setState({ node: "synced", hasGrant: true, memberBond: { deployed: false, activated: null, pubkey: null } });
    const before = Date.now();
    await store.refreshNode();
    expect(store.state.nodeStatusAt).not.toBeNull();
    expect(store.state.nodeStatusAt as number).toBeGreaterThanOrEqual(before);
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

  it("is never persisted or restored", async () => {
    const { PERSIST_KEYS, OBSERVED_NOT_PERSISTED, freshState } = await import("../../shell/state");
    expect(PERSIST_KEYS).not.toContain("nodeStatusAt");
    expect(OBSERVED_NOT_PERSISTED).toContain("nodeStatusAt");
    expect(freshState("p1").nodeStatusAt).toBeNull();
  });
});

describe("Rewards › the store's rewardsOf read (contract rule 2: unknown is not zero)", () => {
  const validator = { bondedStake: 32_000, memberBond: { deployed: true, activated: true, pubkey: "0x" + "aa".repeat(48), unlocked: false } };

  it("a well-formed read lands in state and clears the error", async () => {
    const store = newStore();
    store.setState({ ...validator, rewardsError: "earlier failure" });
    validatorEarnings.mockResolvedValue({ totalWei: "30000000000000000000", claimableWei: "20000000000000000000", proposerPubkey: "0x" + "aa".repeat(48) });
    await store.refreshEarnings();
    expect(store.state.rewards).toEqual({ totalWei: "30000000000000000000", claimableWei: "20000000000000000000" });
    expect(store.state.rewardsError).toBeNull();
  });

  it("an empty or malformed figure is unknown, never 0, and says why", async () => {
    for (const bad of [{ totalWei: "", claimableWei: "0" }, { totalWei: "0", claimableWei: "-1" }, { totalWei: "0x10", claimableWei: "0" }, { totalWei: "1.5", claimableWei: "0" }]) {
      const store = newStore();
      store.setState(validator);
      validatorEarnings.mockResolvedValue({ ...bad, proposerPubkey: "0x" + "aa".repeat(48) });
      await store.refreshEarnings();
      expect(store.state.rewards, JSON.stringify(bad)).toBeNull();
      expect(store.state.rewardsError, JSON.stringify(bad)).toMatch(/unreadable/);
    }
  });

  it("a failed read keeps its reason in state for Node › Rewards", async () => {
    const store = newStore();
    store.setState(validator);
    validatorEarnings.mockRejectedValue("rpc 503");
    await store.refreshEarnings();
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toBe("rpc 503");
  });

  it("not read at all for a member whose rewards aren't read through the bond; no stale error", async () => {
    const store = newStore();
    store.setState({ bondedStake: 0, memberBond: null, rewardsError: "earlier failure", rewards: { totalWei: "1", claimableWei: "1" } });
    await store.refreshEarnings();
    expect(validatorEarnings).not.toHaveBeenCalled();
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toBeNull();
  });

  it("rewardsError is observed, never persisted", async () => {
    const { PERSIST_KEYS, freshState } = await import("../../shell/state");
    expect(PERSIST_KEYS).not.toContain("rewardsError");
    expect(freshState("p1").rewardsError).toBeNull();
  });
});
