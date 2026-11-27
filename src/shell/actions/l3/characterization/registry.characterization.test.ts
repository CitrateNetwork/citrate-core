// 0.6.0 L3-WP00: the L3 characterization registry, ci/characterization.d/l3.json, checked against the
// suite files it lists.
//
// L3 (shell, onboarding, nav, notifications) pins today's launch behavior before any 0.6.0 change
// to it: the gate order, the onboarding stages, the launch, the routes, the coach and toast, the
// absence of OS notifications and what app quit does. Each suite is green on citrate-core a826e441
// and must only change together with a reviewed, deliberate change of its expectation.
//
// The registry is the one list (Rule 9): LQ reads it, and this test fails when a surface, a file or
// a test goes missing, when a file declares a test the registry does not list, or when a listed test
// survives only inside a comment. That a declared test also runs is shown by the runner's own counts
// (LQ ratchet), not here.
import { describe, it, expect } from "vitest";
import { existsSync, readFileSync } from "node:fs";

interface Suite {
  id: string;
  lane: string;
  runner: string;
  files: string[];
  covers: string[];
  pins: string;
  tests: string[];
  supersededBy?: string[];
}
interface Registry {
  version: number;
  owner: string;
  base: string;
  suites: Suite[];
}

const REGISTRY_PATH = "ci/characterization.d/l3.json";
const REG = JSON.parse(readFileSync(REGISTRY_PATH, "utf8")) as Registry;
const DIR = "src/shell/actions/l3/characterization/";

/** The L3 surfaces WP00 pins. Removing one means editing this list too, in review. */
const SURFACES = ["L3-GATES", "L3-STAGES", "L3-LAUNCH", "L3-NAV", "L3-COACH", "L3-NOTIFY", "L3-QUIT", "L3-REGISTRY"];

/** Source with block comments and whole-line `//` comments removed. */
function uncommented(src: string): string {
  return src.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/^[ \t]*\/\/.*$/gm, " ");
}

/** The test names declared as `it("name", ...)` at the start of a line, outside comments. */
function declaredTests(src: string): string[] {
  return [...uncommented(src).matchAll(/^[ \t]*it\(\s*"([A-Za-z0-9_]+)"\s*,/gm)].map((m) => m[1]);
}

describe("L3 characterization registry (L3-WP00)", () => {
  it("registry_lists_every_l3_surface_once", () => {
    expect(REG.version).toBe(1);
    expect(REG.owner).toBe("L3");
    expect(REG.base).toBe("a826e441");
    expect(REG.suites.map((s) => s.id)).toEqual(SURFACES);
    for (const s of REG.suites) {
      expect(s.lane, s.id).toBe("L3");
      expect(s.runner, s.id).toBe("vitest");
      expect(s.files.length, s.id).toBeGreaterThan(0);
      expect(s.covers.length, s.id).toBeGreaterThan(0);
      expect(s.pins.length, s.id).toBeGreaterThan(0);
      expect(s.tests.length, s.id).toBeGreaterThan(0);
      for (const f of s.files) {
        // In the L3-owned path, and named so `npm run test:characterization` selects it.
        expect(f.startsWith(DIR), f).toBe(true);
        expect(f, f).toMatch(/\.characterization\.test\.tsx?$/);
        expect(existsSync(f), f).toBe(true);
      }
      for (const c of s.covers) expect(existsSync(c), `${s.id} covers ${c}`).toBe(true);
    }
    const files = REG.suites.flatMap((s) => s.files);
    expect(new Set(files).size).toBe(files.length);
  });

  it("every_listed_test_is_declared_and_every_declared_test_is_listed", () => {
    for (const s of REG.suites) {
      const declared = s.files.flatMap((f) => declaredTests(readFileSync(f, "utf8")));
      // Each name once, and the file's tests are exactly the listed ones.
      expect(new Set(declared).size, s.id).toBe(declared.length);
      expect([...declared].sort(), s.id).toEqual([...s.tests].sort());
    }
  });

  it("declaration_check_ignores_commented_out_tests", () => {
    const src = [
      'it("kept", () => {});',
      '// it("line_comment", () => {});',
      "/*",
      '  it("block_comment", () => {});',
      "*/",
      'const s = "it(\\"in_a_string\\", () => {})";',
      '  it(\n    "wrapped",\n    () => {},\n  );',
    ].join("\n");
    expect(declaredTests(src)).toEqual(["kept", "wrapped"]);
  });
});
