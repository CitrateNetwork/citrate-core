// L7-WP01 characterization (suite L7-CH-3, ci/characterization.d/l7.json): the member's mesh switch
// model as it ships, for each status core can return. L7-WP11 replaces these with stricter tests
// when the private-link branch lands; until then they stay green unchanged.
import { describe, expect, it } from "vitest";
import type { MeshStatus } from "../bridge/domains";
import { meshSwitchModel } from "./meshSwitch";

const status = (on: boolean, source: MeshStatus["source"], signedOff: boolean): MeshStatus => ({
  on,
  source,
  signedOff,
  note: "note from core",
});

describe("L7-CH-3 meshSwitchModel today", () => {
  it("char_unloaded_status_is_unusable", () => {
    expect(meshSwitchModel(null)).toEqual({ enabled: false, on: false, action: null, reason: "Checking the mesh…" });
  });

  it("char_unsigned_build_cannot_be_changed", () => {
    const unsigned = "You can turn the mesh on or off here once its security review is signed off.";
    expect(meshSwitchModel(status(false, "off", false))).toEqual({ enabled: false, on: false, action: null, reason: unsigned });
    // An operator's env can turn the mesh on in an unsigned build; the switch still cannot change it.
    expect(meshSwitchModel(status(true, "operator", false))).toEqual({ enabled: false, on: true, action: null, reason: unsigned });
  });

  it("char_operator_setting_cannot_be_changed", () => {
    expect(meshSwitchModel(status(true, "operator", true))).toEqual({
      enabled: false,
      on: true,
      action: null,
      reason: "An operator setting on this machine turns the mesh on; it cannot be changed here.",
    });
  });

  it("char_signed_build_offers_the_member_switch", () => {
    const next = "A change applies the next time the cluster starts.";
    expect(meshSwitchModel(status(true, "default", true))).toEqual({ enabled: true, on: true, action: "turn-off", reason: next });
    expect(meshSwitchModel(status(false, "off", true))).toEqual({ enabled: true, on: false, action: "turn-on", reason: next });
  });
});
