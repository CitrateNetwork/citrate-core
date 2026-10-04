// HUP-S8.1 hardening — removing a device is two steps: core mints a confirmation for one device,
// and only that confirmation id revokes it. Nothing revokes by naming a device.
import { describe, it, expect, vi, afterEach } from "vitest";
import { bridge } from "../../bridge";
import { cancelRevokeDevice, clusterSlice, prepareRevokeDevice, revokeMyDevice } from "./cluster";

const DEV = "ab".repeat(20);
const prepared = { confirmId: "c-1", device: DEV, statement: "Remove device 0xabab… for good.", confirmBy: 0 };
const links = { thisDevice: null, links: [], revoked: [] };

afterEach(() => {
  vi.restoreAllMocks();
  clusterSlice.set({ revokePrepared: null, revoking: null, error: null });
});

describe("removing a device", () => {
  it("prepares first, then revokes with core's confirmation id", async () => {
    const prep = vi.spyOn(bridge.cluster, "revokeDevicePrepare").mockResolvedValue(prepared);
    const rev = vi.spyOn(bridge.cluster, "revokeDevice").mockResolvedValue(links);
    await prepareRevokeDevice("0x" + DEV);
    expect(prep).toHaveBeenCalledWith("0x" + DEV);
    expect(rev).not.toHaveBeenCalled();
    expect(clusterSlice.get().revokePrepared?.statement).toMatch(/for good/);
    await revokeMyDevice();
    expect(rev).toHaveBeenCalledWith("c-1");
    expect(clusterSlice.get().revokePrepared).toBeNull();
  });

  it("without a prepared removal, nothing is revoked; Keep drops it", async () => {
    const rev = vi.spyOn(bridge.cluster, "revokeDevice").mockResolvedValue(links);
    await revokeMyDevice();
    expect(rev).not.toHaveBeenCalled();
    vi.spyOn(bridge.cluster, "revokeDevicePrepare").mockResolvedValue(prepared);
    await prepareRevokeDevice(DEV);
    cancelRevokeDevice();
    await revokeMyDevice();
    expect(rev).not.toHaveBeenCalled();
  });
});
