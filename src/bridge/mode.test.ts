// CORE-A1 A1.5 — bridge mode selection.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

describe("bridge mode selection", () => {
  beforeEach(() => {
    vi.resetModules();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    // remove any injected internals from earlier tests
    delete (globalThis as Record<string, unknown>).__TAURI_INTERNALS__;
    if (typeof window !== "undefined") {
      delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    }
  });

  it("resolves to sim on the plain web/dev path (no Tauri internals)", async () => {
    const { BRIDGE_MODE } = await import("./mode");
    expect(BRIDGE_MODE).toBe("sim");
  });

  it("resolves to tauri when the Tauri internals global is injected", async () => {
    // Tauri v2 injects window.__TAURI_INTERNALS__; the SDK isTauri() reads it.
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: () => Promise.resolve(),
    };
    const { BRIDGE_MODE } = await import("./mode");
    expect(BRIDGE_MODE).toBe("tauri");
  });

  it("assertSimAllowed throws once mode is tauri (packaged-build guard)", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: () => Promise.resolve(),
    };
    const { assertSimAllowed } = await import("./mode");
    expect(() => assertSimAllowed("config.read")).toThrowError(/sim adapter reached in a Tauri build/);
  });

  it("assertSimAllowed is a no-op in sim mode", async () => {
    const { assertSimAllowed } = await import("./mode");
    expect(() => assertSimAllowed("config.read")).not.toThrow();
  });
});
