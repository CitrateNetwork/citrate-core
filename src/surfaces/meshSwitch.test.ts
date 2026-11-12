// V60 MESH-4: the member's on/off control for the cross-machine mesh. Source: `cluster_mesh_status`;
// the change goes through `cluster_mesh_set_enabled`, which core refuses until CL-S4 is signed.
import { describe, expect, it } from "vitest";
import { meshSwitchModel } from "./meshSwitch";

const status = (on: boolean, source: "operator" | "default" | "off", signedOff: boolean) => ({
  on,
  source,
  signedOff,
  note: "n",
});

describe("meshSwitchModel", () => {
  it("is unavailable until the status loads", () => {
    expect(meshSwitchModel(null)).toEqual({ enabled: false, on: false, action: null, reason: "Checking the mesh…" });
  });
  it("cannot be changed before the transport sign-off", () => {
    const m = meshSwitchModel(status(false, "off", false));
    expect(m.enabled).toBe(false);
    expect(m.action).toBeNull();
    expect(m.reason).toContain("security review");
  });
  it("cannot be changed here when an operator setting turns it on", () => {
    const m = meshSwitchModel(status(true, "operator", true));
    expect(m.enabled).toBe(false);
    expect(m.on).toBe(true);
    expect(m.reason).toContain("operator");
  });
  it("offers turn off when on by default, and turn on when the member turned it off", () => {
    expect(meshSwitchModel(status(true, "default", true))).toMatchObject({ enabled: true, on: true, action: "turn-off" });
    expect(meshSwitchModel(status(false, "off", true))).toMatchObject({ enabled: true, on: false, action: "turn-on" });
  });
  it("says a change applies when the cluster next starts", () => {
    expect(meshSwitchModel(status(true, "default", true)).reason).toContain("next time");
  });
});
