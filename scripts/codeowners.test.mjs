// created: 2026-11-27 | branch: feat/v60-l0-wp01-ownership-map
// author: Larry Klosowski (L0 lane, L0-WP01; Claude Opus 5.5)
// status: active
//
// L0-WP01: .github/CODEOWNERS is generated from .agentile/v60-ownership.map, so review routing and
// the lane-guard check read one source (Rule 9). These tests fail when the two drift, when a map
// glob cannot be written exactly in CODEOWNERS syntax, when the owner-pinned sign-off path stops
// being the last (winning) CODEOWNERS rule, or when lane-guard puts a branch name into a shell line.
import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { generate, parseMap, toCodeownersPattern } from "./gen-codeowners.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(path.join(repo, p), "utf8");

describe("CODEOWNERS is generated from the v60 ownership map", () => {
  it("codeowners_matches_ownership_map", () => {
    expect(read(".github/CODEOWNERS")).toBe(generate(read(".agentile/v60-ownership.map")));
  });

  it("keeps the owner-pinned sign-off path as the last rule (last match wins)", () => {
    const rules = read(".github/CODEOWNERS").split("\n").filter((l) => l.trim() && !l.startsWith("#"));
    expect(rules[0]).toBe("* @CitrateNetwork/maintainers");
    expect(rules.at(-1)).toBe("/release/signoffs/ @SaulBuilds");
  });

  it("every map row names known lanes and a glob CODEOWNERS can express exactly", () => {
    const { rows } = parseMap(read(".agentile/v60-ownership.map"));
    expect(rows.length).toBeGreaterThan(100);
    for (const r of rows) {
      for (const l of r.lanes) expect(l).toMatch(/^(l[0-9]|lq|frozen|\*)$/);
      expect(() => toCodeownersPattern(r.glob)).not.toThrow();
    }
  });

  it("rejects globs whose CODEOWNERS meaning would differ from the matcher's", () => {
    for (const bad of ["src/**/x.ts", "src/[ab].ts", "src/{a,b}.ts", "!src/a.ts", "/src/a.ts", "src/a b.ts", "src/a?.ts", "src\\a.ts"]) {
      expect(() => toCodeownersPattern(bad), bad).toThrow();
    }
    expect(toCodeownersPattern("**")).toBe("*");
    expect(toCodeownersPattern("src/fl/**")).toBe("/src/fl/**");
    expect(toCodeownersPattern("src-tauri/src/fl_*.rs")).toBe("/src-tauri/src/fl_*.rs");
  });

  it("the ownership matcher selftest passes for the cx and v60 maps", () => {
    const out = execFileSync("bash", ["scripts/cx-ownership-check.sh", "--selftest"], { cwd: repo, encoding: "utf8" });
    expect(out).toContain("SELFTEST: PASS");
    expect(out).not.toContain("SELFTEST FAIL");
  });
});

describe("lane-guard workflow", () => {
  it("passes refs through env and never expands them inside a run block", () => {
    const wf = read(".github/workflows/lane-guard.yml");
    expect(wf).toMatch(/^on:\n {2}pull_request:[^\n]*\n {2}merge_group:/m);
    const lines = wf.split("\n");
    lines.forEach((line, i) => {
      if (!/\$\{\{[^}]*(head_ref|base_ref|\.ref\b|title|body)/.test(line)) return;
      // Only `NAME: ${{ ... }}` env entries may carry a ref expression.
      expect(line, `lane-guard.yml:${i + 1}`).toMatch(/^\s+[A-Z_]+: \$\{\{ [a-z_.]+ \}\}\s*$/);
    });
  });
});
