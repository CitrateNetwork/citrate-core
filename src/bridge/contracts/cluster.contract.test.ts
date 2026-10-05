// CX bridge contract — cluster (C-20). Pins the frozen shape (CX-S0.2).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — cluster (frozen CX-S0.2)", () => {
  it("exposes the cluster domain with its frozen methods", () => {
    expect(bridge.cluster).toBeDefined();
    for (const m of [
      "status",
      "join",
      "peers",
      "shareFile",
      "leave",
      // HUP-S8.1
      "devices",
      "myDevices",
      "linkDeviceRequest",
      "linkDeviceApprove",
      "linkDeviceReject",
      "revokeDevicePrepare",
      "revokeDevice",
      "exportDeviceLink",
      "importDeviceLink",
      // HUP-S8.1 follow-on: other members' links over the group relay
      "deviceLinksShareOffer",
      "deviceLinksMarkShared",
      "deviceLinksIngest",
      // HUP-S8.4 prep: mesh transport policy
      "meshStatus",
      // HUP-S8.4: group links (seeds)
      "groupSeed",
      "addGroupSeed",
    ] as const) {
      expect(typeof bridge.cluster[m]).toBe("function");
    }
  });
  it("sim is honest-empty (no fabricated peers, Rule 1)", async () => {
    if (bridge.mode === "sim") {
      const s = await bridge.cluster.status("g");
      expect(s.online).toBe(0);
      expect(await bridge.cluster.peers("g")).toEqual([]);
    }
  });
  it("sim device linking is honest-empty and refuses to fake a link (HUP-S8.1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.cluster.devices("g")).toEqual([]);
      expect(await bridge.cluster.myDevices()).toEqual({ thisDevice: null, links: [], revoked: [] });
      await expect(bridge.cluster.linkDeviceRequest("laptop")).rejects.toThrow(/desktop app/);
      await expect(bridge.cluster.revokeDevicePrepare("aa")).rejects.toThrow(/desktop app/);
      await expect(bridge.cluster.revokeDevice("c-1")).rejects.toThrow(/desktop app/);
      await expect(bridge.cluster.exportDeviceLink()).rejects.toThrow(/desktop app/);
      await expect(bridge.cluster.importDeviceLink("{}")).rejects.toThrow(/desktop app/);
      // Nothing to share and nothing accepted in the sim: no fabricated links.
      expect(await bridge.cluster.deviceLinksShareOffer("g")).toBeNull();
      expect(await bridge.cluster.deviceLinksIngest("aa", "cdlink1:{}")).toEqual({ links: 0, revocations: 0, refused: [] });
      const mesh = await bridge.cluster.meshStatus();
      expect(mesh.on).toBe(false);
      // HUP-S8.4: no daemon in the sim, so no group link is invented.
      await expect(bridge.cluster.groupSeed("g")).rejects.toThrow(/desktop app/);
      await expect(bridge.cluster.addGroupSeed("g", "citrate-cluster://seed?v=1")).rejects.toThrow(/desktop app/);
    }
  });
});
