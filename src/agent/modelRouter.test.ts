// Hermes ModelRouter core — the runnable proof of the invariants modeled in
// src-tauri/formal/ModelRouter.tla (P0 / WP0.1).
import { describe, it, expect } from "vitest";
import {
  enumerateChoices,
  resolveActive,
  canSelect,
  gatewayChoice,
  GATEWAY_ID,
  type ModelChoice,
} from "./modelRouter";

describe("ModelRouter.enumerateChoices — three sources, one list", () => {
  it("merges local (ready) + registry (not ready) + the always-ready gateway", () => {
    const choices = enumerateChoices({
      local: [{ id: "loc1", label: "Gemma (local)" }],
      registry: [{ id: "reg1", label: "Registry model" }],
    });
    expect(choices.find((c) => c.id === "loc1")).toMatchObject({ source: "local", ready: true });
    expect(choices.find((c) => c.id === "reg1")).toMatchObject({ source: "registry", ready: false });
    expect(choices.find((c) => c.source === "gateway")).toMatchObject({ id: GATEWAY_ID, ready: true });
  });

  it("de-dupes a registry model that is already local — surfaced ONCE as the ready local choice", () => {
    const choices = enumerateChoices({
      local: [{ id: "shared", label: "Shared (local)" }],
      registry: [{ id: "shared", label: "Shared (registry)" }],
    });
    const shared = choices.filter((c) => c.id === "shared");
    expect(shared).toHaveLength(1);
    expect(shared[0]).toMatchObject({ source: "local", ready: true });
  });

  it("reflects an unconfigured gateway (ready:false) when told so", () => {
    const choices = enumerateChoices({ local: [], registry: [], gatewayReady: false });
    expect(gatewayChoice(choices).ready).toBe(false);
  });
});

describe("ModelRouter — INV-Router-2: never serve a not-ready model", () => {
  const choices: ModelChoice[] = [
    { id: "loc1", label: "L", source: "local", ready: true },
    { id: "reg1", label: "R", source: "registry", ready: false },
    { id: GATEWAY_ID, label: "GW", source: "gateway", ready: true },
  ];

  it("resolves a READY active to itself", () => {
    expect(resolveActive("loc1", choices).id).toBe("loc1");
  });

  it("falls back to the gateway when the active is NOT ready (registry not yet pulled)", () => {
    expect(resolveActive("reg1", choices).id).toBe(GATEWAY_ID);
  });

  it("falls back to the gateway when there is no active (default / out-of-box)", () => {
    expect(resolveActive(null, choices).id).toBe(GATEWAY_ID);
  });

  it("falls back to the gateway for an unknown (phantom) active id — never a fabricated backend", () => {
    expect(resolveActive("does-not-exist", choices).id).toBe(GATEWAY_ID);
  });

  it("the resolved backend is ALWAYS ready and ALWAYS an enumerated choice (INV-2 + INV-3)", () => {
    for (const activeId of [null, "loc1", "reg1", GATEWAY_ID, "phantom"]) {
      const resolved = resolveActive(activeId, choices);
      expect(resolved.ready).toBe(true);
      expect(choices).toContainEqual(resolved);
    }
  });
});

describe("ModelRouter — INV-Router-3: only enumerated choices are selectable (no phantom)", () => {
  const choices = enumerateChoices({ local: [{ id: "loc1", label: "L" }], registry: [] });
  it("accepts a real enumerated id", () => {
    expect(canSelect("loc1", choices)).toBe(true);
    expect(canSelect(GATEWAY_ID, choices)).toBe(true);
  });
  it("rejects a phantom id", () => {
    expect(canSelect("nope", choices)).toBe(false);
  });
});
