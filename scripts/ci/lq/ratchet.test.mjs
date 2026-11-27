// @vitest-environment node
//
// LQ-WP01 + LQ-WP02 (0.6.0 planset, lane LQ): the test-count ratchet behind the `lq / ratchet`
// check. scripts/ci/lq/ratchet.mjs measures a head (the rust and node jobs' own runner output plus
// tree counts, by the rules LQ-WP01 measured the committed baseline with) and compares it with the
// merge base's .agentile/coverage/ratchet.json. These tests are the revert check for each rule:
// every rule here fails on the defect it guards.
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

const VIT = (over = {}) => ({
  numPassedTests: 1, numPendingTests: 0, numTodoTests: 0, numFailedTests: 0, numFailedTestSuites: 0, success: true,
  testResults: [], ...over,
});
const FULL_FM = "---\ncreated: 2026-11-26\nbranch: main\nauthor: Larry Klosowski\nstatus: active\n---\n# x\n";

describe("measure (LQ-WP02 head, LQ-WP01 rules)", () => {
  it("parses cargo \"test result\" lines across crates and counts ignored separately", () => {
    const log = [
      "running 3 tests", "test a ... ok",
      "test result: ok. 3000 passed; 0 failed; 35 ignored; 0 measured; 0 filtered out; finished in 9.1s",
      "test result: ok. 218 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 1.0s",
    ].join("\n");
    expect(R.parseCargoLog(log)).toMatchObject({ passed: 3218, ignored: 38, failed: 0, suites: 2 });
  });

  it("parses the vitest json reporter totals, counting todo as skipped", () => {
    const v = VIT({ numPassedTests: 5200, numPendingTests: 7, numTodoTests: 2 });
    expect(R.parseVitestJson(v)).toMatchObject({ passed: 5200, skipped: 9, failed: 0 });
  });

  it("refuses a log with a failed test, or a suite that failed to load, as a measurement", () => {
    const root = tree({ "src-tauri/formal/A.tla": "" });
    const cargo = "test result: FAILED. 3217 passed; 1 failed; 38 ignored; 0 measured; 0 filtered out";
    expect(() => R.measure({ root, cargoLog: cargo, vitestJson: VIT() })).toThrow(/failed/);
    const ok = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
    expect(() => R.measure({ root, cargoLog: ok, vitestJson: VIT({ numFailedTestSuites: 1, success: false }) })).toThrow(/failed/);
    expect(() => R.measure({ root, cargoLog: "no summary here", vitestJson: VIT() })).toThrow(/no "test result"/);
  });

  it("counts specs, EXPECT configs, CI gate steps and Rule-5 frontmatter from the tree", () => {
    const root = tree({
      "src-tauri/formal/A.tla": "", "src-tauri/formal/B.tla": "",
      "src-tauri/formal/A_ok.cfg": "\\* EXPECT: ok\n", "src-tauri/formal/B.cfg": "SPECIFICATION Spec\n",
      // One gate step: a named step that runs a command. `uses:`, unnamed and setup steps are not gates.
      ".github/workflows/ci.yml": "jobs:\n  a:\n    steps:\n      - uses: x\n      - name: y\n        run: z\n      - run: w\n      - name: Install deps\n        run: apt\n",
      "docs/a.md": FULL_FM, "docs/b.md": "no frontmatter\n", ".agentile/c.md": "---\ncreated: x\n---\nonly one Rule-5 key\n",
    });
    const d = R.measure({
      root, rev: "c".repeat(40), now: "2026-11-26T00:00:00Z",
      cargoLog: "test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out",
      vitestJson: VIT({ numPassedTests: 4 }),
    });
    expect(d).toEqual({
      version: 1, rev: "c".repeat(40), measured: "2026-11-26T00:00:00Z",
      tests: { rust: { passed: 5, ignored: 1 }, vitest: { passed: 4, skipped: 0 } },
      specs: { tla: 2, tlc_configs_in_ci: 1 },
      tripwires: { registered: 0, ci_steps: 1 },
      frontmatter: { docs: 3, with: 1 },
    });
  });

  it("registered tripwires: the runners' passed tripwire tests until the LQ-WP03 registry exists, then its live entries", () => {
    const cargoLog = ["test fleet_tests::rule3_tripwire_no_silent_peer ... ok", "test x::tripwire_ignored ... ignored",
      "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"].join("\n");
    const vitestJson = VIT({ testResults: [{ assertionResults: [
      { fullName: "bridge Tripwire: no token crosses", status: "passed" }, { fullName: "tripwire later", status: "skipped" }] }] });
    expect(R.measure({ root: tree({}), cargoLog, vitestJson }).tripwires.registered).toBe(2);
    const withRegistry = tree({ ".agentile/coverage/tripwires.json": JSON.stringify({ version: 1, tripwires: [
      { id: "a", replaced_by: null }, { id: "b", replaced_by: null }, { id: "c", replaced_by: null }, { id: "old", replaced_by: "c" }] }) });
    expect(R.measure({ root: withRegistry, cargoLog, vitestJson }).tripwires.registered).toBe(3);
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
  it("the uploaded cargo test log can never carry the private-repo credential", () => {
    const rust = ci.slice(ci.indexOf("\n  rust:"), ci.indexOf("\n  node:"));
    const guard = rust.indexOf("name: refuse to keep a test log that carries a credential");
    const upload = rust.indexOf("name: keep the test log for lq / ratchet");
    expect(guard).toBeGreaterThan(0);
    expect(guard).toBeLessThan(upload);
    const step = rust.slice(guard, upload);
    expect(step).toMatch(/grep -q 'x-access-token' cargo-test\.log/);
    // The token value is matched from a file descriptor, never put on a command line.
    expect(step).toMatch(/grep -qF -f <\(printf '%s\\n' "\$CHAIN_TOKEN"\) cargo-test\.log/);
    expect(step).toMatch(/rm -f cargo-test\.log/);
    expect(step).toMatch(/exit 1/);
  });
  it("the record job runs only on a push to main and opens a PR rather than pushing to main", () => {
    const job = wf.slice(wf.indexOf("\n  record:"));
    expect(job).toMatch(/if: \$\{\{ github\.event_name == 'push' && github\.ref == 'refs\/heads\/main' \}\}/);
    expect(job).not.toMatch(/git push[^\n]*\bmain\b/);
    expect(job).toMatch(/gh pr create/);
  });
});

// LQ-WP01's measure tests, kept as they landed on feat/v60-lq-wp01-baseline: the head is measured
// by the same rules as the committed baseline.
function tmpdir1() {
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
    const r = R.parseCargoLog(CARGO_OK);
    expect(r.suites).toBe(4);
    expect(r.passed).toBe(28 + 3228 + 706 + 0);
    expect(r.failed).toBe(0);
    // Test names come from the runner's own per-test lines, prefix columns stripped.
    expect(r.tripwireTests).toEqual(["fleet_tests::rule3_tripwire_no_silent_peer"]);
  });

  it("parses the vitest json reporter totals", () => {
    const r = R.parseVitestJson(VITEST_OK);
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
    expect(R.parseCargoLog(failedCargo).failed).toBe(1);
    expect(() => R.buildRatchet({ cargo: R.parseCargoLog(failedCargo), vitest: R.parseVitestJson(VITEST_OK) })).toThrow(
      /failed/,
    );
    const failedVitest = { ...VITEST_OK, numFailedTests: 1, success: false };
    expect(() => R.buildRatchet({ cargo: R.parseCargoLog(CARGO_OK), vitest: R.parseVitestJson(failedVitest) })).toThrow(
      /failed/,
    );
    // An empty or truncated log is not a count of zero.
    expect(() => R.buildRatchet({ cargo: R.parseCargoLog("Compiling citrate v0.5.9\n"), vitest: R.parseVitestJson(VITEST_OK) })).toThrow(
      /no "test result" line/,
    );

    // The CLI refuses too: exit 1, and no ratchet.json is written.
    const dir = tmpdir1();
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
    const r = R.parseCargoLog(CARGO_OK);
    // 1 + 22 + 1 ignored, never folded into passed.
    expect(r.ignored).toBe(24);
    expect(r.passed).toBe(3962);
    const withIgnoredOnly = R.parseCargoLog(
      "test result: ok. 0 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.00s",
    );
    expect(withIgnoredOnly).toMatchObject({ passed: 0, ignored: 5, failed: 0 });
  });

  it("measures specs, Rule-5 frontmatter and CI gate steps from the tree, and writes schema v1", () => {
    const dir = tmpdir1();
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

    const t = R.measureTree(dir);
    expect(t.specs).toEqual({ tla: 2, tlc_configs_in_ci: 2 });
    expect(t.frontmatter.docs).toBe(4);
    expect(t.frontmatter.with).toBe(2);
    expect(t.frontmatter.missing.sort()).toEqual([".agentile/c.md", ".agentile/d.md"]);
    // fmt, test, typecheck (ci.yml) + wait (merge-gate) + eval check (eval-check); setup steps
    // (authenticate, install, fetch, upload) and unnamed steps are not gates.
    expect(t.ci_steps).toBe(5);

    const r = R.buildRatchet({
      cargo: R.parseCargoLog(CARGO_OK),
      vitest: R.parseVitestJson(VITEST_OK),
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
