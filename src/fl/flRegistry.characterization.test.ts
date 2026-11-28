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
// (#[ignore], cfg_attr, or a cfg other than cfg(test)), when a protected Rust file's host test
// module or its lib.rs `mod` is cfg'd out, when a protected or L8 vitest file skips, focuses or
// inverts a test or a whole describe block, or when the characterization doc stops naming a
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
  host?: string;
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
const LIB_RS = "src-tauri/src/lib.rs";
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

/**
 * Rust source as code only: comments become a space and string, raw string and char literals
 * become `""`, in one left-to-right pass so a `//` in a string or a quote in a comment is read as
 * the lexer reads it.
 */
function rustCode(src: string): string {
  const token = /\/\/[^\n]*|\/\*[\s\S]*?\*\/|b?r(#*)"[\s\S]*?"\1|b?"(?:[^"\\]|\\[\s\S])*"|b?'(?:[^'\\\n]|\\(?:u\{[0-9a-fA-F]{1,6}\}|x[0-9a-fA-F]{2}|.))'/g;
  return src.replace(token, (t) => (t.startsWith("/") ? " " : '""'));
}

/**
 * Attributes that turn a Rust test off wherever they sit, the block parse aside: an #[ignore], a
 * cfg_attr, a cfg other than cfg(test) (on an enclosing module too), or any inner `#![...]`.
 */
const DISABLING_ATTR = /#\s*(?:!\s*\[|\[\s*(?:ignore\b|cfg(?!\s*\(\s*test\s*\)\s*\])))/g;
const ATTR_AT = new RegExp(`^#\\s*!?\\s*\\[((?:"(?:[^"\\\\]|\\\\.)*"|[^\\]"])*)\\]`);

/** The `mod` items in Rust code, by name. */
const MOD_ITEM = /\bmod\s+([A-Za-z_][A-Za-z0-9_]*)/g;

/** cargo: the functions under `#[test]` (or `#[tokio::test]`) that still run, outside comments. */
function declaredCargo(src: string): string[] {
  return cargoFns(src)
    .filter((f) => isCargoTest(f) && disabledBy(f) === undefined)
    .map((f) => f.name);
}

/**
 * cargo: each test turned off (`name: #[attr]`), then each disabling attribute anywhere in the code
 * (whitespace removed), then each `mod <name>` other than the suite's own opening `suiteMod`. A
 * test in a cfg'd-out module keeps its own #[test] block, so a protected or pinned file carries
 * no such attribute and no other module at all. Empty when every test runs.
 */
function disabledCargo(src: string, suiteMod?: string): string[] {
  const off = cargoFns(src)
    .filter((f) => isCargoTest(f) && disabledBy(f) !== undefined)
    .map((f) => `${f.name}: #[${disabledBy(f)}]`);
  const code = rustCode(src);
  const attrs = [...code.matchAll(DISABLING_ATTR)].map((m) => {
    const at = code.slice(m.index ?? 0);
    const whole = at.match(ATTR_AT)?.[0] ?? at.split("\n")[0];
    return whole.replace(/\s+/g, "");
  });
  const mods = [...code.matchAll(MOD_ITEM)].map((m) => `mod ${m[1]}`);
  const own = suiteMod === undefined ? -1 : mods.indexOf(`mod ${suiteMod}`);
  if (own >= 0) mods.splice(own, 1);
  return [...off, ...attrs, ...mods];
}

/** The column-0 test module of a host file: `#[cfg(test)]`, `mod tests {`, include lines only, `}`. */
const TEST_MODULE = /^#\[cfg\(test\)\]\nmod tests \{\n((?:    include!\("[a-z0-9_]+\.rs"\);\n)+)\}$/gm;

/** How many times `needle` occurs in `hay`. */
const occurrences = (hay: string, needle: string) => hay.split(needle).length - 1;

/**
 * Why `host` does not compile `file` into its tests, or undefined when it does: the include occurs
 * once outside comments, inside the column-0 `#[cfg(test)] mod tests { ... }` block, with no
 * attribute stacked on that block and no inner `#![cfg...]` in the host.
 */
function hostCompileError(host: string, file: string): string | undefined {
  const src = cargoSource(host);
  const inc = `include!("${basename(file)}");`;
  const n = occurrences(src, inc);
  if (n !== 1) return `${inc} occurs ${n} times outside comments`;
  if (/^[ \t]*#!\[\s*cfg/m.test(src)) return "an inner #![cfg...] in the host";
  const blocks = [...src.matchAll(TEST_MODULE)].filter((m) => m[1].includes(`    ${inc}\n`));
  if (blocks.length !== 1) return `${inc} is not in a column-0 #[cfg(test)] mod tests { ... }`;
  if (src.slice(0, blocks[0].index ?? 0).trimEnd().endsWith("]")) return "an attribute on the test module";
  return undefined;
}

/** Why lib.rs does not compile `mod <name>;` in, or undefined: once, at column 0, no attribute, no crate #![cfg(...)]. */
function libDeclareError(lib: string, name: string): string | undefined {
  const src = cargoSource(lib);
  if (/^[ \t]*#!\[\s*cfg\s*\(/m.test(src)) return "a crate-level #![cfg(...)]";
  const lines = src.match(new RegExp(`^.*\\bmod ${name}\\b.*$`, "gm")) ?? [];
  if (lines.length !== 1 || lines[0] !== `mod ${name};`) return `mod ${name}: ${JSON.stringify(lines)}`;
  const at = src.search(new RegExp(`^mod ${name};$`, "m"));
  if (src.slice(0, at).trimEnd().endsWith("]")) return `an attribute on mod ${name}`;
  return undefined;
}

/** Why `src` does not include `file` as a top-level item, or undefined: once, at column 0, right after a closed item. */
function topLevelIncludeError(src: string, file: string): string | undefined {
  const code = cargoSource(src);
  const inc = `include!("${basename(file)}");`;
  const n = occurrences(code, inc);
  if (n !== 1) return `${inc} occurs ${n} times outside comments`;
  if (/^[ \t]*#!\[/m.test(code)) return "an inner attribute";
  const at = code.split("\n").indexOf(inc);
  if (at < 0) return `${inc} is not a column-0 line of its own`;
  if (!code.split("\n").slice(0, at).join("\n").trimEnd().endsWith("}")) return `${inc} does not follow a closed item`;
  return undefined;
}

/** Why the suite file does not open with its `mod <name> {` (nothing but comments above it), or undefined. */
function suiteModError(src: string, name: string): string | undefined {
  return uncommented(src).trimStart().startsWith(`mod ${name} {`) ? undefined : `mod ${name} is not the first item`;
}

/** vitest modifiers that skip, focus or invert a test or a whole describe block. */
const MODIFIER = "skip|only|todo|skipIf|runIf|fails";
const SKIPPING = [
  new RegExp(`\\.\\s*(?:${MODIFIER})\\b`), // describe.<m>, it.<m>, test.<m>, ctx.<m>()
  new RegExp(`\\[\\s*["'\`](?:${MODIFIER})["'\`]\\s*\\]`), // describe["<m>"]
  new RegExp(`\\b(?:${MODIFIER})["'\`]?\\s*:`), // { <m>: true } test or describe options
  /\bx(?:describe|it|test)\b/, // the x-prefixed skips
];

/** The lines, outside comments, that skip, focus or invert a test or a describe block. Empty when all run. */
function skippingLines(src: string): string[] {
  return uncommented(src)
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => SKIPPING.some((r) => r.test(l)));
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
      // The including file is itself protected, so its own host and lib.rs line are checked too.
      const protectedHost = REG.protected.some((p) => p.runner === "cargo" && p.file === host);
      expect(protectedHost, `${s.id}: ${host} is a protected cargo file`).toBe(true);
      const hostSrc = existsSync(host) ? readFileSync(host, "utf8") : "";
      const mod = (s.filter ?? "").replace(/::$/, "");
      for (const f of s.files) {
        // The host test file pulls the suite in as a top-level item, so `cargo test` builds it.
        expect(topLevelIncludeError(hostSrc, f), `${host} includes ${f}`).toBeUndefined();
        // The filter LQ runs names the module the file opens with, and nothing cfgs that module out.
        const src = existsSync(f) ? readFileSync(f, "utf8") : "";
        expect(suiteModError(src, mod), `${f} declares mod ${mod}`).toBeUndefined();
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

  it("protected_cargo_files_are_compiled_in", () => {
    // A protected Rust file runs only while its host's `#[cfg(test)] mod tests` includes it and
    // lib.rs declares the host module with no cfg. Either one cfg'd out drops every test in the
    // file (the 27 collector tests, or this lane's own L8-RUST suite) with no name or count change.
    const lib = existsSync(LIB_RS) ? readFileSync(LIB_RS, "utf8") : "";
    const cargo = REG.protected.filter((p) => p.runner === "cargo");
    expect(cargo.map((p) => p.file)).toContain("src-tauri/src/fl_collect_tests.rs");
    for (const p of cargo) {
      expect(p.host, p.file).toMatch(/^src-tauri\/src\/[a-z0-9_]+\.rs$/);
      const host = p.host ?? "";
      expect(existsSync(host), `${p.file}: ${host}`).toBe(true);
      const hostSrc = existsSync(host) ? readFileSync(host, "utf8") : "";
      expect(hostCompileError(hostSrc, p.file), `${host} compiles ${p.file}`).toBeUndefined();
      expect(libDeclareError(lib, basename(host, ".rs")), `lib.rs declares ${host}`).toBeUndefined();
    }
  });

  it("no_protected_or_l8_vitest_suite_is_skipped", () => {
    // A describe.skip (or .only, .todo, .skipIf, an x-prefix, a { skip: true }) keeps every
    // test's name and count while none of them runs, so no protected or L8 vitest file has one.
    const files = [
      ...REG.suites.filter((s) => s.runner === "vitest").flatMap((s) => s.files),
      ...REG.protected.filter((p) => p.runner === "vitest").map((p) => p.file),
    ];
    expect(files).toContain("src/fl/fl.characterization.test.tsx");
    expect(files).toContain("src/surfaces/alfSurfaceHonesty.test.tsx");
    for (const f of files) {
      const src = existsSync(f) ? readFileSync(f, "utf8") : "";
      expect(skippingLines(src), f).toEqual([]);
    }
  });

  it("no_pinned_or_protected_cargo_test_is_disabled", () => {
    // A Rust test that is #[ignore]d, behind a cfg_attr or cfg'd out (itself or by an enclosing
    // module) no longer runs, so no pinned or protected file carries one (the collector safety
    // guards in fl_collect_tests.rs above all).
    const files = [
      ...REG.suites.filter((s) => s.runner === "cargo").flatMap((s) => s.files),
      ...REG.protected.filter((p) => p.runner === "cargo").map((p) => p.file),
    ];
    expect(files).toContain("src-tauri/src/fl_collect_tests.rs");
    // The L8-RUST file opens with its own module (suiteModError pins where); no other mod anywhere.
    const suiteMod = new Map<string, string>();
    for (const s of REG.suites.filter((x) => x.runner === "cargo"))
      for (const f of s.files) suiteMod.set(f, (s.filter ?? "").replace(/::$/, ""));
    for (const f of files) {
      const src = existsSync(f) ? readFileSync(f, "utf8") : "";
      expect(disabledCargo(src, suiteMod.get(f)), f).toEqual([]);
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
      "    #[allow(dead_code)]",
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
      "#[allow(clippy::bool_assert_comparison)]",
      "fn allow_runs() {}",
      "#[test]",
      "#[should_panic]",
      "fn should_panic_inverts() {}",
    ].join("\n");
    expect(declaredCargo(rs)).toEqual(["cfg_test_runs", "allow_runs"]);
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
      "should_panic_inverts: #[should_panic]",
      "#[ignore]",
      '#[ignore=""]',
      "#[ignore]",
      "#[ignore]",
      "#[ignore]",
      '#[ignore=""]',
      "#[cfg(any())]",
      "#[cfg(not(test))]",
      "#[cfg_attr(unix,ignore)]",
      "#[cfg_attr(unix,ignore)]",
      "#[should_panic]",
    ]);
    // So a protected file whose test gains #[ignore] drops below its count and loses the name.
    const before = "#[test]\nfn guard() {}\n#[test]\nfn other() {}";
    const after = "#[ignore]\n#[test]\nfn guard() {}\n#[test]\nfn other() {}";
    expect(countTests("cargo", before)).toBe(2);
    expect(protectedDeclares("cargo", before, "guard")).toBe(true);
    expect(countTests("cargo", after)).toBe(1);
    expect(protectedDeclares("cargo", after, "guard")).toBe(false);
  });

  it("cargo_checks_reject_a_test_in_a_cfgd_out_module", () => {
    // A cfg on an enclosing module turns off every test in it while each fn keeps its own #[test]
    // block, so no pinned or protected file carries a cfg other than cfg(test), an inner
    // attribute, or a mod other than the suite's own opening module, wherever it sits.
    const inner = "    #[test]\n    fn guard() {}\n";
    const lost = [
      `#[cfg(any())]\nmod off {\n${inner}}\n`,
      `#[cfg(not(test))]\npub(crate) mod off {\n${inner}}\n`,
      `fn x() {} #[cfg(any())] mod off {\n${inner}}\n`,
      `#[cfg(test)] #[cfg(any())] mod off {\n${inner}}\n`,
      `#[cfg(\n    any()\n)]\nmod off {\n${inner}}\n`,
      `#\n[cfg(any())]\nconst _: () = ();\n${inner}`,
      `#[\n  cfg(any())\n]\nconst _: () = ();\n${inner}`,
      `mod off {\n    #![cfg(any())]\n${inner}}\n`,
      `#![cfg(any())]\n${inner}`,
      `mod plain {\n${inner}}\n`,
    ];
    for (const rs of lost) {
      // The hole: the fn's own attribute block still reads as a live test.
      expect(declaredCargo(rs), rs).toEqual(["guard"]);
      expect(disabledCargo(rs), rs).not.toEqual([]);
    }
    // The suite file's own opening module is the one mod allowed, and only under its own name.
    const suite = `mod lq_g {\n${inner}}\n`;
    expect(disabledCargo(suite, "lq_g")).toEqual([]);
    expect(disabledCargo(suite)).toEqual(["mod lq_g"]);
    expect(disabledCargo(`${suite}mod more {\n${inner}}\n`, "lq_g")).toEqual(["mod more"]);
    // cfg(test), a derive, and text inside a string, a char or a comment stay live.
    const live = [
      "#[cfg(test)]",
      "#[test]",
      "fn guard() {}",
      "#[derive(Default)]",
      "struct S;",
      'const A: &str = "#[cfg(test)] mod guard; #[cfg(any())]";',
      'const B: &str = r#"mod off { "#;',
      "const C: char = '\"';",
      "const D: [char; 3] = ['\\'', '\\u{22}', '\"'];",
      "const E: &str = \"mod off\";",
      "fn d() {} // a trailing comment: mod off, #[cfg(any())]",
    ].join("\n");
    expect(disabledCargo(live)).toEqual([]);
  });

  it("host_checks_reject_a_cfgd_out_test_module", () => {
    // A cfg'd-out host module or lib.rs line drops every test the file declares, names intact.
    const inc = 'include!("g_tests.rs");';
    const good = `fn x() {\n}\n\n#[cfg(test)]\nmod tests {\n    ${inc}\n}\n`;
    for (const ok of [
      good,
      good.replace("mod tests {\n", 'mod tests {\n    include!("other_tests.rs");\n'),
      good.replace("#[cfg(test)]", "// The tests.\n#[cfg(test)]"),
    ])
      expect(hostCompileError(ok, "src-tauri/src/g_tests.rs"), ok).toBeUndefined();
    const lostHost = [
      good.replace("#[cfg(test)]", "#[cfg(any())]"),
      good.replace("#[cfg(test)]", "#[cfg(not(test))]"),
      good.replace("#[cfg(test)]", "#[cfg(any())]\n#[cfg(test)]"),
      good.replace("#[cfg(test)]", "#[cfg(test)]\n#[cfg(any())]"),
      good.replace("#[cfg(test)]", "#[cfg(any())] // for now\n#[cfg(test)]"),
      good.replace(`    ${inc}`, `    // ${inc}`),
      good.replace(`    ${inc}`, `    #[cfg(any())]\n    ${inc}`),
      good.replace("mod tests {\n", "mod tests {\n    #![cfg(any())]\n"),
      `#![cfg(any())]\n${good}`,
      `/*\n${good}*/\n`,
      good.replace(`    ${inc}\n`, ""),
      `${good}\n#[cfg(test)]\nmod more {\n    ${inc}\n}\n`,
    ];
    for (const lost of lostHost) expect(hostCompileError(lost, "g_tests.rs"), lost).toBeDefined();

    const lib = "mod a;\n// The guard.\nmod guard;\nmod b;\n";
    expect(libDeclareError(lib, "guard")).toBeUndefined();
    const lostLib = [
      lib.replace("mod guard;", "#[cfg(any())]\nmod guard;"),
      lib.replace("mod guard;", "#[cfg(test)] mod guard;"),
      lib.replace("mod guard;", "// mod guard;"),
      lib.replace("mod guard;", "/*\nmod guard;\n*/"),
      lib.replace("mod guard;", "mod off {\n    mod guard;\n}"),
      lib.replace("mod guard;\n", ""),
      `#![cfg(any())]\n${lib}`,
    ];
    for (const lost of lostLib) expect(libDeclareError(lost, "guard"), lost).toBeDefined();

    const tail = `#[test]\nfn t() {\n}\n\n// The suite.\n${inc}\n`;
    expect(topLevelIncludeError(tail, "g_tests.rs")).toBeUndefined();
    const lostTail = [
      tail.replace(inc, `#[cfg(any())]\n${inc}`),
      tail.replace(inc, `#[cfg(any())] ${inc}`),
      tail.replace(inc, `// ${inc}`),
      tail.replace(inc, `mod off {\n    ${inc}\n}`),
      `#![cfg(any())]\n${tail}`,
      `${tail}${inc}\n`,
    ];
    for (const lost of lostTail) expect(topLevelIncludeError(lost, "g_tests.rs"), lost).toBeDefined();

    const suite = "// The suite.\nmod lq_g {\n}\n";
    expect(suiteModError(suite, "lq_g")).toBeUndefined();
    expect(suiteModError(suite.replace("mod lq_g", "#[cfg(any())]\nmod lq_g"), "lq_g")).toBeDefined();
    expect(suiteModError(suite.replace("mod lq_g", "// mod lq_g"), "lq_g")).toBeDefined();
  });

  it("cargo_checks_reject_a_nested_or_inverted_test", () => {
    // rustc collects a #[test] fn at module scope only: one inside a fn body, a closure, an impl or
    // a const block never runs (an allow(unnameable_test_items) silences the lint), so it counts as
    // lost; so does a #[should_panic] test, which passes only while the behavior it guards is broken.
    const inner = "    #[test]\n    fn guard() {}\n";
    const nested = [
      `fn off() {\n${inner}}\n`,
      `#[allow(unnameable_test_items, dead_code)] fn off() {\n${inner}}\n`,
      `const _: () = {\n${inner}};\n`,
      `struct S;\nimpl S {\n${inner}}\n`,
      `fn off() {\n    let _ = || {\n${inner}    };\n}\n`,
      `mod lq_g {\n    fn off() {\n${inner}    }\n}\n`,
    ];
    for (const rs of nested) {
      expect(declaredCargo(rs), rs).toEqual([]);
      expect(countTests("cargo", rs), rs).toBe(0);
      expect(protectedDeclares("cargo", rs, "guard"), rs).toBe(false);
      expect(disabledCargo(rs, "lq_g"), rs).toContain("guard: inside a block that is not a module");
    }
    for (const rs of ["#[test]\n#[should_panic]\nfn guard() {}", '#[should_panic(expected = "dark")]\n#[test]\nfn guard() {}']) {
      expect(declaredCargo(rs), rs).toEqual([]);
      expect(disabledCargo(rs), rs).not.toEqual([]);
    }
    expect(disabledCargo("#[allow(unnameable_test_items)]\nconst X: u8 = 0;\n")).toEqual(["unnameable_test_items"]);
    expect(disabledCargo(`}\n${inner}`)).toContain("a } with no { open");
    expect(disabledCargo(`fn off() {\n${inner}`)).toContain("an unclosed {");
    // A module scope stays live, and braces in a string, a char, a raw string or a comment are no scope.
    expect(disabledCargo(`mod lq_g {\n${inner}}\n`, "lq_g")).toEqual([]);
    expect(declaredCargo(`mod lq_g {\n${inner}}\n`)).toEqual(["guard"]);
    const live = [
      "fn h<'a>(s: &'a str) -> &'a str {",
      '    let _ = "}";',
      "    let _ = '}';",
      '    let _ = r#"}"#;',
      "    s",
      "} // }",
      "/* } */",
      "#[test]",
      "fn guard() {}",
    ].join("\n");
    expect(declaredCargo(live)).toEqual(["guard"]);
    expect(disabledCargo(live)).toEqual([]);

    // The review's case on the real file: the DNS-rebinding guard wrapped in a fn body.
    const real = readFileSync("src-tauri/src/fl_collect_tests.rs", "utf8");
    const name = "every_fetch_is_pinned_to_the_one_vetted_answer_and_a_rebinding_answer_is_never_used";
    const count = countTests("cargo", real);
    expect(count).toBeGreaterThanOrEqual(27);
    expect(protectedDeclares("cargo", real, name)).toBe(true);
    expect(disabledCargo(real)).toEqual([]);
    const start = real.indexOf(`#[test]\nfn ${name}() {\n`);
    expect(start).toBeGreaterThan(0);
    const end = real.indexOf("\n}\n", start) + 3;
    for (const open of ["#[allow(unnameable_test_items, dead_code)] fn off() {\n", "fn off() {\n"]) {
      const wrapped = `${real.slice(0, start)}${open}${real.slice(start, end)}}\n${real.slice(end)}`;
      expect(countTests("cargo", wrapped), open).toBe(count - 1);
      expect(protectedDeclares("cargo", wrapped, name), open).toBe(false);
      expect(disabledCargo(wrapped), open).toContain(`${name}: inside a block that is not a module`);
    }
  });

  it("vitest_checks_reject_a_skipped_or_focused_describe", () => {
    // Built from MODIFIER so this file itself carries none of the patterns it rejects.
    const mods = MODIFIER.split("|");
    const lost = [
      ...mods.map((m) => `describe.${m}("suite", () => {`),
      ...mods.map((m) => `  it.${m}("t", () => {});`),
      ...mods.map((m) => `  test.${m}("t", () => {});`),
      ...mods.map((m) => `describe("suite", { ${m}: true }, () => {`),
      ...mods.map((m) => `describe["${m}"]("suite", () => {`),
      ...["describe", "it", "test"].map((f) => `x${f}("t", () => {});`),
      `describe\n  .${mods[0]}("suite", () => {`,
      `  it("t", (ctx) => { ctx.${mods[0]}(); });`,
    ];
    for (const l of lost) expect(skippingLines(l), l).not.toEqual([]);
    const live = [
      'describe("suite", () => {',
      '  it("kept", () => {});',
      '  it.each([1, 2])("each %i", () => {});',
      '  test("only_once", { timeout: 5000 }, () => {});',
      "  const skipped = 0;",
      `  // describe.${mods[0]} in a line comment`,
      `/* x${"it"} in a block comment */`,
      "});",
    ].join("\n");
    expect(skippingLines(live)).toEqual([]);
    // The hole this closes: a describe-level skip keeps the count and the names.
    const file = [`describe.${mods[0]}("suite", () => {`, '  it("guard", () => {});', "});"].join("\n");
    expect(countTests("vitest", file)).toBe(1);
    expect(protectedDeclares("vitest", file, "guard")).toBe(true);
    expect(skippingLines(file)).toEqual([`describe.${mods[0]}("suite", () => {`]);
  });
});
