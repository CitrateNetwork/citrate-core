// HUP-S8.1 — the "Your devices" view model + the shared device-name rule.
import { describe, expect, it } from "vitest";
import { deviceNameError, devicePanelModel, MAX_DEVICE_NAME } from "./clusterDevices";

describe("deviceNameError (same rule as core + cluster-core)", () => {
  it("accepts ordinary names", () => {
    expect(deviceNameError("Studio Mac")).toBeNull();
    expect(deviceNameError("larry's linux-box_2.0")).toBeNull();
    expect(deviceNameError("x".repeat(MAX_DEVICE_NAME))).toBeNull();
  });
  it("refuses what core would refuse", () => {
    expect(deviceNameError("")).not.toBeNull();
    expect(deviceNameError(" padded")).not.toBeNull();
    expect(deviceNameError("two\nlines")).not.toBeNull();
    expect(deviceNameError("member: 0xabc")).not.toBeNull();
    expect(deviceNameError("café")).not.toBeNull();
    expect(deviceNameError("x".repeat(MAX_DEVICE_NAME + 1))).not.toBeNull();
  });
});

describe("devicePanelModel", () => {
  const link = (device: string, index: number, label: string, thisDevice = false) => ({
    device,
    member: "a1",
    wallet: "b1",
    index,
    label,
    issuedAt: 1,
    thisDevice,
  });
  it("is empty and unlinked before anything loads", () => {
    expect(devicePanelModel(null)).toEqual({ thisDeviceLinked: false, thisDeviceLabel: null, rows: [] });
  });
  it("puts this device first, then the rest by index", () => {
    const m = devicePanelModel({
      thisDevice: "d2",
      links: [link("d3", 2, "Box"), link("d1", 0, "Laptop"), link("d2", 1, "Studio", true)],
      revoked: [],
    });
    expect(m.thisDeviceLinked).toBe(true);
    expect(m.thisDeviceLabel).toBe("Studio");
    expect(m.rows.map((r) => r.device)).toEqual(["d2", "d1", "d3"]);
  });
  it("reports this device as unlinked when only other devices are linked", () => {
    const m = devicePanelModel({ thisDevice: null, links: [link("d1", 0, "Laptop")], revoked: [] });
    expect(m.thisDeviceLinked).toBe(false);
    expect(m.rows).toHaveLength(1);
  });
});

// HUP-S8.2: the wizard links a machine under the name it already has; the name is made valid first.
import { deviceNameFrom } from "./clusterDevices";
describe("deviceNameFrom", () => {
  it("keeps a valid name and repairs or replaces one core would refuse", () => {
    expect(deviceNameFrom("Studio Mac")).toBe("Studio Mac");
    expect(deviceNameFrom("  Larry's MacBook Pro (M3)  ")).toBe("Larry's MacBook Pro M3");
    expect(deviceNameFrom("🙂🙂")).toBe("My machine");
    expect(deviceNameFrom("x".repeat(80))).toHaveLength(48);
    for (const n of ["Studio Mac", "  Larry's MacBook Pro (M3)  ", "🙂", "a/b\\c", "x".repeat(80)]) {
      expect(deviceNameError(deviceNameFrom(n))).toBeNull();
    }
  });
});
