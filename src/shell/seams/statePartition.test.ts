// L0-WP06: AppState is composed from one part per lane (src/shell/state/core.ts and l1..l9.ts).
// A field lives in exactly one part, every field lives in some part, and the persisted-key table
// is the union of the parts' tables, in the order the 0.5.9 save wrote it.
// The values themselves are pinned by SEAM-STATE (state.characterization.test.ts).
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { freshState, PERSIST_KEYS, PERSIST_SCOPE, PERSONAS } from "../state";
import { CORE_PERSIST_SCOPE, freshCore } from "../state/core";
import { freshL1, L1_PERSIST_SCOPE } from "../state/l1";
import { freshL2, L2_PERSIST_SCOPE } from "../state/l2";
import { freshL3, L3_PERSIST_SCOPE } from "../state/l3";
import { freshL4, L4_PERSIST_SCOPE } from "../state/l4";
import { freshL5, L5_PERSIST_SCOPE } from "../state/l5";
import { freshL6, L6_PERSIST_SCOPE } from "../state/l6";
import { freshL7, L7_PERSIST_SCOPE } from "../state/l7";
import { freshL8, L8_PERSIST_SCOPE } from "../state/l8";
import { freshL9, L9_PERSIST_SCOPE } from "../state/l9";
import { ROOT, SEAMS_BASE } from "../../../scripts/seams-snapshot.mjs";

type Part = { lane: string; fresh: (pid: string) => object; persist: Record<string, string> };

const PARTS: Part[] = [
  { lane: "core", fresh: freshCore, persist: CORE_PERSIST_SCOPE },
  { lane: "l1", fresh: freshL1, persist: L1_PERSIST_SCOPE },
  { lane: "l2", fresh: freshL2, persist: L2_PERSIST_SCOPE },
  { lane: "l3", fresh: freshL3, persist: L3_PERSIST_SCOPE },
  { lane: "l4", fresh: freshL4, persist: L4_PERSIST_SCOPE },
  { lane: "l5", fresh: freshL5, persist: L5_PERSIST_SCOPE },
  { lane: "l6", fresh: freshL6, persist: L6_PERSIST_SCOPE },
  { lane: "l7", fresh: freshL7, persist: L7_PERSIST_SCOPE },
  { lane: "l8", fresh: freshL8, persist: L8_PERSIST_SCOPE },
  { lane: "l9", fresh: freshL9, persist: L9_PERSIST_SCOPE },
];

/** Every key that is in more than one of `sets`, with the lanes that hold it. */
function shared(sets: { lane: string; keys: string[] }[]): string[] {
  const where = new Map<string, string[]>();
  for (const { lane, keys } of sets) for (const k of keys) where.set(k, [...(where.get(k) ?? []), lane]);
  return [...where].filter(([, lanes]) => lanes.length > 1).map(([k, lanes]) => `${k}: ${lanes.join(", ")}`);
}

/** The optional AppState fields freshState leaves unset, so no runtime key shows them. */
function optionalFields(file: string): string[] {
  const text = readFileSync(join(ROOT, file), "utf8");
  return [...text.matchAll(/^ {2}(\w+)\?:/gm)].map((m) => m[1]);
}

const PART_FILES = ["core", "l1", "l2", "l3", "l4", "l5", "l6", "l7", "l8", "l9"].map((n) => `src/shell/state/${n}.ts`);

describe("L0-WP06: AppState partition", () => {
  it("app_state_parts_are_disjoint", () => {
    for (const pid of Object.keys(PERSONAS)) {
      const sets = PARTS.map((p) => ({ lane: p.lane, keys: Object.keys(p.fresh(pid)) }));
      expect(shared(sets), `persona ${pid}`).toEqual([]);
      // Together the parts are the whole state: no field is outside every part.
      const union = sets.flatMap((s) => s.keys).sort();
      expect(union, `persona ${pid}`).toEqual(Object.keys(freshState(pid)).sort());
    }
    // Optional fields are absent at run time, so they are checked by declaration.
    const optional = PART_FILES.map((f) => ({ lane: f, keys: optionalFields(f) }));
    expect(shared(optional)).toEqual([]);
    expect(optional.flatMap((o) => o.keys).sort()).toEqual(["memGraph", "s6ready"]);
    // Mutation: a field written into two parts is named.
    expect(shared([{ lane: "core", keys: ["route", "node"] }, { lane: "l3", keys: ["stage", "route"] }])).toEqual(["route: core, l3"]);
  });

  it("persist_scope_union_is_unchanged", () => {
    const sets = PARTS.map((p) => ({ lane: p.lane, keys: Object.keys(p.persist) }));
    expect(shared(sets)).toEqual([]);
    const union: Record<string, string> = Object.assign({}, ...PARTS.map((p) => p.persist));
    expect(union).toEqual(PERSIST_SCOPE);
    // Each part persists only fields it holds.
    for (const p of PARTS) {
      const own = new Set(Object.keys(p.fresh("p1")));
      expect(Object.keys(p.persist).filter((k) => !own.has(k)), p.lane).toEqual([]);
    }
    // The table and its key order are the ones recorded before the split (the save writes the
    // keys in this order, so a save of the same state is the same bytes).
    const recorded = JSON.parse(readFileSync(join(ROOT, `fixtures/seams/persist-keys.${SEAMS_BASE}.json`), "utf8"));
    expect(PERSIST_SCOPE).toEqual(recorded.persistScope);
    expect([...PERSIST_KEYS]).toEqual(recorded.persistKeys);
  });

  it("parts_never_import_state_ts_at_run_time", () => {
    // state.ts imports every part; a part that imported state.ts for a value would close a cycle
    // and read state.ts's exports before they exist. Types are erased, so `import type` is fine.
    const files = readdirSync(join(ROOT, "src/shell/state")).filter((f) => f.endsWith(".ts") && !f.includes(".test."));
    expect(files.sort()).toEqual([...PART_FILES.map((f) => f.split("/").pop()), "persona.ts"].sort());
    const valueImport = /^import\s+(?!type\b)[^;]*from\s+"\.\.\/state"/m;
    for (const f of files) {
      const text = readFileSync(join(ROOT, "src/shell/state", f), "utf8");
      expect(valueImport.test(text), f).toBe(false);
    }
    expect(valueImport.test('import { PERSONAS } from "../state";')).toBe(true);
    expect(valueImport.test('import type { ChatMsg } from "../state";')).toBe(false);
  });
});
