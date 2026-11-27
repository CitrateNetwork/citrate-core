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
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
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

  it("a lane that moves another lane's file out is still caught (renames list both paths)", () => {
    const tmp = mkdtempSync(path.join(os.tmpdir(), "lane-guard-"));
    try {
      mkdirSync(path.join(tmp, "scripts"));
      mkdirSync(path.join(tmp, ".agentile"));
      mkdirSync(path.join(tmp, "src/shell"), { recursive: true });
      copyFileSync(path.join(repo, "scripts/cx-ownership-check.sh"), path.join(tmp, "scripts/cx-ownership-check.sh"));
      copyFileSync(path.join(repo, ".agentile/v60-ownership.map"), path.join(tmp, ".agentile/v60-ownership.map"));
      writeFileSync(path.join(tmp, "src/shell/store.ts"), "export const store = 1;\n");
      const git = (...a) =>
        execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", ...a], { cwd: tmp, encoding: "utf8" });
      git("init", "-q");
      git("add", "-A");
      git("commit", "-q", "-m", "base");
      mkdirSync(path.join(tmp, "src/models"));
      git("mv", "src/shell/store.ts", "src/models/store.ts");
      git("commit", "-q", "-m", "l5 moves the L0 hotspot into its own directory");
      let failed = false;
      let stderr = "";
      try {
        execFileSync("bash", ["scripts/cx-ownership-check.sh", "--map", "v60", "l5", "HEAD~1"], { cwd: tmp, encoding: "utf8", stdio: "pipe" });
      } catch (e) {
        failed = true;
        stderr = String(e.stderr);
      }
      expect(failed).toBe(true);
      expect(stderr).toContain("src/shell/store.ts (owner: l0)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});

describe("lane-guard workflow", () => {
  it("passes refs through env and never expands them inside a run block", () => {
    const wf = read(".github/workflows/lane-guard.yml");
    expect(wf).toMatch(/^on:\n {2}pull_request:[^\n]*\n {2}merge_group:/m);
    // Collect every run: body (single line or block) and require it to hold no ${{ }} expression.
    const lines = wf.split("\n");
    const runBodies = [];
    lines.forEach((line, i) => {
      const m = /^(\s*)(- )?run: ?(.*)$/.exec(line);
      if (!m) return;
      const indent = m[1].length + (m[2] ? 2 : 0);
      if (m[3] !== "|") return void runBodies.push([i + 1, m[3]]);
      for (let j = i + 1; j < lines.length && (!lines[j].trim() || lines[j].search(/\S/) > indent); j++) {
        runBodies.push([j + 1, lines[j]]);
      }
    });
    expect(runBodies.length).toBeGreaterThan(5);
    for (const [n, body] of runBodies) expect(body, `lane-guard.yml:${n}`).not.toContain("${{");
    // The head ref reaches the shell only as an env value.
    expect(wf).toContain("HEAD_REF: ${{ github.head_ref }}");
  });

  it("a queued PR is judged on its own queue entry, not on the PRs queued ahead of it", () => {
    // In a merge group each queued PR is one commit (squash) or one merge commit on top of the
    // entry ahead of it, so HEAD^1...HEAD is exactly that PR. merge_group.base_sha is the tip of
    // main under the WHOLE group: diffing against it blames an l5 PR for an l4 PR ahead of it.
    const wf = read(".github/workflows/lane-guard.yml");
    expect(wf).not.toContain("merge_group.base_sha");
    expect(wf).toMatch(/^ {10}base="HEAD\^1"$/m);
    expect(wf.match(/base=/g)).toHaveLength(1);

    const tmp = mkdtempSync(path.join(os.tmpdir(), "lane-guard-mq-"));
    try {
      mkdirSync(path.join(tmp, "scripts"));
      mkdirSync(path.join(tmp, ".agentile"));
      copyFileSync(path.join(repo, "scripts/cx-ownership-check.sh"), path.join(tmp, "scripts/cx-ownership-check.sh"));
      copyFileSync(path.join(repo, ".agentile/v60-ownership.map"), path.join(tmp, ".agentile/v60-ownership.map"));
      const git = (...a) =>
        execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", ...a], { cwd: tmp, encoding: "utf8" });
      const commitFile = (rel, msg) => {
        mkdirSync(path.dirname(path.join(tmp, rel)), { recursive: true });
        writeFileSync(path.join(tmp, rel), `${msg}\n`);
        git("add", "-A");
        git("commit", "-q", "-m", msg);
      };
      git("init", "-q");
      git("add", "-A");
      git("commit", "-q", "-m", "main");
      const groupBase = git("rev-parse", "HEAD").trim();
      commitFile("src/shell/actions/l4/pins.ts", "queue entry 1: an l4 PR");
      commitFile("src/shell/actions/l5/caps.ts", "queue entry 2: an l5 PR");
      const run = (base) => {
        try {
          execFileSync("bash", ["scripts/cx-ownership-check.sh", "--map", "v60", "l5", base], { cwd: tmp, encoding: "utf8", stdio: "pipe" });
          return "pass";
        } catch {
          return "fail";
        }
      };
      expect(run("HEAD^1")).toBe("pass");
      expect(run(groupBase)).toBe("fail");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});
