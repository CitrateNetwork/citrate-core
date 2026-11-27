// @vitest-environment node
//
// L5-WP01 (planset v060, lane L5): the lane's protected-test manifest, tests/lq/l5.json, names
// real tests. Every listed suite file exists, holds at least the recorded number of tests (the
// count never drops), and every listed test id is declared in it: a Rust `fn <id>(` under a test
// attribute, or a vitest `it("<id>"` / `test("<id>"`. A characterization test is only replaced by
// a stricter one, so a `replacedBy` id must also exist. LQ owns the runner that executes these
// suites; this check only keeps the manifest honest.
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(fs.readFileSync(path.join(root, "tests/lq/l5.json"), "utf8"));

const RUST_TEST = /^\s*#\[(tokio::)?test/gm;
const VITEST_TEST = /^\s*(it|test)(\.each\([^)]*\))?\(/gm;

function countTests(runner, src) {
  return (src.match(runner === "cargo" ? RUST_TEST : VITEST_TEST) ?? []).length;
}

function declares(runner, src, id) {
  if (runner === "cargo") return new RegExp(`\\bfn ${id}\\(`).test(src);
  return src.includes(`it("${id}"`) || src.includes(`test("${id}"`);
}

const suites = manifest.surfaces.flatMap((s) => s.suites.map((suite) => ({ surface: s.surface, ...suite })));

describe("tests/lq/l5.json", () => {
  it("is version 1 for lane L5 and lists every surface the lane will change", () => {
    expect(manifest.version).toBe(1);
    expect(manifest.lane).toBe("L5");
    expect(manifest.surfaces.map((s) => s.surface)).toEqual([
      "onboarding-model-pins",
      "llama-server-argv",
      "model-catalog",
      "settings-local-model-card",
      "adapters-eval-gate",
      "media-routes",
      "voice",
    ]);
  });

  it("gives every surface at least one characterization suite and real source paths", () => {
    for (const s of manifest.surfaces) {
      expect(s.suites.some((x) => x.kind === "characterization"), s.surface).toBe(true);
      for (const p of s.paths) expect(fs.existsSync(path.join(root, p)), `${s.surface}: ${p}`).toBe(true);
    }
  });

  for (const suite of suites) {
    it(`${suite.surface}: ${suite.file} exists and keeps at least ${suite.count} tests`, () => {
      expect(["cargo", "vitest"]).toContain(suite.runner);
      expect(["characterization", "protected"]).toContain(suite.kind);
      const file = path.join(root, suite.file);
      expect(fs.existsSync(file), `missing ${suite.file}`).toBe(true);
      const src = fs.readFileSync(file, "utf8");
      expect(countTests(suite.runner, src)).toBeGreaterThanOrEqual(suite.count);
      if (suite.kind === "characterization") expect(suite.tests.length).toBe(suite.count);
      for (const t of suite.tests) {
        expect(declares(suite.runner, src, t.id), `${suite.file} declares ${t.id}`).toBe(true);
        if (t.replacedBy) expect(declares(suite.runner, src, t.replacedBy), `${suite.file} declares ${t.replacedBy}`).toBe(true);
      }
    });
  }

  it("registers each new Rust characterization file as a test-only module", () => {
    const lib = fs.readFileSync(path.join(root, "src-tauri/src/lib.rs"), "utf8");
    for (const suite of suites.filter((x) => x.runner === "cargo" && x.kind === "characterization")) {
      const mod = path.basename(suite.file, ".rs");
      expect(lib, `lib.rs registers ${mod}`).toMatch(new RegExp(`#\\[cfg\\(test\\)\\]\\s*\\nmod ${mod};`));
    }
  });
});
