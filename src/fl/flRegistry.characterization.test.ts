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
// when a protected (honesty) suite loses tests, when a pinned or protected Rust test is turned off
// (#[ignore], cfg_attr, or a cfg other than cfg(test)), or when the characterization doc stops
// naming a pinned test. That a declared test also runs is shown by the runners' own counts (LQ ratchet).
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

/** A Rust fn and its whole attribute block, each attribute with its whitespace removed. */
interface CargoFn {
  name: string;
  attrs: string[];
}

/** One attribute, `#[...]`, its body captured; brackets inside a string literal stay in the body. */
const ATTR = /#\s*\[((?:"(?:[^"\\]|\\.)*"|[^\]"])*)\]/.source;
const CARGO_FN = new RegExp(
  `^[ \\t]*((?:${ATTR}\\s*)+)(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+([A-Za-z0-9_]+)\\s*\\(`,
  "gm",
);

/** Rust source for the cargo checks: comments out, and a trailing `//` comment after an attribute dropped. */
function cargoSource(src: string): string {
  return uncommented(src).replace(/^([ \t]*#.*\])[ \t]*\/\/.*$/gm, "$1");
}

/** Every fn with attributes directly above it, outside comments. */
function cargoFns(src: string): CargoFn[] {
  return [...cargoSource(src).matchAll(CARGO_FN)].map((m) => ({
    name: m[m.length - 1],
    attrs: [...m[1].matchAll(new RegExp(ATTR, "g"))].map((a) => a[1].replace(/\s+/g, "")),
  }));
}

const isCargoTest = (f: CargoFn) => f.attrs.some((a) => /^(?:tokio::)?test\b/.test(a));

/** What turns a Rust test off: an #[ignore], a cfg_attr, or a cfg other than cfg(test), anywhere in its block. */
function disabledBy(f: CargoFn): string | undefined {
  return f.attrs.find((a) => /^(?:ignore|cfg_attr)\b/.test(a) || (/^cfg\b/.test(a) && a !== "cfg(test)"));
}

/** Lines that turn a Rust test off wherever they sit: a backstop for an attribute the block parse misreads. */
const DISABLING_LINE = /^[ \t]*#\s*\[\s*(?:ignore|cfg_attr)\b.*$/gm;

/** cargo: the functions under `#[test]` (or `#[tokio::test]`) that still run, outside comments. */
function declaredCargo(src: string): string[] {
  return cargoFns(src)
    .filter((f) => isCargoTest(f) && disabledBy(f) === undefined)
    .map((f) => f.name);
}

/** cargo: each test turned off (`name: #[attr]`), then each disabling line. Empty when every test runs. */
function disabledCargo(src: string): string[] {
  const off = cargoFns(src)
    .filter((f) => isCargoTest(f) && disabledBy(f) !== undefined)
    .map((f) => `${f.name}: #[${disabledBy(f)}]`);
  const lines = (cargoSource(src).match(DISABLING_LINE) ?? []).map((l) => l.trim());
  return [...off, ...lines];
}

/** Every test a protected file declares, counted the way its runner sees a test start (a Rust test only if it runs). */
function countTests(runner: Runner, src: string): number {
  if (runner === "cargo") return declaredCargo(src).length;
  return (uncommented(src).match(/^[ \t]*(?:it|test)(?:\.each)?\(/gm) ?? []).length;
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

  it("no_pinned_or_protected_cargo_test_is_disabled", () => {
    // A Rust test that is #[ignore]d, behind a cfg_attr or cfg'd out no longer runs, so no pinned
    // or protected file carries one (the collector safety guards in fl_collect_tests.rs above all).
    const files = [
      ...REG.suites.filter((s) => s.runner === "cargo").flatMap((s) => s.files),
      ...REG.protected.filter((p) => p.runner === "cargo").map((p) => p.file),
    ];
    expect(files).toContain("src-tauri/src/fl_collect_tests.rs");
    for (const f of files) {
      const src = existsSync(f) ? readFileSync(f, "utf8") : "";
      expect(disabledCargo(src), f).toEqual([]);
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
      "    #[should_panic]",
      "    async fn nested_async() {}",
      "fn not_a_test() {}",
    ].join("\n");
    expect(declaredCargo(rs)).toEqual(["kept", "nested_async"]);
    expect(countTests("cargo", rs)).toBe(2);
  });

  it("cargo_checks_reject_an_ignored_or_cfgd_out_test", () => {
    // A Rust test that no longer runs is a lost test, as it.skip is on the vitest side, wherever
    // the attribute sits in its block.
    const rs = [
      "#[ignore]",
      "#[test]",
      "fn ignore_above() {}",
      "#[test]",
      '#[ignore = "flaky"]',
      "fn ignore_below() {}",
      "#[test] #[ignore] fn same_line() {}",
      "# [ignore]",
      "#[test]",
      "fn spaced_ignore() {}",
      "#[ignore] // for now",
      "#[test]",
      "fn ignore_with_a_comment() {}",
      '#[ignore = "see [1]"]',
      "#[test]",
      "fn ignore_with_a_bracket() {}",
      "#[cfg(any())]",
      "#[test]",
      "fn cfg_never() {}",
      "#[cfg(not(test))]",
      "#[test]",
      "fn cfg_not_test() {}",
      "#[cfg_attr(unix, ignore)]",
      "#[test]",
      "fn cfg_attr_ignore() {}",
      "#[cfg_attr(",
      "    unix,",
      "    ignore",
      ")]",
      "#[test]",
      "fn cfg_attr_wrapped() {}",
      "#[cfg(test)]",
      "#[test]",
      "fn cfg_test_runs() {}",
      "#[test]",
      "#[should_panic]",
      "fn should_panic_runs() {}",
    ].join("\n");
    expect(declaredCargo(rs)).toEqual(["cfg_test_runs", "should_panic_runs"]);
    expect(countTests("cargo", rs)).toBe(2);
    expect(disabledCargo(rs)).toEqual([
      "ignore_above: #[ignore]",
      'ignore_below: #[ignore="flaky"]',
      "same_line: #[ignore]",
      "spaced_ignore: #[ignore]",
      "ignore_with_a_comment: #[ignore]",
      'ignore_with_a_bracket: #[ignore="see[1]"]',
      "cfg_never: #[cfg(any())]",
      "cfg_not_test: #[cfg(not(test))]",
      "cfg_attr_ignore: #[cfg_attr(unix,ignore)]",
      "cfg_attr_wrapped: #[cfg_attr(unix,ignore)]",
      "#[ignore]",
      '#[ignore = "flaky"]',
      "# [ignore]",
      "#[ignore]",
      '#[ignore = "see [1]"]',
      "#[cfg_attr(unix, ignore)]",
      "#[cfg_attr(",
    ]);
    // So a protected file whose test gains #[ignore] drops below its count and loses the name.
    const before = "#[test]\nfn guard() {}\n#[test]\nfn other() {}";
    const after = "#[ignore]\n#[test]\nfn guard() {}\n#[test]\nfn other() {}";
    expect(countTests("cargo", before)).toBe(2);
    expect(protectedDeclares("cargo", before, "guard")).toBe(true);
    expect(countTests("cargo", after)).toBe(1);
    expect(protectedDeclares("cargo", after, "guard")).toBe(false);
  });
});
