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
      [SCRIPT, "measure", "--cargo-log", log, "--vitest-json", vj, "--root", dir, "--rev", "a".repeat(40), "--out", out],
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
    write(dir, "src-tauri/formal/A.cfg", "\\* EXPECT: ok\nINIT Init\n");
    write(dir, "src-tauri/formal/A_bug.cfg", "\\* EXPECT: violation Safety\nINIT Init\n");
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
    expect(t.specs).toEqual({ tla: 2, tlc_configs_in_ci: 2 });
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
      specs: { tla: 2, tlc_configs_in_ci: 2 },
      tripwires: { registered: 3, ci_steps: 5 },
      frontmatter: { docs: 4, with: 2 },
    });
  });
});
