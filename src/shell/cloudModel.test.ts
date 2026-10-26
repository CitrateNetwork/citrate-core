// D-11 (P-5) — when Hermes's loop runs on the member's cloud model: only with a choice, the sidecar
// loop on, the desktop app, and never under Local model only. A stored choice is two short ids
// (never the Cognition delegate); the monitor files the session as a cloud model, not local.
import { describe, it, expect } from "vitest";
import { cloudModelFor } from "./store";
import { validCloudModel } from "./state";
import { providerClass, spendFor } from "../popout/monitorSnapshot";

const C = { providerId: "openai", model: "gpt-4o" };

describe("Hermes on a cloud model (D-11)", () => {
  it("runs on the cloud model only when chosen, allowed and in the desktop app", () => {
    expect(cloudModelFor(C, "ask_first", true, "tauri")).toEqual(C);
    expect(cloudModelFor(C, "local_only", true, "tauri")).toBeNull();
    expect(cloudModelFor(C, "ask_first", false, "tauri")).toBeNull();
    expect(cloudModelFor(C, "ask_first", true, "sim")).toBeNull();
    expect(cloudModelFor(null, "ask_first", true, "tauri")).toBeNull();
  });

  it("a stored choice is validated on load", () => {
    expect(validCloudModel(C)).toEqual(C);
    for (const bad of [null, "openai", { providerId: "openai" }, { providerId: "", model: "m" }, { providerId: "cognition", model: "devin" }, { providerId: "x".repeat(65), model: "m" }]) {
      expect(validCloudModel(bad)).toBeNull();
    }
  });

  it("the monitor files a cloud session as a cloud model billed by the provider", () => {
    expect(providerClass("sidecar-cloud")).toBe("cloud");
    expect(providerClass("sidecar")).toBe("local");
    expect(spendFor("sidecar-cloud").note).toMatch(/Settings › Models/);
  });
});
