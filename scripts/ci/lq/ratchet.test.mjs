// @vitest-environment node
//
// scripts/ci/lq/ratchet.mjs (LQ-WP01): the coverage baseline is the test runners' own count.
// `measure` reads a `cargo test --workspace --locked` log and a `vitest run --reporter=json`
// file from a Linux CI run, plus the tree (formal specs, Rule-5 frontmatter, CI gate steps), and
// writes .agentile/coverage/ratchet.json (schema v1, lanes/LQ_NON_REGRESSION_GUARD.md section
// 3.1). A run with a failed test is never a baseline.
//
// LQ-WP02: `compare` (the check `lq / ratchet`) judges a head against the merge base's
// ratchet.json. Those tests follow the LQ-WP01 ones; each rule fails on the defect it guards.
// Fixture strings that look like focused or skipped tests are built by concatenation, so this file
// does not itself trip the tree and diff scans it tests.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseCargoLog, parseVitestJson, measureTree, buildRatchet } from "./ratchet.mjs";
import * as R from "./ratchet.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, "ratchet.mjs");

const tmp = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});
function tmpdir() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "lq-ratchet-"));
  tmp.push(d);
  return d;
}
function write(dir, rel, text) {
  const p = path.join(dir, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, text);
  return p;
}

// Shaped like `gh run view --log` output of ci.yml's rust job (job, step and timestamp columns
// before the libtest line), mixed with a bare local line and the doc-test summary.
const P = "rust (fmt, clippy, test, audit)\tUNKNOWN STEP\t2026-11-25T04:50:52.4691791Z ";
const CARGO_OK = [
  `${P}     Running unittests src/lib.rs (target/debug/deps/citrate_components-0f1)`,
  `${P}test archive::tests::unpacks ... ok`,
  `${P}test archive::tests::live_fetch ... ignored, live: needs the network`,
  `${P}test result: ok. 28 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s`,
  `${P}     Running unittests src/lib.rs (target/debug/deps/citrate_core_lib-9a2)`,
  `${P}test fleet_tests::rule3_tripwire_no_silent_peer ... ok`,
  `${P}test honesty::tests::honest_zero_claim ... ok`,
  `${P}test result: ok. 3228 passed; 0 failed; 22 ignored; 0 measured; 0 filtered out; finished in 283.39s`,
  "     Running tests/flows.rs (target/debug/deps/flows-1b2)",
  "test result: ok. 706 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 662.40s",
  `${P}   Doc-tests citrate_components`,
  `${P}test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`,
].join("\n");

const VITEST_OK = {
  numTotalTestSuites: 3,
  numPassedTestSuites: 3,
  numFailedTestSuites: 0,
  numPendingTestSuites: 0,
  numTotalTests: 6,
  numPassedTests: 4,
  numFailedTests: 0,
  numPendingTests: 1,
  numTodoTests: 1,
  success: true,
  testResults: [
    {
      name: "/w/src/a.test.ts",
      status: "passed",
      assertionResults: [
        { fullName: "bridge tripwire: no token crosses", title: "no token crosses", status: "passed" },
        { fullName: "bridge sim mode", title: "sim mode", status: "passed" },
        { fullName: "bridge live provider", title: "live provider", status: "skipped" },
      ],
    },
    {
      name: "/w/scripts/b.test.mjs",
      status: "passed",
      assertionResults: [
        { fullName: "pins agree", title: "pins agree", status: "passed" },
        { fullName: "pins Tripwire: digest", title: "digest", status: "passed" },
        { fullName: "pins later", title: "later", status: "todo" },
      ],
    },
  ],
};

describe("ratchet.mjs measure (LQ-WP01)", () => {
  it('parses cargo "test result" lines across crates', () => {
    const r = parseCargoLog(CARGO_OK);
    expect(r.suites).toBe(4);
    expect(r.passed).toBe(28 + 3228 + 706 + 0);
    expect(r.failed).toBe(0);
    // Test names come from the runner's own per-test lines, prefix columns stripped.
    expect(r.tripwireTests).toEqual(["fleet_tests::rule3_tripwire_no_silent_peer"]);
  });

  it("parses the vitest json reporter totals", () => {
    const r = parseVitestJson(VITEST_OK);
    expect(r.passed).toBe(4);
    // Pending (skipped) and todo are both "not run": the ratchet's skipped axis holds both.
    expect(r.skipped).toBe(2);
    expect(r.failed).toBe(0);
    expect(r.tripwireTests).toEqual(["bridge tripwire: no token crosses", "pins Tripwire: digest"]);
  });

  it("refuses a log with a failed test as a baseline", () => {
    const failedCargo = CARGO_OK.replace(
      "test result: ok. 706 passed; 0 failed;",
      "test result: FAILED. 705 passed; 1 failed;",
    );
    expect(parseCargoLog(failedCargo).failed).toBe(1);
    expect(() => buildRatchet({ cargo: parseCargoLog(failedCargo), vitest: parseVitestJson(VITEST_OK) })).toThrow(
      /failed/,
    );
    const failedVitest = { ...VITEST_OK, numFailedTests: 1, success: false };
    expect(() => buildRatchet({ cargo: parseCargoLog(CARGO_OK), vitest: parseVitestJson(failedVitest) })).toThrow(
      /failed/,
    );
    // An empty or truncated log is not a count of zero.
    expect(() => buildRatchet({ cargo: parseCargoLog("Compiling citrate v0.5.9\n"), vitest: parseVitestJson(VITEST_OK) })).toThrow(
      /no "test result" line/,
    );

    // The CLI refuses too: exit 1, and no ratchet.json is written.
    const dir = tmpdir();
    const log = write(dir, "cargo.log", failedCargo);
    const vj = write(dir, "vitest.json", JSON.stringify(VITEST_OK));
    const out = path.join(dir, "ratchet.json");
    const res = spawnSync(
      process.execPath,
      [
        SCRIPT, "measure", "--cargo-log", log, "--cargo-exit", "101", "--vitest-json", vj,
        "--root", dir, "--rev", "a".repeat(40), "--out", out,
      ],
      { encoding: "utf8" },
    );
    expect(res.status).toBe(1);
    expect(res.stderr).toMatch(/failed/);
    expect(fs.existsSync(out)).toBe(false);
  });

  it("counts #[ignore] separately", () => {
    const r = parseCargoLog(CARGO_OK);
    // 1 + 22 + 1 ignored, never folded into passed.
    expect(r.ignored).toBe(24);
    expect(r.passed).toBe(3962);
    const withIgnoredOnly = parseCargoLog(
      "test result: ok. 0 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.00s",
    );
    expect(withIgnoredOnly).toMatchObject({ passed: 0, ignored: 5, failed: 0 });
  });

  it("measures specs, Rule-5 frontmatter and CI gate steps from the tree, and writes schema v1", () => {
    const dir = tmpdir();
    const full = "---\ncreated: 2026-11-26\nbranch: main\nauthor: Larry Klosowski\nstatus: active\n---\n# x\n";
    write(dir, "docs/a.md", full);
    write(dir, "docs/sub/b.md", full);
    write(dir, ".agentile/c.md", "---\ncreated: 2026-11-26\nauthor: x\nstatus: draft\n---\n# no branch\n");
    write(dir, ".agentile/d.md", "# no frontmatter at all\n");
    write(dir, "README.md", "# outside docs/ and .agentile/, not counted\n");
    write(dir, "src-tauri/formal/A.tla", "---- MODULE A ----\n====\n");
    write(dir, "src-tauri/formal/B.tla", "---- MODULE B ----\n====\n");
    // EXPECT lines, but no tlc_expect.py and no tla-models.yml in this tree: CI checks none of
    // them, so they add nothing to tlc_configs_in_ci (the runner test below covers discovery).
    write(dir, "src-tauri/formal/A.cfg", "\\* EXPECT: pass\nINIT Init\n");
    write(dir, "src-tauri/formal/A_bug.cfg", "\\* EXPECT: violated Safety\nINIT Init\n");
    write(dir, "src-tauri/formal/B.cfg", "INIT Init\n");
    write(
      dir,
      ".github/workflows/ci.yml",
      [
        "jobs:",
        "  rust:",
        "    steps:",
        "      - uses: actions/checkout@x",
        "      - name: Authenticate to private federation repos (citrate-chain)",
        "        run: git config",
        "      - name: Install Tauri Linux system deps",
        "        run: |",
        "          sudo apt-get install -y x",
        "      - name: fmt",
        "        run: cargo fmt --all -- --check",
        "      - name: test",
        "        run: cargo test --workspace --locked",
        "  node:",
        "    steps:",
        "      - run: npm ci",
        "      - name: typecheck",
        "        run: npm run typecheck",
        "",
      ].join("\n"),
    );
    write(
      dir,
      ".github/workflows/merge-gate.yml",
      "jobs:\n  ci:\n    steps:\n      - name: Wait for every other check on this commit\n        env:\n          X: y\n        run: |\n          echo wait\n",
    );
    write(
      dir,
      ".github/workflows/eval-check.yml",
      "jobs:\n  e:\n    steps:\n      - name: Fetch the skill sources\n        run: x\n      - name: Eval check\n        run: node e.mjs\n      - name: Upload the scorecards\n        uses: actions/upload-artifact@x\n",
    );
    write(dir, ".github/workflows/other.yml", "jobs:\n  o:\n    steps:\n      - name: not a gate\n        run: x\n");

    const t = measureTree(dir);
    expect(t.specs).toEqual({ tla: 2, tlc_configs_in_ci: 0 });
    expect(t.frontmatter.docs).toBe(4);
    expect(t.frontmatter.with).toBe(2);
    expect(t.frontmatter.missing.sort()).toEqual([".agentile/c.md", ".agentile/d.md"]);
    // fmt, test, typecheck (ci.yml) + wait (merge-gate) + eval check (eval-check); setup steps
    // (authenticate, install, fetch, upload) and unnamed steps are not gates.
    expect(t.ci_steps).toBe(5);

    const r = buildRatchet({
      cargo: parseCargoLog(CARGO_OK),
      vitest: parseVitestJson(VITEST_OK),
      tree: t,
      rev: "b".repeat(40),
      measured: "2026-11-26T00:00:00Z",
    });
    expect(r).toEqual({
      version: 1,
      rev: "b".repeat(40),
      measured: "2026-11-26T00:00:00Z",
      tests: { rust: { passed: 3962, ignored: 24 }, vitest: { passed: 4, skipped: 2 } },
      specs: { tla: 2, tlc_configs_in_ci: 0 },
      tripwires: { registered: 3, ci_steps: 5 },
      frontmatter: { docs: 4, with: 2 },
    });
  });
});

// A workflow file, split into jobs and steps by indentation (no YAML dependency).
function workflowJobs(text) {
  const jobs = [];
  let inJobs = false;
  let job = null;
  let step = null;
  for (const line of text.split(/\r?\n/)) {
    if (/^\S/.test(line)) {
      inJobs = /^jobs:\s*$/.test(line);
      job = null;
      continue;
    }
    if (!inJobs) continue;
    if (/^ {2}[\w-]+:\s*$/.test(line)) {
      job = { name: line.trim().slice(0, -1), head: [], steps: [], inSteps: false };
      jobs.push(job);
      step = null;
    } else if (!job) {
      continue;
    } else if (/^ {4}steps:\s*$/.test(line)) {
      job.inSteps = true;
    } else if (job.inSteps && /^ {6}- /.test(line)) {
      step = [line];
      job.steps.push(step);
    } else if (job.inSteps && step && (/^ {8}/.test(line) || line.trim() === "")) {
      step.push(line);
    } else if (!job.inSteps) {
      job.head.push(line);
    }
  }
  return jobs.map((j) => ({ name: j.name, head: j.head.join("\n"), steps: j.steps.map((st) => st.join("\n")) }));
}

// Review round on c56bf1f6 (PR #786): a partial cargo log, an orphan or invalid TLC config, and
// the private-repo credential reaching PR-controlled tests or an uploaded artifact.
describe("ratchet.mjs hardening (LQ-WP01 review)", () => {
  const ROOT = path.resolve(here, "..", "..", "..");
  const run = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });

  // One suite passes, then the next test binary dies before libtest prints its summary; cargo
  // --no-fail-fast carries on and exits non-zero.
  const CARGO_ABORTED = [
    "     Running unittests src/lib.rs (target/debug/deps/citrate_components-0f1)",
    "test result: ok. 28 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s",
    "     Running unittests src/lib.rs (target/debug/deps/citrate_core_lib-9a2)",
    "test fleet_tests::rule3_tripwire_no_silent_peer ... ok",
    "error: test failed, to rerun pass `-p citrate-core --lib`",
    "",
    "Caused by:",
    "  process didn't exit successfully: `target/debug/deps/citrate_core_lib-9a2` (signal: 11, SIGSEGV: invalid memory reference)",
  ].join("\n");

  it("refuses a log where a later test binary aborted after an earlier suite passed", () => {
    const r = parseCargoLog(CARGO_ABORTED);
    expect(r).toMatchObject({ suites: 1, headers: 2, failed: 0 });
    expect(() => buildRatchet({ cargo: r, vitest: parseVitestJson(VITEST_OK) })).toThrow(/2 test targets started, 1 summary/);

    const dir = tmpdir();
    const log = write(dir, "cargo.log", CARGO_ABORTED);
    const vj = write(dir, "vitest.json", JSON.stringify(VITEST_OK));
    const out = path.join(dir, "ratchet.json");
    // Even a caller that claims cargo exited 0 gets a refusal: the log itself is incomplete.
    for (const exit of ["0", "101"]) {
      const res = run(["measure", "--cargo-log", log, "--cargo-exit", exit, "--vitest-json", vj, "--root", dir, "--out", out]);
      expect(res.status).toBe(1);
      expect(fs.existsSync(out)).toBe(false);
    }
  });

  it("measure needs cargo's own exit status next to a raw log", () => {
    const dir = tmpdir();
    const log = write(dir, "cargo.log", CARGO_OK);
    const vj = write(dir, "vitest.json", JSON.stringify(VITEST_OK));
    const out = path.join(dir, "ratchet.json");
    const missing = run(["measure", "--cargo-log", log, "--vitest-json", vj, "--root", dir, "--out", out]);
    expect(missing.status).toBe(2);
    expect(missing.stderr).toMatch(/--cargo-exit/);
    const red = run(["measure", "--cargo-log", log, "--cargo-exit", "101", "--vitest-json", vj, "--root", dir, "--out", out]);
    expect(red.status).toBe(1);
    expect(red.stderr).toMatch(/exited 101/);
    expect(fs.existsSync(out)).toBe(false);
    const ok = run(["measure", "--cargo-log", log, "--cargo-exit", "0", "--vitest-json", vj, "--root", dir, "--out", out]);
    expect(ok.status).toBe(0);
    expect(JSON.parse(fs.readFileSync(out, "utf8")).tests.rust).toEqual({ passed: 3962, ignored: 24 });
  });

  it("reads cargo's colored output (CARGO_TERM_COLOR=always) the same as plain", () => {
    const colored = CARGO_OK.replace(/(\s)(Running|Doc-tests)\b/g, "$1\u001b[1m\u001b[92m$2\u001b[0m");
    expect(colored).not.toBe(CARGO_OK);
    expect(parseCargoLog(colored)).toEqual(parseCargoLog(CARGO_OK));
    expect(parseCargoLog(CARGO_OK).headers).toBe(4);
  });

  it("cargo-counts publishes validated counts only, and measure accepts them", () => {
    const dir = tmpdir();
    const log = write(dir, "cargo.log", CARGO_OK);
    const counts = path.join(dir, "cargo-counts.json");
    const res = run(["cargo-counts", "--cargo-log", log, "--cargo-exit", "0", "--out", counts]);
    expect(res.status).toBe(0);
    const c = JSON.parse(fs.readFileSync(counts, "utf8"));
    expect(c).toEqual({
      version: 1,
      kind: "cargo-counts",
      exit: 0,
      suites: 4,
      headers: 4,
      passed: 3962,
      failed: 0,
      ignored: 24,
      tripwireTests: ["fleet_tests::rule3_tripwire_no_silent_peer"],
    });
    // No raw log line is carried: only the counts and the tripwire test names.
    expect(fs.readFileSync(counts, "utf8")).not.toMatch(/Running|test result|target\/debug/);

    const vj = write(dir, "vitest.json", JSON.stringify(VITEST_OK));
    const out = path.join(dir, "ratchet.json");
    const m = run(["measure", "--cargo-counts", counts, "--vitest-json", vj, "--root", dir, "--out", out]);
    expect(m.status).toBe(0);
    expect(JSON.parse(fs.readFileSync(out, "utf8")).tests.rust).toEqual({ passed: 3962, ignored: 24 });

    // A red or partial run writes no counts file, and a doctored counts file is refused.
    const red = path.join(dir, "red-counts.json");
    expect(run(["cargo-counts", "--cargo-log", log, "--cargo-exit", "101", "--out", red]).status).toBe(1);
    expect(fs.existsSync(red)).toBe(false);
    const aborted = write(dir, "aborted.log", CARGO_ABORTED);
    expect(run(["cargo-counts", "--cargo-log", aborted, "--cargo-exit", "0", "--out", red]).status).toBe(1);
    expect(fs.existsSync(red)).toBe(false);
    const doctored = write(dir, "doctored.json", JSON.stringify({ ...c, headers: 5 }));
    expect(run(["measure", "--cargo-counts", doctored, "--vitest-json", vj, "--root", dir, "--out", out]).status).toBe(1);
  });

  it("tlc_configs_in_ci comes from the TLC runner's own discovery and plan", () => {
    const dir = tmpdir();
    for (const f of ["tlc_expect.py", "tla_frame_lint.py"]) {
      write(dir, `src-tauri/formal/${f}`, fs.readFileSync(path.join(ROOT, "src-tauri", "formal", f), "utf8"));
    }
    write(dir, ".github/workflows/tla-models.yml", fs.readFileSync(path.join(ROOT, ".github/workflows/tla-models.yml"), "utf8"));
    const mod = (m) => `---- MODULE ${m} ----\n====\n`;
    write(dir, "src-tauri/formal/A.tla", mod("A"));
    write(dir, "src-tauri/formal/A.cfg", "\\* EXPECT: pass\nINIT Init\n"); // planned
    write(dir, "src-tauri/formal/A_Mut.cfg", "\\* EXPECT: violated Safety\nINIT Init\n"); // planned
    write(dir, "src-tauri/formal/Orphan_X.cfg", "\\* EXPECT: pass\nINIT Init\n"); // no Orphan.tla: never run
    write(dir, "src-tauri/formal/B.tla", mod("B"));
    write(dir, "src-tauri/formal/B.cfg", "\\* EXPECT: ok\nINIT Init\n"); // not a valid expectation: B is not discovered
    write(dir, "src-tauri/formal/C.tla", mod("C")); // no configs at all
    expect(measureTree(dir).specs).toEqual({ tla: 3, tlc_configs_in_ci: 2 });

    // An invalid expectation inside a checked spec stops the runner before TLC runs: CI checks
    // nothing, so nothing counts.
    write(dir, "src-tauri/formal/A_Bad.cfg", "\\* EXPECT: violation Safety\nINIT Init\n");
    const broken = measureTree(dir);
    expect(broken.specs.tlc_configs_in_ci).toBe(0);
    expect(broken.tlc_plan_errors).toEqual(["A_Bad: no EXPECT line"]);
    fs.rmSync(path.join(dir, "src-tauri/formal/A_Bad.cfg"));

    // A workflow that no longer runs the runner over every spec checks none of them.
    write(dir, ".github/workflows/tla-models.yml", "name: tla models\njobs: {}\n");
    expect(measureTree(dir).specs.tlc_configs_in_ci).toBe(0);
  });

  it("the measured tree today: 61 configs under TLC in CI, from the runner", () => {
    const t = measureTree(ROOT);
    expect(t.tlc_plan_errors).toEqual([]);
    // 61 at the lane start (SidecarSupervisor 39, AppExitCoordinator 22); LQ-WP09 only raises it.
    expect(t.specs.tlc_configs_in_ci).toBeGreaterThanOrEqual(61);
  });


  it("the chain read token reaches only the cargo fetch step, never the tests or an artifact", () => {
    const wf = fs.readFileSync(path.join(ROOT, ".github/workflows/lq-ratchet.yml"), "utf8");
    const jobs = workflowJobs(wf);
    // LQ-WP02 adds `lq`, the reusable call to lq-compare.yml; it has no steps and takes no secret.
    expect(jobs.map((j) => j.name)).toEqual(["cargo-test", "vitest-json", "measure", "lq"]);
    for (const j of jobs) {
      // No job-wide secret: a job env is inherited by every step, PR-controlled ones included.
      expect(j.head).not.toMatch(/secrets\./);
    }
    const all = jobs.flatMap((j) => j.steps);
    const withToken = all.filter((s) => s.includes("secrets.CITRATE_CHAIN_READ_TOKEN"));
    expect(withToken.length).toBeGreaterThan(0);
    for (const s of withToken) {
      expect(s).toMatch(/cargo fetch --locked/);
      expect(s).not.toMatch(/cargo (test|build|run|check|clippy)\b/);
      // The rewrite lives in that step's process env only, never in a git config file on disk.
      expect(s).not.toMatch(/git config/);
    }
    const tests = all.filter((s) => /cargo test\b/.test(s));
    expect(tests.length).toBe(1);
    for (const s of tests) expect(s).not.toMatch(/CHAIN_TOKEN|secrets\./);
    // Only validated counts leave the cargo job; the raw test output stays in the job log, which
    // GitHub masks.
    const uploads = all.filter((s) => s.includes("actions/upload-artifact"));
    for (const s of uploads) expect(s).not.toMatch(/cargo-test\.log/);
    expect(uploads.some((s) => s.includes("cargo-counts.json"))).toBe(true);
  });
});

// ---- LQ-WP02: compare ----

const repo = path.resolve(here, "..", "..", "..");
const ONLY = "it" + ".only";
const SKIP = "it" + ".skip";
const IGNORE = "#[" + "ignore";
function tree(files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lq-ratchet-"));
  tmp.push(dir);
  for (const [p, body] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, p)), { recursive: true });
    fs.writeFileSync(path.join(dir, p), body);
  }
  return dir;
}
function doc(over = {}) {
  const d = {
    version: 1, rev: "b".repeat(40), measured: "2026-11-26T00:00:00Z",
    tests: { rust: { passed: 3218, ignored: 38 }, vitest: { passed: 5200, skipped: 9 } },
    specs: { tla: 15, tlc_configs_in_ci: 61 },
    tripwires: { registered: 0, ci_steps: 22 },
    frontmatter: { docs: 261, with: 261 },
  };
  for (const [k, v] of Object.entries(over)) {
    const [a, b, c] = k.split(".");
    if (c) d[a][b][c] = v; else d[a][b] = v;
  }
  return d;
}
const rustDiff = (lines) => ["diff --git a/src-tauri/src/x_tests.rs b/src-tauri/src/x_tests.rs",
  "--- a/src-tauri/src/x_tests.rs", "+++ b/src-tauri/src/x_tests.rs", "@@ -10,6 +10,6 @@", ...lines].join("\n");
const tsDiff = (lines) => ["diff --git a/src/a.test.ts b/src/a.test.ts",
  "--- a/src/a.test.ts", "+++ b/src/a.test.ts", "@@ -1,4 +1,4 @@", ...lines].join("\n");

const VIT = (over = {}) => ({
  numPassedTests: 1, numPendingTests: 0, numTodoTests: 0, numFailedTests: 0, numFailedTestSuites: 0, success: true,
  testResults: [], ...over,
});
const FULL_FM = "---\ncreated: 2026-11-26\nbranch: main\nauthor: Larry Klosowski\nstatus: active\n---\n# x\n";

describe("compare: the committed baseline and the tripwire registry", () => {
  it("registered tripwires: the runners' passed tripwire tests until the LQ-WP03 registry exists, then its live entries", () => {
    const cargoLog = ["     Running unittests src/lib.rs (target/debug/deps/x-1)",
      "test fleet_tests::rule3_tripwire_no_silent_peer ... ok", "test x::tripwire_ignored ... ignored",
      "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"].join("\n");
    const vitestJson = VIT({ testResults: [{ assertionResults: [
      { fullName: "bridge Tripwire: no token crosses", status: "passed" }, { fullName: "tripwire later", status: "skipped" }] }] });
    const at = (root) => R.buildRatchet({ cargo: R.parseCargoLog(cargoLog), vitest: R.parseVitestJson(vitestJson), tree: R.measureTree(root) });
    expect(at(tree({})).tripwires.registered).toBe(2);
    const withRegistry = tree({ ".agentile/coverage/tripwires.json": JSON.stringify({ version: 1, tripwires: [
      { id: "a", replaced_by: null }, { id: "b", replaced_by: null }, { id: "c", replaced_by: null }, { id: "old", replaced_by: "c" }] }) });
    expect(at(withRegistry).tripwires.registered).toBe(3);
    const broken = tree({ ".agentile/coverage/tripwires.json": JSON.stringify({ version: 1 }) });
    expect(() => at(broken)).toThrow(/no tripwires array/);
  });

  it("the committed LQ-WP01 baseline is a valid base: every axis is a count, it compares clean with itself, one test less fails", () => {
    const base = JSON.parse(fs.readFileSync(path.join(repo, ".agentile/coverage/ratchet.json"), "utf8"));
    for (const k of R.AXES) expect(typeof k.split(".").reduce((o, s) => o?.[s], base), k).toBe("number");
    expect(R.compare(base, base, { root: tree({}) })).toMatchObject({ ok: true, failures: [] });
    const less = JSON.parse(JSON.stringify(base));
    less.tests.rust.passed -= 1;
    expect(R.compare(base, less, { root: tree({}) }).ok).toBe(false);
  });
});

describe("compare: axes", () => {
  it("a decrease without ratchet-adr fails, naming the axis and the delta", () => {
    const r = R.compare(doc(), doc({ "tests.rust.passed": 3217 }), { root: tree({}) });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/tests\.rust\.passed decreased 3218 -> 3217 \(-1\)/);
  });

  it("a decrease with ratchet-adr and a linked ADR passes", () => {
    const diff = rustDiff(["-#[test]", "-fn drops_the_old_probe() {", "-}", " fn keep() {}"]);
    const root = tree({ "docs/adr/ADR-2026-11-26-drop-probe.md":
      "---\ncreated: 2026-11-26\n---\nRemoves drops_the_old_probe; tests.rust.passed goes down by one.\n" });
    const r = R.compare(doc(), doc({ "tests.rust.passed": 3217 }), {
      root, diff, labels: ["ratchet-adr"], body: "ADR: docs/adr/ADR-2026-11-26-drop-probe.md",
    });
    expect(r.failures).toEqual([]);
    expect(r.ok).toBe(true);
    expect(r.notes.join("\n")).toMatch(/accepted by docs\/adr\/ADR-2026-11-26-drop-probe\.md/);
  });

  it("ratchet-adr alone does not pass: the ADR must exist and name the axis and each removed test", () => {
    const diff = rustDiff(["-#[test]", "-fn drops_the_old_probe() {", "-}"]);
    const head = doc({ "tests.rust.passed": 3217 });
    const noLink = R.compare(doc(), head, { root: tree({}), diff, labels: ["ratchet-adr"], body: "trust me" });
    expect(noLink.ok).toBe(false);
    expect(noLink.failures.join("\n")).toMatch(/no docs\/adr\/.*\.md linked/);
    const missing = R.compare(doc(), head, { root: tree({}), diff, labels: ["ratchet-adr"], body: "docs/adr/ADR-x.md" });
    expect(missing.ok).toBe(false);
    expect(missing.failures.join("\n")).toMatch(/does not exist/);
    const vague = R.compare(doc(), head, {
      root: tree({ "docs/adr/ADR-x.md": "tests.rust.passed goes down\n" }), diff, labels: ["ratchet-adr"], body: "docs/adr/ADR-x.md",
    });
    expect(vague.ok).toBe(false);
    expect(vague.failures.join("\n")).toMatch(/drops_the_old_probe/);
    const pathy = R.compare(doc(), head, { root: tree({}), diff, labels: ["ratchet-adr"], body: "docs/adr/../../etc/x.md" });
    expect(pathy.ok).toBe(false);
  });

  it("formal spec count decrease fails", () => {
    const r = R.compare(doc(), doc({ "specs.tla": 14 }), { root: tree({}) });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/specs\.tla decreased 15 -> 14 \(-1\)/);
  });

  it("tlc configs in CI decrease fails", () => {
    const r = R.compare(doc(), doc({ "specs.tlc_configs_in_ci": 60 }), { root: tree({}) });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/specs\.tlc_configs_in_ci decreased 61 -> 60/);
  });

  it("frontmatter coverage decrease fails (a doc added without frontmatter)", () => {
    const r = R.compare(doc(), doc({ "frontmatter.docs": 262 }), { root: tree({}) });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/frontmatter: docs without frontmatter 0 -> 1/);
    // Deleting a doc that had frontmatter is not a coverage regression.
    expect(R.compare(doc(), doc({ "frontmatter.docs": 260, "frontmatter.with": 260 }), { root: tree({}) }).ok).toBe(true);
  });

  it("vitest passed, registered tripwires and CI steps are ratcheted too; growth passes", () => {
    for (const k of ["tests.vitest.passed", "tripwires.ci_steps"]) {
      expect(R.compare(doc(), doc({ [k]: 1 }), { root: tree({}) }).ok).toBe(false);
    }
    expect(R.compare(doc({ "tripwires.registered": 3 }), doc({ "tripwires.registered": 2 }), { root: tree({}) }).ok).toBe(false);
    expect(R.compare(doc(), doc({ "tests.rust.passed": 3300, "specs.tla": 16 }), { root: tree({}) })).toMatchObject({ ok: true, failures: [] });
  });

  it("with no baseline at the merge base it says so and still runs the diff and tree rules", () => {
    const r = R.compare(null, doc(), { root: tree({}), diff: tsDiff([`+${ONLY}("x", () => {});`]) });
    expect(r.ok).toBe(false);
    expect(r.notes.join("\n")).toMatch(/no ratchet\.json at the merge base/);
    expect(R.compare(null, doc(), { root: tree({}) }).ok).toBe(true);
  });
});

describe("compare: focused, ignored and skipped tests", () => {
  it("it.only anywhere fails: in the diff, and in the tree even with no diff", () => {
    const viaDiff = R.compare(doc(), doc(), { root: tree({}), diff: tsDiff([`+  ${ONLY}("focus", () => {}); // #123 reason`]) });
    expect(viaDiff.ok).toBe(false);
    expect(viaDiff.failures.join("\n")).toMatch(/focused test/);
    const root = tree({ "src/deep/x.test.tsx": `describe("a", () => {\n  ${"describe" + ".only"}("b", () => {});\n});\n` });
    const viaTree = R.compare(doc(), doc(), { root });
    expect(viaTree.ok).toBe(false);
    expect(viaTree.failures.join("\n")).toMatch(/src\/deep\/x\.test\.tsx:2/);
    // A comment that mentions the word is not a focused test.
    expect(R.compare(doc(), doc(), { root: tree({ "src/y.test.ts": `// never use ${ONLY} here\n` }) }).ok).toBe(true);
  });

  it("new #[ignore] without a reason fails", () => {
    for (const line of [`+${IGNORE}]`, `+${IGNORE} = ""]`, `+${IGNORE} = "slow"]`]) {
      const r = R.compare(doc(), doc(), { root: tree({}), diff: rustDiff([" #[test]", line, " fn slow() {}"]) });
      expect(r.ok, line).toBe(false);
      expect(r.failures.join("\n")).toMatch(/reason and an issue link: src-tauri\/src\/x_tests\.rs:11: /);
    }
  });

  it("new #[ignore] with a reason and an issue link passes, inline or in the comment above", () => {
    const inline = rustDiff([" #[test]", `+${IGNORE} = "needs anvil; tracked in #812"]`, " fn slow() {}"]);
    expect(R.compare(doc(), doc(), { root: tree({}), diff: inline }).ok).toBe(true);
    const above = rustDiff([" #[test]", "+// live only: needs the funded key (https://github.com/CitrateNetwork/citrate-core/issues/812)",
      `+${IGNORE}]`, " fn slow() {}"]);
    expect(R.compare(doc(), doc(), { root: tree({}), diff: above }).ok).toBe(true);
    // A comment that merely mentions the attribute is not an ignore.
    expect(R.compare(doc(), doc(), { root: tree({}), diff: rustDiff([`+// the live proof is ${IGNORE}]d in CI`]) }).ok).toBe(true);
  });

  it("a new it.skip, todo or skipIf needs a reason comment and an issue link", () => {
    const bare = R.compare(doc(), doc(), { root: tree({}), diff: tsDiff([`+${SKIP}("later", () => {});`]) });
    expect(bare.ok).toBe(false);
    expect(bare.failures.join("\n")).toMatch(/reason and an issue link: src\/a\.test\.ts:1: /);
    const todo = R.compare(doc(), doc(), { root: tree({}), diff: tsDiff([`+${"it" + ".todo"}("x");`]) });
    expect(todo.ok).toBe(false);
    const reasoned = tsDiff(["+// skipped until the relay fixture lands (#801)", `+${SKIP}("later", () => {});`]);
    expect(R.compare(doc(), doc(), { root: tree({}), diff: reasoned }).ok).toBe(true);
    // A removed skip is never a failure.
    expect(R.compare(doc(), doc(), { root: tree({}), diff: tsDiff([`-${SKIP}("later", () => {});`]) }).ok).toBe(true);
  });

  it("collects the names of removed rust and vitest tests from the diff", () => {
    const diff = [rustDiff(["-#[tokio::test]", "-async fn gone_async() {", "-}", "-#[test]", "-fn gone_sync() {}"]),
      tsDiff(['-  it("drops the probe", () => {', "-  });", "-  test(`second one`, () => {});"])].join("\n");
    expect(R.scanDiff(diff).removedTests).toEqual(["gone_async", "gone_sync", "drops the probe", "second one"]);
  });
});

// Review of #790: the committed floor (.agentile/coverage/ratchet.json in the head) is ratcheted
// like a measured axis, an accepted decrease lowers it in the same PR, and a removed test needs
// the ADR whatever the counts say (the floor only advances when the record PR lands, so it can
// lag the tree).
describe("compare: the committed floor and removed tests", () => {
  it("a PR that lowers ratchet.json fails without ratchet-adr, even when the measured counts held", () => {
    const r = R.compare(doc(), doc(), { root: tree({}), floor: doc({ "tests.rust.passed": 3000 }) });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/ratchet\.json lowers tests\.rust\.passed 3218 -> 3000 \(-218\)/);
    const fm = R.compare(doc(), doc(), { root: tree({}), floor: doc({ "frontmatter.docs": 262 }) });
    expect(fm.ok).toBe(false);
    expect(fm.failures.join("\n")).toMatch(/ratchet\.json lowers frontmatter/);
    const adr = tree({ "docs/adr/ADR-floor.md": "Lowers the tests.rust.passed floor.\n" });
    const accepted = R.compare(doc(), doc(), {
      root: adr, floor: doc({ "tests.rust.passed": 3000 }), labels: ["ratchet-adr"], body: "docs/adr/ADR-floor.md",
    });
    expect(accepted).toMatchObject({ ok: true, failures: [] });
    // A floor that rises with the tree, or stays, passes.
    const up = doc({ "tests.rust.passed": 3300 });
    expect(R.compare(doc(), up, { root: tree({}), floor: up })).toMatchObject({ ok: true, failures: [] });
    expect(R.compare(doc(), doc(), { root: tree({}), floor: doc() })).toMatchObject({ ok: true, failures: [] });
  });

  it("a head that deletes ratchet.json is refused, label or not", () => {
    const adr = tree({ "docs/adr/ADR-x.md": "tests.rust.passed\n" });
    for (const labels of [[], ["ratchet-adr"]]) {
      const r = R.compare(doc(), doc(), { root: adr, floor: null, labels, body: "docs/adr/ADR-x.md" });
      expect(r.ok).toBe(false);
      expect(r.failures.join("\n")).toMatch(/ratchet\.json is missing or unreadable in the head/);
    }
  });

  it("an accepted decrease must lower ratchet.json to the measured count in the same PR, so the next PR is not refused", () => {
    const diff = rustDiff(["-#[test]", "-fn drops_the_old_probe() {", "-}"]);
    const root = tree({ "docs/adr/ADR-drop.md": "Removes drops_the_old_probe; tests.rust.passed goes down by one.\n" });
    const opts = { root, diff, labels: ["ratchet-adr"], body: "docs/adr/ADR-drop.md" };
    const stale = R.compare(doc(), doc({ "tests.rust.passed": 3217 }), { ...opts, floor: doc() });
    expect(stale.ok).toBe(false);
    expect(stale.failures.join("\n")).toMatch(/tests\.rust\.passed: an accepted decrease must lower \.agentile\/coverage\/ratchet\.json to 3217 or less in the same PR/);
    const lowered = R.compare(doc(), doc({ "tests.rust.passed": 3217 }), { ...opts, floor: doc({ "tests.rust.passed": 3217 }) });
    expect(lowered).toMatchObject({ ok: true, failures: [] });
    // The next PR is judged against the lowered floor and passes.
    const next = doc({ "tests.rust.passed": 3217 });
    expect(R.compare(next, next, { root: tree({}), floor: next })).toMatchObject({ ok: true, failures: [] });
  });

  it("a test the diff removes and does not add back needs ratchet-adr even when the counts grew", () => {
    const grown = doc({ "tests.rust.passed": 3300, "tests.vitest.passed": 5300 });
    const rust = rustDiff(["-#[test]", "-fn guards_the_bridge() {", "-}"]);
    const r = R.compare(doc(), grown, { root: tree({}), diff: rust, floor: doc() });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/test removed and not added back: guards_the_bridge/);
    const ts = R.compare(doc(), grown, { root: tree({}), diff: tsDiff(['-  it("keeps the probe", () => {', "-  });"]), floor: doc() });
    expect(ts.ok).toBe(false);
    expect(ts.failures.join("\n")).toMatch(/keeps the probe/);
    const adr = tree({ "docs/adr/ADR-bridge.md": "Removes guards_the_bridge, replaced by the fleet suite.\n" });
    expect(R.compare(doc(), grown, { root: adr, diff: rust, floor: doc(), labels: ["ratchet-adr"], body: "docs/adr/ADR-bridge.md" }))
      .toMatchObject({ ok: true, failures: [] });
    // Moved to another file, or re-signed in place (sync to async): added back, so not a removal.
    const moved = [rust, "diff --git a/src-tauri/src/y_tests.rs b/src-tauri/src/y_tests.rs",
      "--- a/src-tauri/src/y_tests.rs", "+++ b/src-tauri/src/y_tests.rs", "@@ -1,2 +1,5 @@",
      "+#[tokio::test]", "+async fn guards_the_bridge() {", "+}"].join("\n");
    expect(R.compare(doc(), grown, { root: tree({}), diff: moved, floor: doc() })).toMatchObject({ ok: true, failures: [] });
    const resigned = tsDiff(['-  it("keeps the probe", () => {', '+  it("keeps the probe", async () => {']);
    expect(R.compare(doc(), grown, { root: tree({}), diff: resigned, floor: doc() })).toMatchObject({ ok: true, failures: [] });
  });

  it("a rust test renamed under an unchanged #[test] line counts as removed (old name) and added (new name)", () => {
    const s = R.scanDiff(rustDiff([" #[test]", "-fn old_name() {", "+fn new_name() {", " }"]));
    expect(s.removedTests).toEqual(["old_name"]);
    expect(s.addedTests).toEqual(["new_name"]);
  });
});


// Review of #790 (round 3), reproduced: (1) deleting a whole test file skipped the removed-test
// rule, because the file name was taken from `+++ /dev/null`; (2) a head could raise its committed
// ratchet.json above its own measured count and pass, after which every later PR, the record PR
// included, is refused. These diffs are real `git diff` output, built the way lq-compare.yml
// builds lq.diff.
const DIFF_ARGS = ["diff", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", "-M", "-U1"];
const DIFF_PATHS = ["*.rs", "*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs"];
function gitDiff(before, change) {
  return gitRepo(before, change).diff;
}
// The diff and the head tree it ends at (compare's --root in CI).
function gitRepo(before, change) {
  const dir = tree(before);
  const git = (...args) => {
    const r = spawnSync("git", ["-c", "user.name=lq", "-c", "user.email=lq@example.invalid", "-c", "commit.gpgsign=false",
      "-c", "core.hooksPath=/dev/null", "-c", "init.defaultBranch=main", "-C", dir, ...args], { encoding: "utf8" });
    if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
    return r.stdout;
  };
  git("init", "-q");
  git("add", "-A");
  git("commit", "-qm", "base");
  change(dir);
  git("add", "-A");
  git("commit", "-qm", "head");
  return { diff: git(...DIFF_ARGS, "HEAD~1", "HEAD", "--", ...DIFF_PATHS), dir };
}
const RS_TESTS = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn guards_the_bridge() {\n        assert!(true);\n    }\n\n    #[tokio::test]\n    async fn keeps_the_relay() {}\n}\n";
const TS_TESTS = 'import { it } from "vitest";\n\nit("keeps the probe", () => {\n  expect(1).toBe(1);\n});\n\nit("holds the floor", () => {});\n';

describe("compare: removed tests from real git diffs", () => {
  it("a deleted rust test file: every test in it is removed and needs the ADR", () => {
    const diff = gitDiff({ "src-tauri/src/bridge_tests.rs": RS_TESTS, "src-tauri/src/lib.rs": "pub fn a() {}\n" },
      (d) => fs.rmSync(path.join(d, "src-tauri/src/bridge_tests.rs")));
    expect(diff).toMatch(/^\+\+\+ \/dev\/null$/m);
    expect(R.scanDiff(diff).removedTests).toEqual(["guards_the_bridge", "keeps_the_relay"]);
    const r = R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/test removed and not added back: guards_the_bridge/);
    expect(r.failures.join("\n")).toMatch(/test removed and not added back: keeps_the_relay/);
  });

  it("a deleted vitest file: every test in it is removed and needs the ADR", () => {
    const diff = gitDiff({ "src/lib/probe.test.ts": TS_TESTS }, (d) => fs.rmSync(path.join(d, "src/lib/probe.test.ts")));
    expect(R.scanDiff(diff).removedTests).toEqual(["keeps the probe", "holds the floor"]);
    const r = R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/keeps the probe/);
    const adr = tree({ "docs/adr/ADR-probe.md": "Removes keeps the probe and holds the floor (moved to the e2e suite).\n" });
    expect(R.compare(doc(), doc(), { root: adr, diff, floor: doc(), labels: ["ratchet-adr"], body: "docs/adr/ADR-probe.md" }))
      .toMatchObject({ ok: true, failures: [] });
  });

  it("a test file renamed with one test dropped: only the dropped test is a removal", () => {
    const diff = gitDiff({ "src/lib/probe.test.ts": TS_TESTS }, (d) => {
      fs.rmSync(path.join(d, "src/lib/probe.test.ts"));
      fs.mkdirSync(path.join(d, "src/probe"), { recursive: true });
      fs.writeFileSync(path.join(d, "src/probe/probe.test.ts"), TS_TESTS.replace('\nit("holds the floor", () => {});\n', ""));
    });
    expect(diff).toMatch(/^rename from src\/lib\/probe\.test\.ts$/m);
    const s = R.scanDiff(diff);
    expect(s.removedTests).toEqual(["holds the floor"]);
    expect(R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() }).failures.join("\n")).toMatch(/holds the floor/);
  });

  it("a moved rust test file with every test kept is not a removal", () => {
    const diff = gitDiff({ "src-tauri/src/bridge_tests.rs": RS_TESTS }, (d) => {
      fs.rmSync(path.join(d, "src-tauri/src/bridge_tests.rs"));
      fs.mkdirSync(path.join(d, "src-tauri/tests"), { recursive: true });
      fs.writeFileSync(path.join(d, "src-tauri/tests/bridge.rs"), RS_TESTS.replace("assert!(true);", "assert_eq!(1, 1);"));
    });
    expect(R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() })).toMatchObject({ ok: true, failures: [] });
  });

  it("a new test file (from /dev/null) is scanned at its new path for focused and unreasoned skipped tests", () => {
    const diff = gitDiff({ "src/a.ts": "export const a = 1;\n" }, (d) => {
      fs.writeFileSync(path.join(d, "src/b.test.ts"), `${ONLY}("focus", () => {});\n${SKIP}("later", () => {});\n`);
    });
    expect(diff).toMatch(/^--- \/dev\/null$/m);
    const s = R.scanDiff(diff);
    expect(s.focused).toEqual([`src/b.test.ts:1: ${ONLY}("focus", () => {});`]);
    expect(s.unreasoned).toEqual([`src/b.test.ts:2: ${SKIP}("later", () => {});`]);
    expect(s.removedTests).toEqual([]);
  });

  it("a removed line that reads '-- x' is hunk content, not a file header", () => {
    const body = 'const sql = `\n-- drops the index\nselect 1;\n`;\nit("runs the migration", () => {});\n';
    const diff = gitDiff({ "src/m.test.ts": body }, (d) => fs.writeFileSync(path.join(d, "src/m.test.ts"), body.replace("-- drops the index\n", "")));
    expect(diff).toMatch(/^--- drops the index$/m);
    expect(R.scanDiff(diff)).toMatchObject({ removedTests: [], addedTests: [], focused: [], unreasoned: [] });
    // And a test removed after it is still found in the right file.
    const both = gitDiff({ "src/m.test.ts": body }, (d) => fs.writeFileSync(path.join(d, "src/m.test.ts"), "const sql = `\nselect 1;\n`;\n"));
    expect(R.scanDiff(both).removedTests).toEqual(["runs the migration"]);
  });
});

// Review of #790 (round 4), reproduced: deleting only the #[test] line and keeping the fn turns
// the test off without removing its fn line, so the removed-test rule saw nothing, and a filler
// test kept the count level. A test attribute removed (or added) on its own is a removal (or an
// addition) of the fn it sits on. Real `git diff -U1` output, as lq-compare.yml builds lq.diff.
const RS_FILE = "src-tauri/src/bridge_tests.rs";
const editRs = (d, f) => fs.writeFileSync(path.join(d, RS_FILE), f(fs.readFileSync(path.join(d, RS_FILE), "utf8")));
const FILLER = "\n#[test]\nfn filler() {}\n";

describe("compare: a test attribute removed or added on its own", () => {
  it("removing only #[test] from a fn that stays is a removal, even when a filler test keeps the count", () => {
    const diff = gitDiff({ [RS_FILE]: RS_TESTS }, (d) => editRs(d, (t) => t.replace("    #[test]\n    fn guards", "    fn guards") + FILLER));
    expect(diff).toMatch(/^-    #\[test\]$/m);
    expect(diff).toMatch(/^     fn guards_the_bridge\(\) \{$/m);
    const s = R.scanDiff(diff);
    expect(s.removedTests).toEqual(["guards_the_bridge"]);
    expect(s.addedTests).toEqual(["filler"]);
    const r = R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/test removed and not added back: guards_the_bridge/);
    const adr = tree({ "docs/adr/ADR-bridge.md": "Turns off guards_the_bridge; the fleet suite covers it.\n" });
    expect(R.compare(doc(), doc(), { root: adr, diff, floor: doc(), labels: ["ratchet-adr"], body: "docs/adr/ADR-bridge.md" }))
      .toMatchObject({ ok: true, failures: [] });
  });

  it("removing only #[tokio::test] from an async fn that stays is a removal", () => {
    const diff = gitDiff({ [RS_FILE]: RS_TESTS }, (d) => editRs(d, (t) => t.replace("    #[tokio::test]\n", "")));
    expect(diff).toMatch(/^-    #\[tokio::test\]$/m);
    expect(R.scanDiff(diff)).toMatchObject({ removedTests: ["keeps_the_relay"], addedTests: [] });
    expect(R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() }).failures.join("\n"))
      .toMatch(/test removed and not added back: keeps_the_relay/);
  });

  it("a test attribute added to an unchanged fn is an addition", () => {
    const before = "#[cfg(test)]\nmod tests {\n    async fn checks_the_seam() {}\n\n    fn checks_the_relay() {}\n}\n";
    const diff = gitDiff({ [RS_FILE]: before }, (d) => editRs(d, (t) => t.replace("    async fn", "    #[tokio::test]\n    async fn")));
    expect(diff).toMatch(/^\+    #\[tokio::test\]$/m);
    expect(diff).toMatch(/^     async fn checks_the_seam\(\) \{\}$/m);
    expect(R.scanDiff(diff)).toMatchObject({ removedTests: [], addedTests: ["checks_the_seam"] });
    const plain = gitDiff({ [RS_FILE]: before.replace("async fn checks_the_seam", "fn checks_the_seam") },
      (d) => editRs(d, (t) => t.replace("    fn checks_the_seam", "    #[test]\n    fn checks_the_seam")));
    expect(R.scanDiff(plain)).toMatchObject({ removedTests: [], addedTests: ["checks_the_seam"] });
  });

  it("swapping #[test] for #[tokio::test] on an unchanged fn is removed and added back, so it passes", () => {
    const diff = gitDiff({ [RS_FILE]: RS_TESTS }, (d) => editRs(d, (t) => t.replace("    #[test]\n    fn guards", "    #[tokio::test]\n    fn guards")));
    const s = R.scanDiff(diff);
    expect(s.removedTests).toEqual(["guards_the_bridge"]);
    expect(s.addedTests).toEqual(["guards_the_bridge"]);
    expect(R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() })).toMatchObject({ ok: true, failures: [] });
    // Swapping it for #[ignore] turns the test off: a removal.
    const off = gitDiff({ [RS_FILE]: RS_TESTS }, (d) => editRs(d, (t) => t.replace("    #[test]\n    fn guards", "    #[ignore = \"needs anvil, #812\"]\n    fn guards")));
    expect(R.scanDiff(off).removedTests).toEqual(["guards_the_bridge"]);
  });

  it("a removed #[test] whose fn is outside the diff context: named from the head tree, or by file and line without it", () => {
    const before = "#[cfg(test)]\nmod tests {\n    #[test]\n    #[ignore = \"needs anvil, #812\"]\n    #[allow(dead_code)]\n    fn slow_bridge() {}\n}\n";
    const { diff, dir } = gitRepo({ [RS_FILE]: before }, (d) => editRs(d, (t) => t.replace("    #[test]\n", "")));
    expect(diff).not.toMatch(/fn slow_bridge/);
    expect(R.scanDiff(diff, { root: dir }).removedTests).toEqual(["slow_bridge"]);
    expect(R.compare(doc(), doc(), { root: dir, diff, floor: doc() }).failures.join("\n"))
      .toMatch(/test removed and not added back: slow_bridge/);
    // No head tree to read: still refused (fail closed), named by the old file and line.
    expect(R.scanDiff(diff).removedTests).toEqual([`${RS_FILE}:3`]);
    expect(R.compare(doc(), doc(), { root: tree({}), diff, floor: doc() }).ok).toBe(false);
  });
});

describe("compare: a committed floor above the measured head", () => {
  it("a head that raises ratchet.json above its own measured count is refused, label or not", () => {
    const at = doc({ "tests.rust.passed": 100 });
    const r = R.compare(at, at, { root: tree({}), floor: doc({ "tests.rust.passed": 999999 }) });
    expect(r.ok).toBe(false);
    expect(r.failures.join("\n")).toMatch(/tests\.rust\.passed: \.agentile\/coverage\/ratchet\.json records 999999, above the 100 this head measures/);
    const adr = tree({ "docs/adr/ADR-raise.md": "tests.rust.passed\n" });
    expect(R.compare(at, at, { root: adr, floor: doc({ "tests.rust.passed": 999999 }), labels: ["ratchet-adr"], body: "docs/adr/ADR-raise.md" }).ok)
      .toBe(false);
  });

  it("every axis: a floor above the head fails, a floor at or below it passes", () => {
    for (const k of R.AXES) {
      const head = doc();
      const v = k.split(".").reduce((o, s) => o[s], head);
      expect(R.compare(doc(), head, { root: tree({}), floor: doc({ [k]: v + 1 }) }).ok, k).toBe(false);
      expect(R.compare(doc(), head, { root: tree({}), floor: doc({ [k]: v }) }).ok, k).toBe(true);
    }
    // Frontmatter: a floor that claims fewer docs without frontmatter than the head has.
    const fm = R.compare(doc({ "frontmatter.docs": 262 }), doc({ "frontmatter.docs": 262 }), { root: tree({}), floor: doc() });
    expect(fm.ok).toBe(false);
    expect(fm.failures.join("\n")).toMatch(/frontmatter: .*records 0 docs without frontmatter, below the 1 this head measures/);
    // With no baseline at the merge base yet, a floor above the head is still refused.
    expect(R.compare(null, doc(), { root: tree({}), floor: doc({ "tests.vitest.passed": 5201 }) }).ok).toBe(false);
    // A head that grew past its floor passes (the record PR raises the floor later).
    expect(R.compare(doc(), doc({ "tests.rust.passed": 3300 }), { root: tree({}), floor: doc() })).toMatchObject({ ok: true, failures: [] });
  });

  it("CLI: compare refuses a committed floor above the measured head", () => {
    const d = tree({ "base.json": JSON.stringify(doc()), "head.json": JSON.stringify(doc()),
      ".agentile/coverage/ratchet.json": JSON.stringify(doc({ "tests.vitest.passed": 999999 })) });
    const res = spawnSync(process.execPath, [SCRIPT, "compare", "--base", path.join(d, "base.json"), "--head", path.join(d, "head.json"), "--root", d], { encoding: "utf8" });
    expect(res.status).toBe(1);
    expect(res.stdout).toMatch(/tests\.vitest\.passed: .*records 999999, above the 5200 this head measures/);
  });
});

describe("CLI", () => {
  const run = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
  it("compare reads the head's floor from --root and refuses when it is missing or lowered", () => {
    const d = tree({ "base.json": JSON.stringify(doc()), "same.json": JSON.stringify(doc()) });
    const p = (f) => path.join(d, f);
    const args = ["compare", "--base", p("base.json"), "--head", p("same.json"), "--root", d];
    const missing = run(args);
    expect(missing.status).toBe(1);
    expect(missing.stdout).toMatch(/ratchet\.json is missing or unreadable in the head/);
    write(d, ".agentile/coverage/ratchet.json", JSON.stringify(doc({ "tests.rust.passed": 3000 })));
    const lowered = run(args);
    expect(lowered.status).toBe(1);
    expect(lowered.stdout).toMatch(/ratchet\.json lowers tests\.rust\.passed/);
    write(d, ".agentile/coverage/ratchet.json", JSON.stringify(doc()));
    expect(run(args).status).toBe(0);
  });

  it("compare exits 0 on pass, 1 on refusal and 2 on usage", () => {
    const d = tree({ "base.json": JSON.stringify(doc()), "head.json": JSON.stringify(doc({ "tests.rust.passed": 3217 })), "same.json": JSON.stringify(doc()),
      ".agentile/coverage/ratchet.json": JSON.stringify(doc()) });
    const p = (f) => path.join(d, f);
    expect(run(["compare", "--base", p("base.json"), "--head", p("same.json"), "--root", d]).status).toBe(0);
    const refused = run(["compare", "--base", p("base.json"), "--head", p("head.json"), "--root", d]);
    expect(refused.status).toBe(1);
    expect(refused.stdout + refused.stderr).toMatch(/tests\.rust\.passed decreased/);
    expect(run(["compare", "--head", p("head.json")]).status).toBe(2);
    expect(run(["frobnicate"]).status).toBe(2);
    expect(run(["compare", "--base", "none", "--head", p("same.json"), "--root", d]).status).toBe(0);
  });
});

// ci.yml is L0's file (lane-guard, .agentile/v60-ownership.map), so `lq / ratchet` is wired from
// LQ's own lq-ratchet.yml: it calls lq-compare.yml as job `lq` after its two measure jobs.
describe("workflow wiring (tripwires)", () => {
  const ci = fs.readFileSync(path.join(repo, ".github/workflows/ci.yml"), "utf8");
  const lr = fs.readFileSync(path.join(repo, ".github/workflows/lq-ratchet.yml"), "utf8");
  const wf = fs.readFileSync(path.join(repo, ".github/workflows/lq-compare.yml"), "utf8");
  it("lq-ratchet.yml calls lq-compare as job `lq` after both measure jobs, even when they fail, on every PR, merge group and push to main", () => {
    const job = lr.slice(lr.indexOf("\n  lq:"));
    expect(job).toMatch(/uses: \.\/\.github\/workflows\/lq-compare\.yml/);
    expect(job).toMatch(/needs: \[cargo-test, vitest-json\]/);
    expect(job).toMatch(/if: \$\{\{ !cancelled\(\) \}\}/);
    expect(lr).toMatch(/cargo test --workspace --locked --offline --no-fail-fast 2>&1 \| tee "\$RUNNER_TEMP\/cargo-test\.log"/);
    expect(lr).toMatch(/--reporter=json --outputFile\.json=/);
    // A required check with a path filter does not run on most PRs, and a check that never ran
    // cannot block one. Every trigger the merge path uses is there, with no filter.
    const on = lr.slice(lr.indexOf("\non:"), lr.indexOf("\npermissions:"));
    expect(on).toMatch(/\n  pull_request:\n/);
    expect(on).toMatch(/\n  merge_group:\n\s+types: \[checks_requested\]/);
    expect(on).toMatch(/\n  push:\n\s+branches: \[main\]/);
    expect(on).not.toMatch(/paths(-ignore)?:/);
    // The counts are of the event commit (the merge commit on a PR), never the PR head alone: a
    // head that predates tests the base gained would read as a decrease.
    expect(lr).not.toMatch(/pull_request\.head\.sha/);
    // LQ does not edit L0's ci.yml.
    expect(ci).not.toMatch(/lq-compare\.yml/);
  });
  it("the ratchet job is named ratchet (check `lq / ratchet`), reads only, and never puts PR text into a script", () => {
    const job = wf.slice(wf.indexOf("\n  ratchet:"), wf.indexOf("\n  record:"));
    expect(job).toMatch(/name: ratchet\n/);
    expect(job).toMatch(/permissions:\n\s+contents: read\n\s+pull-requests: read/);
    expect(wf).not.toMatch(/\$\{\{[^}]*(body|title|head_ref|labels)[^}]*\}\}/);
    expect(job).toMatch(/fails closed/);
    // The merge base's script judges once it exists there, so a PR's edit to the rules applies from
    // the next PR on. PR code still runs in the measuring steps; review is the control there.
    expect(job).toMatch(/git show "\$base:scripts\/ci\/lq\/ratchet\.mjs" > "\$RUNNER_TEMP\/ratchet\.mjs"/);
    expect(job).toMatch(/node "\$LQ_SCRIPT" compare/);
  });
  it("the merge base's script judges only once it has compare (LQ-WP01's measure-only script cannot refuse every PR)", () => {
    const job = wf.slice(wf.indexOf("\n  ratchet:"), wf.indexOf("\n  record:"));
    expect(job).toMatch(/git show "\$base:scripts\/ci\/lq\/ratchet\.mjs" 2>\/dev\/null \| grep -q '\^export function compare\('/);
    expect(fs.readFileSync(SCRIPT, "utf8")).toMatch(/^export function compare\(/m);
  });
  it("merge_group and push runs find the PRs by the queue branch and by squash or merge-commit subjects", () => {
    const job = wf.slice(wf.indexOf("\n  ratchet:"), wf.indexOf("\n  record:"));
    // The queue branch GitHub builds for a group: gh-readonly-queue/<base>/pr-<N>-<sha>, read from
    // the runner's GITHUB_REF, never from an expression.
    expect(job).toMatch(/"\$GITHUB_REF" \| sed -nE 's#\^refs\/heads\/gh-readonly-queue\/\.\+\/pr-\(\[0-9\]\+\)-\[0-9a-f\]\+\$#\\1#p'/);
    expect(job).toMatch(/s\/\.\*\\\(#\(\[0-9\]\+\)\\\)\$\/\\1\/p/);
    expect(job).toMatch(/s\/\^Merge pull request #\(\[0-9\]\+\) \.\*\/\\1\/p/);
  });
  // Review of #790 (round 3): the raw test log never leaves the cargo job, and the chain read token
  // is scoped to one `cargo fetch --locked` step as an in-process git config. A grep for the
  // literal token cannot catch an encoded form of it.
  const rustJob = () => workflowJobs(lr).find((j) => j.name === "cargo-test");
  it("lq-ratchet.yml cargo job: the chain read token reaches only the cargo fetch step, never the tests or a git config file", () => {
    const rust = rustJob();
    expect(rust).toBeTruthy();
    // No job-wide secret: a job env reaches every step, the PR's build scripts and tests included.
    expect(rust.head).not.toMatch(/secrets\.|CHAIN_TOKEN/);
    const withToken = rust.steps.filter((s) => /secrets\.CITRATE_CHAIN_READ_TOKEN|CHAIN_TOKEN/.test(s));
    expect(withToken.length).toBe(1);
    expect(withToken[0]).toMatch(/cargo fetch --locked/);
    expect(withToken[0]).toMatch(/GIT_CONFIG_KEY_0=/);
    expect(withToken[0]).not.toMatch(/git config|cargo (test|build|run|check|clippy)\b/);
    expect(lr).not.toMatch(/git config --global[^\n]*\n?[^\n]*insteadOf/);
    const names = rust.steps.map((s) => /- name: (.+)/.exec(s)?.[1] ?? s.split("\n")[0].trim());
    const fetch = names.findIndex((n) => /cargo fetch --locked/.test(n));
    const check = names.findIndex((n) => /No credential left on disk/.test(n));
    const firstBuild = rust.steps.findIndex((s) => /cargo (clippy|test|build)\b/.test(s));
    expect(fetch).toBeGreaterThanOrEqual(0);
    expect(check).toBeGreaterThan(fetch);
    expect(firstBuild).toBeGreaterThan(check);
    expect(rust.steps[check]).toMatch(/grep -rIlq 'x-access-token' "\$HOME\/\.gitconfig" "\$cargo_home\/git"/);
    // Every build after the fetch is offline: nothing later can need, or reach for, the token.
    for (const s of rust.steps.filter((x) => /cargo (clippy|test)\b/.test(x))) expect(s).toMatch(/--offline/);
  });
  it("lq-ratchet.yml cargo job: only validated counts leave the job, from cargo's own exit status; the raw log is never an artifact", () => {
    const rust = rustJob();
    const test = rust.steps.find((s) => /cargo test --workspace/.test(s));
    expect(test).toMatch(/code=\$\{PIPESTATUS\[0\]\}/);
    expect(test).toMatch(/ratchet\.mjs cargo-counts --cargo-log "\$RUNNER_TEMP\/cargo-test\.log"[\s\\]*--cargo-exit "\$code" --out "\$RUNNER_TEMP\/cargo-counts\.json"/);
    expect(test).toMatch(/exit "\$code"/);
    const uploads = workflowJobs(lr).flatMap((j) => j.steps).filter((s) => s.includes("actions/upload-artifact"));
    for (const s of uploads) expect(s).not.toMatch(/cargo-test\.log/);
    expect(uploads.some((s) => /name: cargo-counts\n/.test(s) && s.includes("cargo-counts.json"))).toBe(true);
  });
  it("lq / ratchet measures from the counts file and refuses when a measure job did not succeed", () => {
    const job = wf.slice(wf.indexOf("\n  ratchet:"), wf.indexOf("\n  record:"));
    expect(job).toMatch(/name: cargo-counts\n/);
    expect(job).toMatch(/name: vitest-json\n/);
    expect(job).toMatch(/--cargo-counts lq-artifacts\/cargo-counts\.json/);
    expect(job).not.toMatch(/--cargo-log/);
    const lq = lr.slice(lr.indexOf("\n  lq:"));
    expect(lq).toMatch(/rust_result: \$\{\{ needs\.cargo-test\.result \}\}/);
    expect(lq).toMatch(/node_result: \$\{\{ needs\.vitest-json\.result \}\}/);
    expect(wf).toMatch(/workflow_call:\n\s+inputs:\n\s+rust_result:/);
    expect(job).toMatch(/\[ "\$RUST_RESULT" = success \] && \[ "\$NODE_RESULT" = success \]/);
  });
  it("the diff the ratchet reads is the one these tests build: a/ and b/ prefixes, renames found", () => {
    const job = wf.slice(wf.indexOf("\n  ratchet:"), wf.indexOf("\n  record:"));
    expect(job).toContain(`git ${DIFF_ARGS.join(" ")} "$base" HEAD -- ${DIFF_PATHS.map((p) => `'${p}'`).join(" ")} > lq.diff`);
  });
  it("the record job runs only on a push to main and opens a PR rather than pushing to main", () => {
    const job = wf.slice(wf.indexOf("\n  record:"));
    expect(job).toMatch(/if: \$\{\{ github\.event_name == 'push' && github\.ref == 'refs\/heads\/main' \}\}/);
    expect(job).not.toMatch(/git push[^\n]*\bmain\b/);
    expect(job).toMatch(/gh pr create/);
  });
});
