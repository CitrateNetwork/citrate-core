// L0-WP05 (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3): the per-lane bridge namespaces.
// Each 0.6.0 lane programs against its own `bridge.<key>`, declared in src/bridge/lanes/l<N>.ts and
// implemented in src/bridge/tauri/l<N>.ts and src/bridge/sim/l<N>.ts (files only that lane edits).
// The namespace exists in both modes from the start, the tauri build selects the tauri impl, the
// two modes expose the same members, and no lane file imports another lane's file.
import { afterEach, describe, expect, it, vi } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, normalize, relative } from "node:path";
import { fileURLToPath } from "node:url";

/** Lane -> bridge key (L0 section 3.3). Written out here, independent of the code under test. */
const LANE_KEYS: Record<string, string> = {
  L1: "acp",
  L2: "hic",
  L3: "notify",
  L4: "conversations",
  L5: "media",
  L6: "mcpCatalog",
  L7: "net",
  L8: "trainTogether",
  L9: "pqc",
};
const LANES = Object.keys(LANE_KEYS).map((l) => l.toLowerCase());
/** Building the whole bridge (both modes, fresh module graphs) is slow on a loaded runner. */
const BUILDS_THE_BRIDGE = 30_000;

const BRIDGE_DIR = join(dirname(fileURLToPath(import.meta.url)), "..");
const ROOT = join(BRIDGE_DIR, "..", "..");

type Bridge = Record<string, unknown> & { mode: string };

async function simBridge(): Promise<Bridge> {
  vi.resetModules();
  const { bridge } = await import("../index");
  expect(bridge.mode).toBe("sim");
  return bridge as unknown as Bridge;
}

/** The tauri bridge, built with `invoke` replaced by a recorder (as SEAM-BRIDGE does). */
async function tauriBridge(): Promise<{ bridge: Bridge; calls: string[] }> {
  vi.resetModules();
  const calls: string[] = [];
  vi.doMock("@tauri-apps/api/core", async (importOriginal) => ({
    ...((await importOriginal()) as object),
    isTauri: () => true,
    invoke: (cmd: string) => {
      calls.push(cmd);
      return Promise.reject(new Error(`bridgeLanes: ${cmd} invoked while building the bridge`));
    },
  }));
  const { bridge } = await import("../index");
  expect(bridge.mode).toBe("tauri");
  return { bridge: bridge as unknown as Bridge, calls };
}

/** Member names of one namespace, sorted. */
function members(ns: unknown): string[] {
  return Object.keys(ns as object).sort();
}

/** Every module specifier a TS file imports or re-exports, statically or dynamically. */
function specifiers(src: string): string[] {
  const out: string[] = [];
  const re = /(?:^|[\s;])(?:import|export)\b[^"'`;]*?\bfrom\s*["']([^"']+)["']|\bimport\s*\(\s*["']([^"']+)["']\s*\)|^\s*import\s*["']([^"']+)["']/gm;
  for (const m of src.matchAll(re)) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

describe("L0-WP05: v060 lane bridge namespaces", () => {
  afterEach(() => {
    vi.doUnmock("@tauri-apps/api/core");
    vi.resetModules();
  });

  it("v60_bridge_has_every_lane_key", async () => {
    const { V60_LANE_KEYS } = await import("../lanes");
    expect(V60_LANE_KEYS).toEqual(LANE_KEYS);
    // Every key is new: none shadows a legacy or CX domain recorded at a826e441.
    const recorded = JSON.parse(readFileSync(join(ROOT, "fixtures/seams/bridge-sim.a826e441.json"), "utf8"));
    for (const key of Object.values(LANE_KEYS)) expect(Object.keys(recorded.shape), key).not.toContain(key);

    const sim = await simBridge();
    const { bridge: tauri, calls } = await tauriBridge();
    for (const [mode, b] of [["sim", sim], ["tauri", tauri]] as const) {
      for (const key of Object.values(LANE_KEYS)) {
        expect(typeof b[key], `${mode} bridge.${key}`).toBe("object");
        expect(b[key], `${mode} bridge.${key}`).not.toBeNull();
      }
    }
    expect(calls, "building the lane namespaces invokes no command").toEqual([]);
  }, BUILDS_THE_BRIDGE);

  it("v60_tauri_build_selects_the_tauri_lane_impls", async () => {
    // sharp-edges: a sim impl must never be selected in a Tauri build. Each tauri bridge.<key> is
    // the very object src/bridge/tauri/l<N>.ts exports; the sim bridge's is a different object.
    const { bridge: tauri } = await tauriBridge();
    const impls = import.meta.glob("../tauri/l[1-9].ts");
    expect(Object.keys(impls).sort()).toEqual(LANES.map((l) => `../tauri/${l}.ts`));
    const tauriExport = async (lane: string) => {
      const mod = (await impls[`../tauri/${lane}.ts`]()) as Record<string, unknown>;
      const exported = Object.values(mod).filter((v) => typeof v === "object" && v !== null);
      expect(exported, `tauri/${lane}.ts exports one namespace object`).toHaveLength(1);
      return exported[0];
    };
    for (const lane of LANES) {
      const key = LANE_KEYS[lane.toUpperCase()];
      expect(tauri[key], `tauri bridge.${key}`).toBe(await tauriExport(lane));
    }
    // In the sim build (same module registry) the namespace is the sim impl, not the tauri object.
    vi.doUnmock("@tauri-apps/api/core");
    const sim = await simBridge();
    for (const lane of LANES) {
      const key = LANE_KEYS[lane.toUpperCase()];
      expect(sim[key], `sim bridge.${key}`).not.toBe(await tauriExport(lane));
    }
  }, BUILDS_THE_BRIDGE);

  it("v60_lane_namespaces_have_the_same_members_in_both_modes", async () => {
    const sim = await simBridge();
    const { bridge: tauri } = await tauriBridge();
    for (const key of Object.values(LANE_KEYS)) {
      expect(members(sim[key]), `bridge.${key}: sim and tauri expose the same members`).toEqual(members(tauri[key]));
      for (const m of members(tauri[key])) {
        expect(typeof (tauri[key] as Record<string, unknown>)[m], `tauri bridge.${key}.${m}`).toBe("function");
      }
    }
  }, BUILDS_THE_BRIDGE);

  it("lane_files_do_not_import_each_other", () => {
    const problems: string[] = [];
    for (const dir of ["lanes", "tauri", "sim"]) {
      for (const lane of LANES) {
        const file = join(BRIDGE_DIR, dir, `${lane}.ts`);
        const rel = relative(ROOT, file);
        if (!existsSync(file)) {
          problems.push(`${rel} is missing`);
          continue;
        }
        const src = readFileSync(file, "utf8");
        for (const spec of specifiers(src)) {
          if (!spec.startsWith(".")) continue;
          const target = relative(BRIDGE_DIR, normalize(join(dirname(file), spec))).replace(/\.ts$/, "");
          const m = /^(lanes|tauri|sim)\/(l\d)$/.exec(target);
          if (m && m[2] !== lane) problems.push(`${rel} imports ${m[0]} (another lane's file)`);
          if (dir === "tauri" && /^sim(\/|$)/.test(target)) problems.push(`${rel} imports the sim adapter (${spec})`);
          if (dir === "sim" && /^tauri(\/|$)/.test(target)) problems.push(`${rel} imports the tauri adapter (${spec})`);
        }
        // The namespace file declares types only: nothing in it runs.
        if (dir === "lanes") {
          for (const line of src.split("\n")) {
            if (/^\s*import\s+(?!type\b)/.test(line)) problems.push(`${rel} has a value import: ${line.trim()}`);
            if (/^\s*export\s+(?!(type|interface)\b)/.test(line)) problems.push(`${rel} exports a value: ${line.trim()}`);
          }
        }
      }
    }
    // The scan itself sees a cross-lane import (mutation check of this test).
    expect(specifiers('import type { HicDomain } from "./l2";\nexport { x } from "../sim/l3";')).toEqual(["./l2", "../sim/l3"]);
    expect(problems).toEqual([]);
  });
});
