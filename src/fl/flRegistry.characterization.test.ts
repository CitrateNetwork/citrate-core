// 0.6.0 L8-WP01: the L8 characterization registry, ci/characterization.d/l8.json, checked against
// the suite files it lists and against docs/fl/CHARACTERIZATION.md.
//
// L8 (Train together, federated learning) pins today's FL behavior before any 0.6.0 change to it.
// Each suite is green on citrate-core a826e441 and changes only together with a reviewed,
// deliberate change of its expectation, by a WP the registry names.
//
// The registry is the one list (Rule 9): LQ reads it, and this test fails when a suite, a file or
// a test goes missing, when a file declares a test the registry does not list, when a listed test
// survives only inside a comment, when the Rust suite is not compiled in where the registry says,
// when a protected (honesty) suite loses tests, or when the characterization doc stops naming a
// pinned test. That a declared test also runs is shown by the runners' own counts (LQ ratchet).
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
  supersededBy?: string[];
}
interface Registry {
  version: number;
  owner: string;
  base: string;
  suites: Suite[];
  protected: Protected[];
}

const REGISTRY_PATH = "ci/characterization.d/l8.json";
const DOC_PATH = "docs/fl/CHARACTERIZATION.md";
const REG = JSON.parse(readFileSync(REGISTRY_PATH, "utf8")) as Registry;
const TS_DIR = "src/fl/";
const RS_DIR = "src-tauri/src/";

/** The L8 suites WP01 pins. Removing one means editing this list too, in review. */
const SUITES = ["L8-TODAY", "L8-RUST", "L8-REGISTRY"];

/** The honesty list L8-WP01 hands to LQ (lane file section 4, WP01). */
const PROTECTED_FILES = [
  "src/surfaces/alfSurfaceHonesty.test.tsx",
  "src/fl/flFoundation.test.tsx",
  "src/surfaces/node/nodeTabs.test.tsx",
  "src-tauri/src/fl_rounds_tests.rs",
  "src-tauri/src/network_profile_payouts_tests.rs",
  "src-tauri/src/fl_payouts_tests.rs",
  "src-tauri/src/fl_collect_tests.rs",
];

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
const WP = /^L8-WP\d{2}$/;

describe("L8 characterization registry (L8-WP01)", () => {
  it("registry_lists_every_l8_suite_once", () => {
    expect(REG.version).toBe(1);
    expect(REG.owner).toBe("L8");
    expect(REG.base).toBe("a826e441");
    expect(REG.suites.map((s) => s.id)).toEqual(SUITES);
    for (const s of REG.suites) {
      expect(s.lane, s.id).toBe("L8");
      expect(["cargo", "vitest"], s.id).toContain(s.runner);
      expect(s.files.length, s.id).toBeGreaterThan(0);
      expect(s.covers.length, s.id).toBeGreaterThan(0);
      expect(s.pins.length, s.id).toBeGreaterThan(0);
      expect(s.tests.length, s.id).toBeGreaterThan(0);
      for (const f of s.files) {
        // In an L8-owned path, named so the runner (and `npm run test:characterization`) selects it.
        if (s.runner === "vitest") {
          expect(f.startsWith(TS_DIR), f).toBe(true);
          expect(f, f).toMatch(/\.characterization\.test\.tsx?$/);
        } else {
          expect(f.startsWith(RS_DIR), f).toBe(true);
          expect(basename(f), f).toMatch(/^fl_[a-z0-9_]+_tests\.rs$/);
        }
        expect(existsSync(f), `missing ${f}`).toBe(true);
      }
      for (const c of s.covers) expect(existsSync(c), `${s.id} covers ${c}`).toBe(true);
      for (const wp of s.supersededBy ?? []) expect(wp, s.id).toMatch(WP);
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

  it("cargo_suite_is_compiled_in_where_the_registry_says", () => {
    const cargo = REG.suites.filter((x) => x.runner === "cargo");
    expect(cargo.length).toBeGreaterThan(0);
    for (const s of cargo) {
      expect(s.package, s.id).toBe("citrate-core");
      expect(s.filter, s.id).toMatch(/^lq_l8_[a-z0-9_]+::$/);
      expect(s.includedBy, s.id).toMatch(/^src-tauri\/src\/fl_[a-z0-9_]+_tests\.rs$/);
      const host = s.includedBy ?? "";
      expect(existsSync(host), `${s.id}: ${host}`).toBe(true);
      const hostSrc = existsSync(host) ? uncommented(readFileSync(host, "utf8")) : "";
      const mod = (s.filter ?? "").replace(/::$/, "");
      for (const f of s.files) {
        // The host test file pulls the suite into its `mod tests`, so `cargo test` builds it.
        expect(hostSrc, `${host} includes ${f}`).toContain(`include!("${basename(f)}");`);
        // The filter LQ runs names the module the file declares.
        const src = existsSync(f) ? uncommented(readFileSync(f, "utf8")) : "";
        expect(src, `${f} declares mod ${mod}`).toMatch(new RegExp(`^mod ${mod} \\{`, "m"));
      }
    }
  });

  it("protected_suites_keep_their_counts_and_named_tests", () => {
    expect(REG.protected.map((p) => p.file)).toEqual(PROTECTED_FILES);
    for (const p of REG.protected) {
      expect(p.count, p.file).toBeGreaterThan(0);
      expect(p.tests.length, p.file).toBeGreaterThan(0);
      for (const wp of p.supersededBy ?? []) expect(wp, p.file).toMatch(WP);
      expect(existsSync(p.file), `missing protected ${p.file}`).toBe(true);
      const src = existsSync(p.file) ? readFileSync(p.file, "utf8") : "";
      expect(countTests(p.runner, src), p.file).toBeGreaterThanOrEqual(p.count);
      for (const id of p.tests) expect(protectedDeclares(p.runner, src, id), `${p.file} declares ${id}`).toBe(true);
    }
  });

  it("characterization_doc_names_every_pinned_test", () => {
    expect(existsSync(DOC_PATH), `missing ${DOC_PATH}`).toBe(true);
    const doc = existsSync(DOC_PATH) ? readFileSync(DOC_PATH, "utf8") : "";
    // Rule 5 frontmatter.
    for (const k of ["created", "branch", "author", "status"]) expect(doc, k).toMatch(new RegExp(`^${k}: \\S`, "m"));
    // The doc is the human view of the registry: every suite, test and replacing WP appears in it.
    for (const s of REG.suites) {
      expect(doc, s.id).toContain(`\`${s.id}\``);
      for (const t of s.tests) expect(doc, `${s.id} ${t}`).toContain(`\`${t}\``);
      for (const wp of s.supersededBy ?? []) expect(doc, `${s.id} ${wp}`).toContain(wp);
    }
    for (const p of REG.protected) {
      expect(doc, p.file).toContain(`\`${p.file}\``);
      for (const t of p.tests) expect(doc, `${p.file} ${t}`).toContain(`\`${t}\``);
      for (const wp of p.supersededBy ?? []) expect(doc, `${p.file} ${wp}`).toContain(wp);
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
