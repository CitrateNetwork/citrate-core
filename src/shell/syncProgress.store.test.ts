// 0.5.2 RC52 — Store.refreshNode folds sync progress (syncProgress.ts) from its two Rule-7 sources:
// the local height from node_status (local eth_blockNumber) and the public head from
// ChainReads.publicHead (eth_blockNumber on the network profile's RPC), read at most every
// HEAD_READ_MS. A failed head read is unknown, never 0 and never Synced.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { store } from "./store";
import { bridge } from "../bridge";
import type { ChainReads } from "./chainTruth";
import { HEAD_READ_MS } from "./syncProgress";

const T0 = 1_800_000_000_000;
let now = T0;
const flush = () => new Promise((r) => setTimeout(r, 0));

function wire(publicHead: () => Promise<number | null>) {
  const reads = {
    validatorActivation: async () => null,
    minStakeWei: async () => 0n,
    blockSeconds: async () => 2,
    producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
    publicHead: vi.fn(publicHead),
  } as unknown as ChainReads;
  (store as unknown as { chainReads: ChainReads }).chainReads = reads;
  (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
  return reads as unknown as { publicHead: ReturnType<typeof vi.fn> };
}

async function tick(height: number, state = "running") {
  now += 2000;
  vi.spyOn(bridge.node, "status").mockResolvedValue({ state, peers: 3, height, syncPct: 50 });
  await store.refreshNode();
  await flush();
}

beforeEach(() => {
  now = T0;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  // The tracker's window runs on the monotonic clock (#566 review P2).
  vi.spyOn(performance, "now").mockImplementation(() => now);
  vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
  vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: "" });
  // Reset the store's sync window between tests by a stopped poll.
  store.setState({ node: "off", hasGrant: true, bondedStake: 32_000, bondActivated: true, syncProgress: null });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("refreshNode folds sync progress", () => {
  it("measures the catch-up from the local height and the profile RPC head, and gives an ETA", async () => {
    let head = 200_000;
    const reads = wire(async () => head);
    await tick(0, "stopped"); // clears any window
    let local = 100_000;
    for (let i = 0; i < 40; i++) {
      head += 1; // ~0.5 blocks/s at 2 s per tick
      local += 80; // ~40 blocks/s
      await tick(local);
    }
    const p = store.state.syncProgress;
    expect(p?.kind).toBe("syncing");
    if (p?.kind !== "syncing") return;
    expect(p.phase).toBe("catching-up");
    expect(p.rate).toBeGreaterThan(38);
    expect(p.rate).toBeLessThan(41);
    expect(p.etaSec).not.toBeNull();
    // Throttled: about one head read per HEAD_READ_MS, not one per 2 s tick.
    expect(reads.publicHead.mock.calls.length).toBeLessThanOrEqual(Math.ceil((40 * 2000) / HEAD_READ_MS) + 1);
    // At least one read per HEAD_READ_MS rounded up to the next 2 s tick.
    expect(reads.publicHead.mock.calls.length).toBeGreaterThanOrEqual(Math.floor((40 * 2000) / (HEAD_READ_MS + 2000)));
  });

  it("a head read that fails leaves the head unknown: no behind, no rate, no ETA, never Synced", async () => {
    wire(async () => {
      throw new Error("rpc 503");
    });
    await tick(0, "stopped");
    for (let i = 0; i < 40; i++) await tick(100_000 + i * 80);
    expect(store.state.syncProgress).toEqual({ kind: "unknown", missing: "head" });
  });

  it("a head read of 0x0 is unknown, not block 0", async () => {
    wire(async () => null);
    await tick(0, "stopped");
    for (let i = 0; i < 10; i++) await tick(100_000 + i * 80);
    expect(store.state.syncProgress).toEqual({ kind: "unknown", missing: "head" });
  });

  it("a stopped node restarts the window: the local height is unknown", async () => {
    let head = 200_000;
    wire(async () => (head += 2));
    for (let i = 0; i < 20; i++) await tick(100_000 + i * 80);
    expect(store.state.syncProgress?.kind).toBe("syncing");
    await tick(0, "stopped");
    expect(store.state.syncProgress).toEqual({ kind: "unknown", missing: "local" });
  });

  it("the wall clock set back an hour does not freeze the line (the window is monotonic)", async () => {
    let head = 200_000;
    wire(async () => (head += 2));
    await tick(0, "stopped");
    let local = 100_000;
    for (let i = 0; i < 40; i++) await tick((local += 80));
    expect(store.state.syncProgress?.kind === "syncing" && store.state.syncProgress.phase).toBe("catching-up");
    // Wall time jumps back 1 h; monotonic time keeps going. The node makes no progress for 70 s.
    vi.spyOn(Date, "now").mockImplementation(() => now - 3_600_000);
    for (let i = 0; i < 35; i++) await tick(local);
    const p = store.state.syncProgress;
    expect(p?.kind === "syncing" && p.phase).toBe("stalled");
  });

  it("the store's stalled node state uses STALL_MS (60 s) on the monotonic clock (#569 review P2)", async () => {
    wire(async () => 300_000);
    await tick(0, "stopped");
    store.setState({ node: "syncing" });
    // The first frozen tick starts the timer; wall time then jumps back an hour.
    await tick(100_000);
    await tick(100_000);
    const start = now;
    vi.spyOn(Date, "now").mockImplementation(() => now - 3_600_000);
    while (now - start < 58_000) await tick(100_000);
    expect(store.state.node).toBe("syncing");
    await tick(100_000); // 60 s frozen with peers: contract §1 Sync stalled
    expect(now - start).toBe(60_000);
    expect(store.state.node).toBe("stalled");
  });

  it("passes the peer count: 0 peers for 60 s reads No peers", async () => {
    wire(async () => 300_000);
    await tick(0, "stopped");
    for (let i = 0; i < 32; i++) {
      now += 2000;
      vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 0, height: 100_000, syncPct: 50 });
      await store.refreshNode();
      await flush();
    }
    expect(store.state.syncProgress?.kind === "syncing" && store.state.syncProgress.phase).toBe("no-peers");
  });

  it("a running node whose RPC answers 0x0 adds no reading (unknown is not zero)", async () => {
    wire(async () => 200_000);
    await tick(0, "stopped");
    for (let i = 0; i < 5; i++) await tick(0);
    expect(store.state.syncProgress).toEqual({ kind: "unknown", missing: "local" });
  });
});
