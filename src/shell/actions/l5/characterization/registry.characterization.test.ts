// 0.6.0 L5-WP01: the L5 characterization registry, ci/characterization.d/l5.json, checked against the
// suite files it lists.
//
// L5 (models, multimodal, LoRA) pins today's model, serving, catalog, adapter, media and voice
// behavior before any 0.6.0 change to it. Each suite is green on citrate-core a826e441 and must only
// change together with a reviewed, deliberate change of its expectation.
//
// The registry is the one list (Rule 9): LQ reads it, and this test fails when a surface, a file or
// a test goes missing, when a file declares a test the registry does not list, when a listed test
// survives only inside a comment, when a Rust suite is not compiled in where the registry says, or
// when a protected suite loses tests. That a declared test also runs is shown by the runners' own
// counts (LQ ratchet), not here.
import { describe, it, expect } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { basename } from "node:path";

type Runner = "cargo" | "vitest";
interface Suite {
  id: string;
  lane: string;
  runner: Runner;
  package?: string;
  filter?: string;
  files: string[];
  includedBy?: string;
  covers: string[];
  pins: string;
  tests: string[];
  supersededBy?: string[];
}
interface Protected {
  runner: Runner;
  file: string;
  count: number;
  tests: string[];
}
interface Registry {
  version: number;
  owner: string;
  base: string;
  suites: Suite[];
  protected: Protected[];
}

const REGISTRY_PATH = "ci/characterization.d/l5.json";
const REG = JSON.parse(readFileSync(REGISTRY_PATH, "utf8")) as Registry;
const TS_DIR = "src/shell/actions/l5/characterization/";
const RS_DIR = "src-tauri/src/lanes/l5/characterization/";

/** The L5 surfaces WP01 pins. Removing one means editing this list too, in review. */
const SURFACES = ["L5-PINS", "L5-ARGV", "L5-CATALOG", "L5-MEDIA", "L5-CARD", "L5-ADAPTERS", "L5-VOICE", "L5-REGISTRY"];

/** Source with block comments and whole-line `//` comments (doc comments too) removed. */
function uncommented(src: string): string {
  return src.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/^[ \t]*\/\/.*$/gm, " ");
}

/** vitest: the names declared as `it("name", ...)` at the start of a line, outside comments. */
function declaredVitest(src: string): string[] {
  return [...uncommented(src).matchAll(/^[ \t]*it\(\s*"([A-Za-z0-9_]+)"\s*,/gm)].map((m) => m[1]);
}

/** cargo: the functions under a `#[test]` (or `#[tokio::test]`) attribute, outside comments. */
function declaredCargo(src: string): string[] {
  const re = /^[ \t]*#\[(?:tokio::)?test[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\(/gm;
  return [...uncommented(src).matchAll(re)].map((m) => m[1]);
}

/** Every test a protected file declares, counted the way its runner sees a test start. */
function countTests(runner: Runner, src: string): number {
  const re = runner === "cargo" ? /^[ \t]*#\[(?:tokio::)?test/gm : /^[ \t]*(?:it|test)(?:\.each)?\(/gm;
  return (uncommented(src).match(re) ?? []).length;
}

/** A protected test is named by its id: a Rust test fn, or a vitest `it("<id>"` / `test("<id>"`. */
function protectedDeclares(runner: Runner, src: string, id: string): boolean {
  if (runner === "cargo") return declaredCargo(src).includes(id);
  const body = uncommented(src);
  return body.includes(`it("${id}"`) || body.includes(`test("${id}"`);
}

const declared = (s: Suite, src: string) => (s.runner === "cargo" ? declaredCargo(src) : declaredVitest(src));

describe("L5 characterization registry (L5-WP01)", () => {
  it("registry_lists_every_l5_surface_once", () => {
    expect(REG.version).toBe(1);
    expect(REG.owner).toBe("L5");
    expect(REG.base).toBe("a826e441");
    expect(REG.suites.map((s) => s.id)).toEqual(SURFACES);
    for (const s of REG.suites) {
      expect(s.lane, s.id).toBe("L5");
      expect(["cargo", "vitest"], s.id).toContain(s.runner);
      expect(s.files.length, s.id).toBeGreaterThan(0);
      expect(s.covers.length, s.id).toBeGreaterThan(0);
      expect(s.pins.length, s.id).toBeGreaterThan(0);
      expect(s.tests.length, s.id).toBeGreaterThan(0);
      for (const f of s.files) {
        // In the L5-owned lane path, named so the runner (and `npm run test:characterization`) selects it.
        if (s.runner === "vitest") {
          expect(f.startsWith(TS_DIR), f).toBe(true);
          expect(f, f).toMatch(/\.characterization\.test\.tsx?$/);
        } else {
          expect(f.startsWith(RS_DIR), f).toBe(true);
          expect(f, f).toMatch(/_tests\.rs$/);
        }
        expect(existsSync(f), `missing ${f}`).toBe(true);
      }
      for (const c of s.covers) expect(existsSync(c), `${s.id} covers ${c}`).toBe(true);
      for (const wp of s.supersededBy ?? []) expect(wp, s.id).toMatch(/^L5-WP\d{2}$/);
    }
    const files = REG.suites.flatMap((s) => s.files);
    expect(new Set(files).size).toBe(files.length);
  });

  it("every_listed_test_is_declared_and_every_declared_test_is_listed", () => {
    for (const s of REG.suites) {
      const names = s.files.flatMap((f) => (existsSync(f) ? declared(s, readFileSync(f, "utf8")) : []));
      // Each name once, and the files' tests are exactly the listed ones.
      expect(new Set(names).size, s.id).toBe(names.length);
      expect([...names].sort(), s.id).toEqual([...s.tests].sort());
    }
  });

  it("cargo_suites_are_compiled_in_where_the_registry_says", () => {
    for (const s of REG.suites.filter((x) => x.runner === "cargo")) {
      expect(s.package, s.id).toBe("citrate-core");
      expect(s.filter, s.id).toMatch(/^lq_l5_[a-z0-9_]+::$/);
      expect(s.includedBy, s.id).toMatch(/^src-tauri\/src\/[a-z0-9_]+_tests\.rs$/);
      const host = s.includedBy ?? "";
      expect(existsSync(host), `${s.id}: ${host}`).toBe(true);
      const hostSrc = existsSync(host) ? uncommented(readFileSync(host, "utf8")) : "";
      const mod = (s.filter ?? "").replace(/::$/, "");
      for (const f of s.files) {
        // The host test file pulls the lane file into its `mod tests`, so `cargo test` builds it.
        expect(hostSrc, `${host} includes ${f}`).toContain(`include!("lanes/l5/characterization/${basename(f)}");`);
        // The filter LQ runs names the module the file declares.
        const src = existsSync(f) ? uncommented(readFileSync(f, "utf8")) : "";
        expect(src, `${f} declares mod ${mod}`).toMatch(new RegExp(`^mod ${mod} \\{`, "m"));
      }
    }
  });

  it("protected_suites_keep_their_counts_and_named_tests", () => {
    expect(REG.protected.length).toBeGreaterThan(0);
    const files = REG.protected.map((p) => p.file);
    expect(new Set(files).size).toBe(files.length);
    for (const p of REG.protected) {
      expect(existsSync(p.file), `missing protected ${p.file}`).toBe(true);
      const src = readFileSync(p.file, "utf8");
      expect(countTests(p.runner, src), p.file).toBeGreaterThanOrEqual(p.count);
      for (const id of p.tests) expect(protectedDeclares(p.runner, src, id), `${p.file} declares ${id}`).toBe(true);
    }
  });

  it("declaration_checks_ignore_commented_out_tests", () => {
    const ts = [
      'it("kept", () => {});',
      '// it("line_comment", () => {});',
      "/*",
      '  it("block_comment", () => {});',
      "*/",
      'const s = "it(\\"in_a_string\\", () => {})";',
      '  it(\n    "wrapped",\n    () => {},\n  );',
    ].join("\n");
    expect(declaredVitest(ts)).toEqual(["kept", "wrapped"]);
    const rs = [
      "#[test]",
      "fn kept() {}",
      "// #[test]",
      "// fn line_comment() {}",
      "/*",
      "#[test]",
      "fn block_comment() {}",
      "*/",
      "    #[tokio::test]",
      "    #[ignore]",
      "    async fn nested_async() {}",
      "fn not_a_test() {}",
    ].join("\n");
    expect(declaredCargo(rs)).toEqual(["kept", "nested_async"]);
    expect(countTests("cargo", rs)).toBe(2);
  });
});
