// L0-WP02 SEAM-BRIDGE: the assembled bridge (`bridge` from src/bridge/index.ts) in both modes,
// recorded at a826e441: every domain, every method and its arity. L0-WP05 splits domains.ts into
// per-domain files and adds the lane namespaces; the existing keys and methods must stay identical.
// The tauri bridge is built with `invoke` replaced by a recorder: building it calls no command.
import { afterEach, describe, expect, it, vi } from "vitest";
import { SEAMS_BASE, checkSnapshot } from "../../../scripts/seams-snapshot.mjs";

/** Own and inherited member names of an object (not Object.prototype's). */
function names(o: object): string[] {
  const out = new Set<string>();
  for (let p: object | null = o; p && p !== Object.prototype; p = Object.getPrototypeOf(p)) {
    for (const k of Object.getOwnPropertyNames(p)) if (k !== "constructor") out.add(k);
  }
  return [...out].sort();
}

/** Each member as `fn/<arity>`, a nested domain, or its type. */
function shape(v: unknown, depth = 0): unknown {
  if (typeof v === "function") return `fn/${v.length}`;
  if (Array.isArray(v)) return "array";
  if (v !== null && typeof v === "object" && depth < 4) {
    const out: Record<string, unknown> = {};
    for (const k of names(v)) out[k] = shape((v as Record<string, unknown>)[k], depth + 1);
    return out;
  }
  return v === null ? "null" : typeof v;
}

function surface(b: { mode: string }) {
  const s = shape(b) as Record<string, unknown>;
  return { mode: b.mode, domains: Object.keys(s).length, shape: s };
}

describe("SEAM-BRIDGE: bridge keys and methods", () => {
  afterEach(() => {
    vi.doUnmock("@tauri-apps/api/core");
    vi.resetModules();
  });

  it("sim_bridge_keys_and_methods_are_unchanged", async () => {
    vi.resetModules();
    const { bridge } = await import("../index");
    expect(bridge.mode).toBe("sim");
    const got = surface(bridge);
    expect(got.domains).toBeGreaterThan(30);
    expect(checkSnapshot(`fixtures/seams/bridge-sim.${SEAMS_BASE}.json`, got)).toEqual([]);
  });

  it("tauri_bridge_keys_and_methods_are_unchanged", async () => {
    vi.resetModules();
    const calls: string[] = [];
    vi.doMock("@tauri-apps/api/core", async (importOriginal) => ({
      ...((await importOriginal()) as object),
      isTauri: () => true,
      invoke: (cmd: string) => {
        calls.push(cmd);
        return Promise.reject(new Error(`seams: ${cmd} invoked while building the bridge`));
      },
    }));
    const { bridge } = await import("../index");
    expect(bridge.mode).toBe("tauri");
    const got = surface(bridge);
    expect(got.domains).toBeGreaterThan(30);
    expect(calls, "building the tauri bridge invokes no command").toEqual([]);
    expect(checkSnapshot(`fixtures/seams/bridge-tauri.${SEAMS_BASE}.json`, got)).toEqual([]);
  });
});
