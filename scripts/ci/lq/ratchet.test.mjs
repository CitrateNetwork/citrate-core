// @vitest-environment node
//
// LQ-WP02 (0.6.0 planset, lane LQ): the test-count ratchet behind the `lq / ratchet` check.
// scripts/ci/lq/ratchet.mjs measures a head (the rust and node jobs' own runner output plus tree
// counts) and compares it with the merge base's .agentile/coverage/ratchet.json. These tests are
// the revert check for each rule: every rule here fails on the defect it guards.
//
// Fixture strings that look like focused or skipped tests are built by concatenation, so this file
// does not itself trip the tree and diff scans it tests.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as R from "./ratchet.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..", "..", "..");
const SCRIPT = path.join(here, "ratchet.mjs");
const ONLY = "it" + ".only";
const SKIP = "it" + ".skip";
const IGNORE = "#[" + "ignore";

const tmp = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});
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

describe("measure", () => {
  it("parses cargo \"test result\" lines across crates and counts ignored separately", () => {
    const log = [
      "running 3 tests", "test a ... ok",
      "test result: ok. 3000 passed; 0 failed; 35 ignored; 0 measured; 0 filtered out; finished in 9.1s",
      "test result: ok. 218 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 1.0s",
    ].join("\n");
    expect(R.parseCargoLog(log)).toEqual({ passed: 3218, ignored: 38, failed: 0, binaries: 2 });
  });

  it("parses the vitest json reporter totals, counting todo as skipped", () => {
    const v = { numPassedTests: 5200, numPendingTests: 7, numTodoTests: 2, numFailedTests: 0, numFailedTestSuites: 0 };
    expect(R.parseVitestJson(v)).toEqual({ passed: 5200, skipped: 9, failed: 0 });
  });

  it("refuses a log with a failed test, or a suite that failed to load, as a measurement", () => {
    const root = tree({ "src-tauri/formal/A.tla": "" });
    const cargo = "test result: FAILED. 3217 passed; 1 failed; 38 ignored; 0 measured; 0 filtered out";
    const vit = { numPassedTests: 1, numPendingTests: 0, numTodoTests: 0, numFailedTests: 0, numFailedTestSuites: 0 };
    expect(() => R.measure({ root, cargoLog: cargo, vitestJson: vit })).toThrow(/failed/);
    const ok = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
    expect(() => R.measure({ root, cargoLog: ok, vitestJson: { ...vit, numFailedTestSuites: 1 } })).toThrow(/failed/);
    expect(() => R.measure({ root, cargoLog: "no summary here", vitestJson: vit })).toThrow(/no "test result"/);
  });

  it("counts specs, EXPECT configs, registered tripwires, CI steps and frontmatter from the tree", () => {
    const root = tree({
      "src-tauri/formal/A.tla": "", "src-tauri/formal/B.tla": "",
      "src-tauri/formal/A_ok.cfg": "\\* EXPECT: ok\n", "src-tauri/formal/B.cfg": "SPECIFICATION Spec\n",
      ".agentile/coverage/tripwires.json": JSON.stringify({ version: 1, tripwires: [{ id: "a" }, { id: "b" }] }),
      ".github/workflows/ci.yml": "jobs:\n  a:\n    steps:\n      - uses: x\n      - name: y\n        run: z\n      - run: w\n",
      "docs/a.md": "---\ncreated: x\n---\nbody\n", "docs/b.md": "no frontmatter\n", ".agentile/c.md": "---\na: b\n---\n",
    });
    const d = R.measure({
      root, rev: "c".repeat(40), now: "2026-11-26T00:00:00Z",
      cargoLog: "test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out",
      vitestJson: { numPassedTests: 4, numPendingTests: 0, numTodoTests: 0, numFailedTests: 0, numFailedTestSuites: 0 },
    });
    expect(d).toEqual({
      version: 1, rev: "c".repeat(40), measured: "2026-11-26T00:00:00Z",
      tests: { rust: { passed: 5, ignored: 1 }, vitest: { passed: 4, skipped: 0 } },
      specs: { tla: 2, tlc_configs_in_ci: 1 },
      tripwires: { registered: 2, ci_steps: 3 },
      frontmatter: { docs: 3, with: 2 },
    });
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

describe("CLI", () => {
  const run = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
  it("compare exits 0 on pass, 1 on refusal and 2 on usage", () => {
    const d = tree({ "base.json": JSON.stringify(doc()), "head.json": JSON.stringify(doc({ "tests.rust.passed": 3217 })), "same.json": JSON.stringify(doc()) });
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

describe("workflow wiring (tripwires)", () => {
  const ci = fs.readFileSync(path.join(repo, ".github/workflows/ci.yml"), "utf8");
  const wf = fs.readFileSync(path.join(repo, ".github/workflows/lq-ratchet.yml"), "utf8");
  it("ci.yml calls lq-ratchet as job `lq` after rust and node, even when they fail, so a red test job cannot skip the check", () => {
    const job = ci.slice(ci.indexOf("\n  lq:"));
    expect(job).toMatch(/uses: \.\/\.github\/workflows\/lq-ratchet\.yml/);
    expect(job).toMatch(/needs: \[rust, node\]/);
    expect(job).toMatch(/if: \$\{\{ !cancelled\(\) \}\}/);
    expect(ci).toMatch(/cargo test --workspace --locked --no-fail-fast 2>&1 \| tee cargo-test\.log/);
    expect(ci).toMatch(/--outputFile\.json=vitest\.json/);
  });
  it("the ratchet job is named ratchet (check `lq / ratchet`), reads only, and never puts PR text into a script", () => {
    const job = wf.slice(wf.indexOf("\n  ratchet:"), wf.indexOf("\n  record:"));
    expect(job).toMatch(/name: ratchet\n/);
    expect(job).toMatch(/permissions:\n\s+contents: read\n\s+pull-requests: read/);
    expect(wf).not.toMatch(/\$\{\{[^}]*(body|title|head_ref|labels)[^}]*\}\}/);
    expect(job).toMatch(/fails closed/);
    // A PR cannot loosen the rules that judge it: the merge base's script runs once it exists there.
    expect(job).toMatch(/git show "\$base:scripts\/ci\/lq\/ratchet\.mjs" > "\$RUNNER_TEMP\/ratchet\.mjs"/);
    expect(job).toMatch(/node "\$LQ_SCRIPT" compare/);
  });
  it("the record job runs only on a push to main and opens a PR rather than pushing to main", () => {
    const job = wf.slice(wf.indexOf("\n  record:"));
    expect(job).toMatch(/if: \$\{\{ github\.event_name == 'push' && github\.ref == 'refs\/heads\/main' \}\}/);
    expect(job).not.toMatch(/git push[^\n]*\bmain\b/);
    expect(job).toMatch(/gh pr create/);
  });
});
