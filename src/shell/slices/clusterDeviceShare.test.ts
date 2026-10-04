// HUP-S8.1 follow-on: the cluster slice shares your device links with your groups and picks up other
// members' links from the group relay, without losing any chat message to the drain.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { bridge } from "../../bridge";
import { DEVICE_LINKS_MSG_PREFIX } from "../../bridge/domains";
import { groupsSlice } from "./groups";
import { clusterSlice, importDeviceCode, loadMeshStatus, prepareRevokeDevice, revokeMyDevice, shareMyDeviceLinks, syncPeerDeviceLinks } from "./cluster";

const D = "d".repeat(64);
const share = (n: string) => `${DEVICE_LINKS_MSG_PREFIX}{"v":1,"n":"${n}"}`;
const noDevices = { thisDevice: null, links: [], revoked: [] };

describe("cluster slice: device link sharing", () => {
  beforeEach(() => {
    groupsSlice.set({ history: {} });
    vi.spyOn(bridge.groups, "list").mockResolvedValue([
      { id: "g1", name: "One", kind: "team" } as never,
      { id: "g2", name: "Two", kind: "team" } as never,
    ]);
  });
  afterEach(() => vi.restoreAllMocks());

  it("sends the share to every group that does not have it yet", async () => {
    vi.spyOn(bridge.cluster, "deviceLinksShareOffer").mockImplementation(async (g) => (g === "g1" ? { body: share("x"), digest: D } : null));
    const mark = vi.spyOn(bridge.cluster, "deviceLinksMarkShared").mockResolvedValue(undefined);
    const send = vi.spyOn(bridge.groups, "send").mockResolvedValue(undefined);
    expect(await shareMyDeviceLinks()).toBe(1);
    expect(send).toHaveBeenCalledWith("g1", share("x"));
    expect(mark).toHaveBeenCalledWith("g1", D);
  });

  it("drains the group into its retained history, keeps chat, and ingests the newest share per member", async () => {
    vi.spyOn(bridge.groups, "messages").mockResolvedValue([
      { id: "g1:0", groupId: "g1", sender: "aa", body: share("old"), ts: 0 },
      { id: "g1:1", groupId: "g1", sender: "bb", body: "hello", ts: 0 },
      { id: "g1:2", groupId: "g1", sender: "aa", body: share("new"), ts: 0 },
    ]);
    const ingest = vi.spyOn(bridge.cluster, "deviceLinksIngest").mockResolvedValue({ links: 2, revocations: 1, refused: [] });
    const r = await syncPeerDeviceLinks("g1");
    expect(ingest).toHaveBeenCalledTimes(1);
    expect(ingest).toHaveBeenCalledWith("aa", share("new"));
    expect(r).toEqual({ links: 2, revocations: 1 });
    const kept = (groupsSlice.get().history["g1"] ?? []).map((m) => m.body);
    expect(kept).toContain("hello");
  });

  it("revoking or importing a device shares the new set", async () => {
    const offer = vi.spyOn(bridge.cluster, "deviceLinksShareOffer").mockResolvedValue(null);
    vi.spyOn(bridge.cluster, "revokeDevicePrepare").mockResolvedValue({ confirmId: "c-1", device: "dd", statement: "", confirmBy: 0 });
    const revoke = vi.spyOn(bridge.cluster, "revokeDevice").mockResolvedValue(noDevices);
    vi.spyOn(bridge.cluster, "importDeviceLink").mockResolvedValue(noDevices);
    // Removal is two steps: core prepares the confirmation, the member confirms.
    await prepareRevokeDevice("dd");
    await revokeMyDevice();
    expect(revoke).toHaveBeenCalledWith("c-1");
    await importDeviceCode("{}");
    await vi.waitFor(() => expect(offer).toHaveBeenCalledTimes(4)); // two groups, twice
  });

  it("an unreachable daemon leaves the mesh status empty, never invented", async () => {
    vi.spyOn(bridge.cluster, "meshStatus").mockRejectedValue(new Error("daemon down"));
    await loadMeshStatus();
    expect(clusterSlice.get().mesh).toBeNull();
  });
});
