// created: 2026-11-27 | branch: feat/v60-l0-wp01-ownership-map
// author: Larry Klosowski (L0 lane, L0-WP01, L0-WP01b; Claude Opus 5.5)
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

  it("rejects a trailing-slash glob (CODEOWNERS reads a directory, the matcher a literal path)", () => {
    expect(() => toCodeownersPattern("src/models/")).toThrow();
    expect(() => parseMap("#! reviewers * @o/t\nl5 src/models/\n")).toThrow();
  });

  it("seed rows let L0 create registration points and never change CODEOWNERS", () => {
    const map = read(".agentile/v60-ownership.map");
    const { creates } = parseMap(map);
    expect(creates.length).toBeGreaterThanOrEqual(10);
    for (const c of creates) {
      expect(c.lanes).toEqual(["l0"]);
      expect(() => toCodeownersPattern(c.glob)).not.toThrow();
    }
    const withoutSeeds = map.split("\n").filter((l) => !l.startsWith("create:")).join("\n");
    expect(generate(map)).toBe(generate(withoutSeeds));
    expect(() => parseMap("#! reviewers * @o/t\ncreate:l11 src/a.ts\n")).toThrow();
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
    // entry ahead of it, so SHA^1...SHA (the event sha) is exactly that PR. merge_group.base_sha is the tip of
    // main under the WHOLE group: diffing against it blames an l5 PR for an l4 PR ahead of it.
    const wf = read(".github/workflows/lane-guard.yml");
    expect(wf).not.toContain("merge_group.base_sha");
    expect(wf).toMatch(/^ {10}head=\$\(git rev-parse --verify "\$SHA\^\{commit\}"\)$/m);
    expect(wf).toMatch(/^ {10}base=\$\(git rev-parse --verify "\$head\^1"\)$/m);
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

  // The guard step of lane-guard.yml, run as CI runs it, in a scratch repo whose HEAD is a PR merge
  // commit (HEAD^1 = the base it merges into).
  const guardStep = () => {
    const lines = read(".github/workflows/lane-guard.yml").split("\n");
    const at = lines.findIndex((l) => l.includes("name: The lane owns every changed file"));
    const start = lines.findIndex((l, i) => i > at && /^\s+run: \|$/.test(l)) + 1;
    const body = [];
    for (let j = start; j < lines.length && (!lines[j].trim() || lines[j].startsWith("          ")); j++) body.push(lines[j].slice(10));
    return body.join("\n");
  };
  const scratchPr = (prefix, { baseHasPolicy, baseFiles = {} }) => {
    const tmp = mkdtempSync(path.join(os.tmpdir(), prefix));
    const git = (...a) =>
      execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", ...a], { cwd: tmp, encoding: "utf8" });
    const put = (rel, text) => {
      mkdirSync(path.dirname(path.join(tmp, rel)), { recursive: true });
      writeFileSync(path.join(tmp, rel), text);
    };
    const policy = () => {
      put("scripts/cx-ownership-check.sh", read("scripts/cx-ownership-check.sh"));
      put(".agentile/v60-ownership.map", read(".agentile/v60-ownership.map"));
    };
    git("init", "-q", "-b", "main");
    put("src/shell/store.ts", "export const store = 1;\n");
    for (const [rel, text] of Object.entries(baseFiles)) put(rel, text);
    if (baseHasPolicy) policy();
    git("add", "-A");
    git("commit", "-q", "-m", "base");
    git("checkout", "-q", "-b", "pr");
    return { tmp, git, put, policy };
  };
  // sha: the event commit (github.sha); defaults to the scratch repo's HEAD, the PR merge commit.
  // extra: more step env (LABELS_JSON, CHAR_RESULT, CHAR_GREEN: the resolve and seam
  // characterization job outputs).
  const runGuard = (tmp, branch, sha, extra = {}) => {
    const runnerTemp = mkdtempSync(path.join(os.tmpdir(), "lane-guard-runner-"));
    try {
      const SHA = sha ?? execFileSync("git", ["rev-parse", "HEAD"], { cwd: tmp, encoding: "utf8" }).trim();
      const out = execFileSync("bash", ["-c", guardStep()], {
        cwd: tmp,
        encoding: "utf8",
        stdio: "pipe",
        env: { ...process.env, EVENT: "pull_request", HEAD_REF: branch, MQ_REF: "", REPO: "o/r", SHA, RUNNER_TEMP: runnerTemp, ...extra },
      });
      return { ok: true, out };
    } catch (e) {
      return { ok: false, out: `${e.stdout}${e.stderr}` };
    } finally {
      rmSync(runnerTemp, { recursive: true, force: true });
    }
  };
  const mergeIntoMain = (git) => {
    git("checkout", "-q", "main");
    git("merge", "-q", "--no-ff", "-m", "PR merge commit", "pr");
  };

  it("a lane PR that edits the map to claim a file cannot authorize itself (policy is read from the base)", () => {
    const { tmp, git, put } = scratchPr("lane-guard-self-", { baseHasPolicy: true });
    try {
      // An l1 branch appends rows (last match wins) giving itself the map and an L0 hotspot.
      put(".agentile/v60-ownership.map", `${read(".agentile/v60-ownership.map")}l1 .agentile/v60-ownership.map\nl1 src/shell/store.ts\n`);
      put("src/shell/store.ts", "export const store = 2;\n");
      git("add", "-A");
      git("commit", "-q", "-m", "l1 claims the store");
      mergeIntoMain(git);
      // Control: the PR's own copy of the map would let the change through.
      expect(() => execFileSync("bash", ["scripts/cx-ownership-check.sh", "--map", "v60", "l1", "HEAD^1"], { cwd: tmp, stdio: "pipe" })).not.toThrow();
      const r = runGuard(tmp, "feat/v60-l1-wp01-claim-store");
      expect(r.ok, r.out).toBe(false);
      expect(r.out).toContain("src/shell/store.ts (owner: l0)");
      expect(r.out).toContain(".agentile/v60-ownership.map (owner: l0)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("a lane PR that edits the checker to wave itself through is judged by the base checker", () => {
    const { tmp, git, put } = scratchPr("lane-guard-checker-", { baseHasPolicy: true });
    try {
      put("scripts/cx-ownership-check.sh", "#!/usr/bin/env bash\n[ \"$1\" = --lane-of ] && { printf l1; exit 0; }\necho 'ownership OK'\n");
      put("src/shell/store.ts", "export const store = 2;\n");
      git("add", "-A");
      git("commit", "-q", "-m", "l1 replaces the checker");
      mergeIntoMain(git);
      const r = runGuard(tmp, "feat/v60-l1-wp01-claim-store");
      expect(r.ok, r.out).toBe(false);
      expect(r.out).toContain("scripts/cx-ownership-check.sh (owner: l0)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("the guard runs in its own job, with no PR-controlled step before it", () => {
    // A step that runs the PR's scripts could move HEAD, rewrite .git or put a shim on PATH before
    // the trusted step reads the base policy. The guard job is checkout plus the guard, nothing else.
    const wf = read(".github/workflows/lane-guard.yml");
    const jobs = wf.slice(wf.indexOf("\njobs:\n") + 7).split(/\n(?= {2}[a-z0-9-]+:\n)/);
    const guard = jobs.filter((j) => j.includes("name: The lane owns every changed file"));
    expect(guard).toHaveLength(1);
    const steps = guard[0].split("\n").filter((l) => /^ {6}- /.test(l));
    expect(steps).toHaveLength(2);
    expect(steps[0]).toMatch(/^ {6}- uses: actions\/checkout@[0-9a-f]{40} /);
    expect(guard[0]).not.toContain("--selftest");
    expect(guard[0]).not.toContain("gen-codeowners");
    expect(guard[0]).not.toMatch(/\bnpm\b|\bnode\b/);
    // The PR's own self-consistency checks still run, in another job.
    expect(jobs.some((j) => !guard.includes(j) && j.includes("--selftest") && j.includes("gen-codeowners.mjs --check"))).toBe(true);
    // The judged commit comes from the event, not from whatever HEAD points at.
    expect(guard[0]).toContain("SHA: ${{ github.sha }}");
  });

  it("the guard judges the event commit even if HEAD was moved before it ran", () => {
    const { tmp, git, put } = scratchPr("lane-guard-sha-", { baseHasPolicy: true });
    try {
      put("src/shell/store.ts", "export const store = 2;\n");
      git("add", "-A");
      git("commit", "-q", "-m", "l5 edits the L0 hotspot");
      mergeIntoMain(git);
      const sha = git("rev-parse", "HEAD").trim();
      // A hostile earlier step points HEAD at a commit with an empty diff.
      git("checkout", "-q", "--detach", "HEAD^1");
      const r = runGuard(tmp, "feat/v60-l5-wp02-hf-caps", sha);
      expect(r.ok, r.out).toBe(false);
      expect(r.out).toContain("src/shell/store.ts (owner: l0)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("an L0 seam PR may create other lanes' registration points (spec 2.2)", () => {
    const { tmp, git, put } = scratchPr("lane-guard-seed-", { baseHasPolicy: true });
    try {
      for (const rel of [
        "src-tauri/src/commands/l1.rs",
        "src-tauri/permissions/l3-commands.toml",
        "src/bridge/lanes/l5.ts",
        "src/shell/actions/l4/x.ts",
        "src-tauri/src/lanes/l2/hooks.rs",
      ]) put(rel, "// registration point\n");
      git("add", "-A");
      git("commit", "-q", "-m", "L0 seeds registration points");
      mergeIntoMain(git);
      const ok = runGuard(tmp, "feat/v60-l0-wp03-command-registry");
      expect(ok.ok, ok.out).toBe(true);
      const other = runGuard(tmp, "feat/v60-l2-wp01-x");
      expect(other.ok, other.out).toBe(false);
      expect(other.out).toContain("src-tauri/src/commands/l1.rs (owner: l1)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("once a registration point exists, L0 may not edit or delete it", () => {
    const baseFiles = { "src-tauri/src/commands/l1.rs": "// l1\n", "src/bridge/lanes/l5.ts": "export {};\n" };
    for (const change of ["edit", "delete"]) {
      const { tmp, git, put } = scratchPr(`lane-guard-seed-${change}-`, { baseHasPolicy: true, baseFiles });
      try {
        if (change === "edit") put("src-tauri/src/commands/l1.rs", "// l0 edits l1\n");
        else git("rm", "-q", "src/bridge/lanes/l5.ts");
        git("add", "-A");
        git("commit", "-q", "-m", `l0 ${change}s a lane's registration point`);
        mergeIntoMain(git);
        const r = runGuard(tmp, "feat/v60-l0-wp05-bridge-split");
        expect(r.ok, r.out).toBe(false);
        expect(r.out).toContain(change === "edit" ? "src-tauri/src/commands/l1.rs (owner: l1)" : "src/bridge/lanes/l5.ts (owner: l5)");
      } finally {
        rmSync(tmp, { recursive: true, force: true });
      }
    }
  });

  it("a chain-lane branch gets the frozen-path check here", () => {
    const { tmp, git, put } = scratchPr("lane-guard-chain-", { baseHasPolicy: true, baseFiles: { "src-tauri/addresses/40204.json": "{}\n" } });
    try {
      put("src-tauri/addresses/40204.json", "{\"x\":1}\n");
      git("add", "-A");
      git("commit", "-q", "-m", "chain branch edits the address book");
      mergeIntoMain(git);
      const r = runGuard(tmp, "feat/v60-chain-r1008-pin");
      expect(r.ok, r.out).toBe(false);
      expect(r.out).toContain("src-tauri/addresses/40204.json (owner: frozen)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  // L0-WP01b: the seam exemption through the real step (labels and the characterization result
  // arrive as job outputs; the decision itself is tested in scripts/cx-ownership-check.test.sh).
  it("a labelled L0 seam PR may change a listed foreign file only with the characterization job green", () => {
    const { tmp, git, put } = scratchPr("lane-guard-seam-", { baseHasPolicy: true, baseFiles: { "src-tauri/src/cluster_tests.rs": "// l7\n" } });
    try {
      put("src-tauri/src/cluster_tests.rs", "// the handler list moved\n");
      put(".agentile/v60-seams.allow", "l0-wp03 src-tauri/src/cluster_tests.rs\n");
      git("add", "-A");
      git("commit", "-q", "-m", "L0-WP03 migrates a structural test");
      mergeIntoMain(git);
      const branch = "feat/v60-l0-wp03-command-registry";
      const green = { LABELS_JSON: '["lane:l0","seam"]', CHAR_RESULT: "success", CHAR_GREEN: "true" };
      const ok = runGuard(tmp, branch, undefined, green);
      expect(ok.ok, ok.out).toBe(true);
      expect(ok.out).toContain("seam exemption (l0-wp03)");
      for (const extra of [
        { ...green, LABELS_JSON: '["lane:l0"]' },
        { ...green, CHAR_RESULT: "failure", CHAR_GREEN: "" },
        { ...green, CHAR_RESULT: "skipped", CHAR_GREEN: "" },
        { ...green, CHAR_GREEN: "" },
        {},
      ]) {
        const r = runGuard(tmp, branch, undefined, extra);
        expect(r.ok, `${JSON.stringify(extra)}\n${r.out}`).toBe(false);
        expect(r.out).toContain("src-tauri/src/cluster_tests.rs (owner: l7)");
      }
      const label = runGuard(tmp, "feat/v60-l0-wp11-ratchets", undefined, green);
      expect(label.ok, label.out).toBe(false);
      expect(label.out).toContain("label 'seam'");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("a base checker from before the seam exemption still judges as it did", () => {
    const { tmp, git, put } = scratchPr("lane-guard-legacy-", { baseHasPolicy: false });
    try {
      // The base carries a checker without the lane-guard-api marker (as main did before L0-WP01b).
      put("scripts/cx-ownership-check.sh", read("scripts/cx-ownership-check.sh").replace(/^# lane-guard-api: 2.*\n/m, ""));
      put(".agentile/v60-ownership.map", read(".agentile/v60-ownership.map"));
      git("add", "-A");
      git("commit", "-q", "--amend", "-m", "base");
      git("branch", "-f", "main", "HEAD");
      put("src/shell/store.ts", "export const store = 2;\n");
      git("add", "-A");
      git("commit", "-q", "-m", "l5 edits the L0 hotspot");
      mergeIntoMain(git);
      const green = { LABELS_JSON: '["seam"]', CHAR_RESULT: "success", CHAR_GREEN: "true" };
      const bad = runGuard(tmp, "feat/v60-l5-wp02-hf-caps", undefined, green);
      expect(bad.ok, bad.out).toBe(false);
      expect(bad.out).toContain("src/shell/store.ts (owner: l0)");
      expect(runGuard(tmp, "fix/055-sidecar-flake").ok).toBe(true);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("bootstrap: a base with no v60 policy yet uses the PR's own map and checker, and says so", () => {
    const { tmp, git, policy, put } = scratchPr("lane-guard-boot-", { baseHasPolicy: false });
    try {
      policy();
      put("src/shell/store.ts", "export const store = 2;\n");
      git("add", "-A");
      git("commit", "-q", "-m", "L0 adds the policy");
      mergeIntoMain(git);
      const ok = runGuard(tmp, "feat/v60-l0-wp01-ownership-map");
      expect(ok.ok, ok.out).toBe(true);
      expect(ok.out).toContain("bootstrap");
      const bad = runGuard(tmp, "feat/v60-l5-wp02-hf-caps");
      expect(bad.ok, bad.out).toBe(false);
      expect(bad.out).toContain("src/shell/store.ts (owner: l0)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});

describe("ownership check input handling", () => {
  const scratch = (prefix) => {
    const tmp = mkdtempSync(path.join(os.tmpdir(), prefix));
    mkdirSync(path.join(tmp, "scripts"));
    mkdirSync(path.join(tmp, ".agentile"));
    copyFileSync(path.join(repo, "scripts/cx-ownership-check.sh"), path.join(tmp, "scripts/cx-ownership-check.sh"));
    return tmp;
  };

  it("reads the final map row when the map has no trailing newline", () => {
    const tmp = scratch("lane-guard-eol-");
    try {
      for (const m of ["cx-ownership.map", "v60-ownership.map"]) {
        writeFileSync(path.join(tmp, ".agentile", m), read(`.agentile/${m}`).replace(/\n+$/, ""));
      }
      // The v60 map's last row is the always-allowed sprint glob; the selftest checks it.
      const out = execFileSync("bash", ["scripts/cx-ownership-check.sh", "--selftest"], { cwd: tmp, encoding: "utf8" });
      expect(out).toContain("SELFTEST: PASS");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("attributes a non-ASCII path to its real owner (paths are read NUL-delimited, unquoted)", () => {
    const tmp = scratch("lane-guard-utf8-");
    try {
      copyFileSync(path.join(repo, ".agentile/v60-ownership.map"), path.join(tmp, ".agentile/v60-ownership.map"));
      const git = (...a) =>
        execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", ...a], { cwd: tmp, encoding: "utf8" });
      git("init", "-q");
      git("config", "core.quotePath", "true");
      git("add", "-A");
      git("commit", "-q", "-m", "base");
      mkdirSync(path.join(tmp, "src/shell/actions/l4"), { recursive: true });
      writeFileSync(path.join(tmp, "src/shell/actions/l4/naïve pins.ts"), "export {};\n");
      git("add", "-A");
      git("commit", "-q", "-m", "l4 adds a file with a non-ASCII name");
      const check = (lane) => {
        try {
          return { ok: true, out: execFileSync("bash", ["scripts/cx-ownership-check.sh", "--map", "v60", lane, "HEAD~1"], { cwd: tmp, encoding: "utf8", stdio: "pipe" }) };
        } catch (e) {
          return { ok: false, out: `${e.stdout}${e.stderr}` };
        }
      };
      const own = check("l4");
      expect(own.ok, own.out).toBe(true);
      const other = check("l5");
      expect(other.ok, other.out).toBe(false);
      expect(other.out).toContain("src/shell/actions/l4/naïve pins.ts (owner: l4)");
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("fails closed when the base ref cannot be diffed", () => {
    const tmp = scratch("lane-guard-noref-");
    try {
      copyFileSync(path.join(repo, ".agentile/v60-ownership.map"), path.join(tmp, ".agentile/v60-ownership.map"));
      execFileSync("git", ["init", "-q"], { cwd: tmp });
      expect(() => execFileSync("bash", ["scripts/cx-ownership-check.sh", "--map", "v60", "l4", "no-such-ref"], { cwd: tmp, stdio: "pipe" })).toThrow();
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});
