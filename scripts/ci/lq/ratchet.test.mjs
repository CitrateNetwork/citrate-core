// @vitest-environment node
//
// scripts/ci/lq/ratchet.mjs (LQ-WP01): the coverage baseline is the test runners' own count.
// `measure` reads a `cargo test --workspace --locked` log and a `vitest run --reporter=json`
// file from a Linux CI run, plus the tree (formal specs, Rule-5 frontmatter, CI gate steps), and
// writes .agentile/coverage/ratchet.json (schema v1, lanes/LQ_NON_REGRESSION_GUARD.md section
// 3.1). A run with a failed test is never a baseline.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseCargoLog, parseVitestJson, measureTree, buildRatchet } from "./ratchet.mjs";

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

  // lq-ratchet.yml, split into jobs and steps by indentation (no YAML dependency).
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

  it("the chain read token reaches only the cargo fetch step, never the tests or an artifact", () => {
    const wf = fs.readFileSync(path.join(ROOT, ".github/workflows/lq-ratchet.yml"), "utf8");
    const jobs = workflowJobs(wf);
    expect(jobs.map((j) => j.name)).toEqual(["cargo-test", "vitest-json", "measure"]);
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
