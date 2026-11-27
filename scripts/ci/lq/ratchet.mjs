#!/usr/bin/env node
// created: 2026-11-26 | branch: feat/v60-lq-wp01-baseline, feat/v60-lq-wp02-ratchet-job
// author: Larry Klosowski (LQ lane, 0.6.0)
// status: active
//
// LQ-WP01: the coverage ratchet's numbers are the test runners' own counts, never a grep.
// LQ-WP02: `compare` is the check `lq / ratchet` (Rule 2): a head is measured by the same rules
// and compared with the merge base's .agentile/coverage/ratchet.json.
//
//   node scripts/ci/lq/ratchet.mjs cargo-counts --cargo-log <file> --cargo-exit <code> --out <file>
//   node scripts/ci/lq/ratchet.mjs measure (--cargo-counts <file> | --cargo-log <file> --cargo-exit <code>)
//        --vitest-json <file> [--root <repo>] [--rev <sha>] [--out <ratchet.json>]
//   node scripts/ci/lq/ratchet.mjs compare --base <ratchet.json|none> --head <measured.json>
//        [--root <repo>] [--diff <unified diff>] [--labels a,b] [--pr-body <file>]
//
// `cargo-counts` runs in the job that ran the tests and turns its log plus cargo's own exit status
// into validated counts; that small file, not the raw test output, is what leaves the job. A raw
// log is only ever measured next to the exit status of the cargo run that wrote it: a log alone
// cannot show that a later test binary did not die before printing its summary.
//
// Data sources (Rule 7):
//   rust    the `test result:` summary lines of `cargo test --workspace --locked` (libtest), one
//           per test binary and doc-test target; passed and ignored are summed separately. Every
//           target cargo starts (`Running ...`, `Doc-tests ...`) must have its summary line, and
//           cargo must have exited 0.
//   vitest  the json reporter file of `vitest run --reporter=json` (numPassedTests; skipped is
//           numPendingTests + numTodoTests).
//   tree    src-tauri/formal/*.tla; the configs TLC runs in CI, from the tree's own runner
//           (src-tauri/formal/tlc_expect.py discover_specs + plan, as `--all` in
//           tla-models.yml calls them; 0 when either is absent or the plan has an error); Markdown
//           under docs/ and .agentile/ with all four Rule-5 keys (created, branch, author, status)
//           in a leading frontmatter block; the named `run:` steps of the required gate workflows
//           (ci.yml, merge-gate.yml, eval-check.yml), setup steps excepted.
//   tripwires  the live (not replaced) entries of the LQ-WP03 registry
//           (.agentile/coverage/tripwires.json) once it exists; until then the number of tests the
//           runners executed whose full name contains "tripwire" (rust per-test lines plus vitest
//           assertion full names).
//   compare the merge base's ratchet.json, the head's committed ratchet.json (the floor), the PR
//           diff, and the PR's labels and body.
//
// Exit codes: 0 written or passed, 1 refused (a failed test, a non-zero cargo exit, a target with
// no summary line, a bad reporter or counts file, a ratchet rule), 2 usage. A refused measure
// writes nothing: a red run is never a baseline.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SUMMARY =
  /test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out/;
// cargo's per-target headers: `     Running unittests src/lib.rs (target/debug/deps/x-hash)`,
// `     Running tests/foo.rs (target/...)` and `   Doc-tests crate_name`. Each one is followed by
// exactly one libtest summary unless the binary died first.
const HEADER = /(?:^|\s)(?:Running (?:unittests )?\S+ \([^)]*\)|Doc-tests [\w-]+)\s*$/;
// libtest per-test line: `test <name> ... ok|ignored[, reason]|FAILED`. GitHub log exports put
// job, step and timestamp columns in front of it; anything before " test " is ignored.
const PER_TEST = /(?:^|\s)test (\S+) \.\.\. (ok|ignored|FAILED)\b/;
const TRIPWIRE = /tripwire/i;
// SGR color sequences (CI sets CARGO_TERM_COLOR=always).
const ANSI = /\u001b\[[0-9;]*m/g;

export function parseCargoLog(text) {
  const out = { suites: 0, headers: 0, passed: 0, failed: 0, ignored: 0, tripwireTests: [] };
  for (const raw of String(text).split(/\r?\n/)) {
    const line = raw.replace(ANSI, "");
    const s = SUMMARY.exec(line);
    if (s) {
      out.suites += 1;
      out.passed += Number(s[2]);
      out.failed += Number(s[3]);
      out.ignored += Number(s[4]);
      if (s[1] === "FAILED" && Number(s[3]) === 0) out.failed += 1;
      continue;
    }
    if (HEADER.test(line)) {
      out.headers += 1;
      continue;
    }
    const t = PER_TEST.exec(line);
    if (t && t[2] !== "ignored" && TRIPWIRE.test(t[1])) out.tripwireTests.push(t[1]);
  }
  return out;
}

// The counts a red, partial or truncated cargo run can never produce.
export function checkCargo(cargo) {
  if (cargo.suites === 0) throw new Error('cargo log: no "test result" line (empty or truncated log)');
  if (cargo.failed > 0) throw new Error(`cargo log: ${cargo.failed} failed test(s); a red run is not a baseline`);
  if (cargo.exit !== undefined && cargo.exit !== 0) {
    throw new Error(`cargo test exited ${cargo.exit}; a red run is not a baseline`);
  }
  if (cargo.headers !== cargo.suites) {
    throw new Error(
      `cargo log: ${cargo.headers} test targets started, ${cargo.suites} summary line${cargo.suites === 1 ? "" : "s"} ` +
        "(a test binary aborted or the log is truncated)",
    );
  }
  return cargo;
}

function exitStatus(v) {
  if (!/^\d+$/.test(String(v))) throw new Error(`--cargo-exit must be cargo's numeric exit status, got ${v}`);
  return Number(v);
}

export function cargoCounts(logText, exit) {
  const c = checkCargo({ ...parseCargoLog(logText), exit: exitStatus(exit) });
  return {
    version: 1,
    kind: "cargo-counts",
    exit: c.exit,
    suites: c.suites,
    headers: c.headers,
    passed: c.passed,
    failed: c.failed,
    ignored: c.ignored,
    tripwireTests: c.tripwireTests,
  };
}

export function readCargoCounts(doc) {
  const d = typeof doc === "string" ? JSON.parse(doc) : doc;
  if (d?.version !== 1 || d.kind !== "cargo-counts") throw new Error("cargo counts: not a v1 cargo-counts file");
  for (const k of ["exit", "suites", "headers", "passed", "failed", "ignored"]) {
    if (!Number.isInteger(d[k]) || d[k] < 0) throw new Error(`cargo counts: ${k} is not a count`);
  }
  if (!Array.isArray(d.tripwireTests) || !d.tripwireTests.every((n) => typeof n === "string")) {
    throw new Error("cargo counts: tripwireTests is not a list of test names");
  }
  return checkCargo({ ...d });
}

export function parseVitestJson(doc) {
  const d = typeof doc === "string" ? JSON.parse(doc) : doc;
  const num = (k) => {
    if (!Number.isInteger(d?.[k]) || d[k] < 0) throw new Error(`vitest json: ${k} is not a count`);
    return d[k];
  };
  const out = {
    passed: num("numPassedTests"),
    skipped: num("numPendingTests") + num("numTodoTests"),
    failed: num("numFailedTests"),
    failedSuites: Number.isInteger(d.numFailedTestSuites) ? d.numFailedTestSuites : 0,
    success: d.success === true,
    tripwireTests: [],
  };
  for (const file of d.testResults ?? []) {
    for (const a of file.assertionResults ?? []) {
      if (a.status === "passed" && TRIPWIRE.test(a.fullName ?? "")) out.tripwireTests.push(a.fullName);
    }
  }
  return out;
}

function walkMd(dir, root, acc) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return acc;
  }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walkMd(p, root, acc);
    else if (e.isFile() && e.name.endsWith(".md")) acc.push(path.relative(root, p).split(path.sep).join("/"));
  }
  return acc;
}

const RULE5_KEYS = ["created", "branch", "author", "status"];

export function hasRule5Frontmatter(text) {
  const lines = String(text).split(/\r?\n/);
  if (lines[0] !== "---") return false;
  const end = lines.indexOf("---", 1);
  if (end < 0) return false;
  const keys = new Set(lines.slice(1, end).map((l) => /^([A-Za-z_][\w-]*):/.exec(l)?.[1]).filter(Boolean));
  return RULE5_KEYS.every((k) => keys.has(k));
}

// The required gate workflows (branch protection: ci, secret-scan, rust, node; merge-gate's ci
// summary also waits on eval-check). A gate step is a named step that runs a command; checkout,
// toolchain, auth, install, fetch and upload steps are setup, not gates.
const GATE_WORKFLOWS = ["ci.yml", "merge-gate.yml", "eval-check.yml"];
const SETUP_STEP = /^(authenticate|install|fetch|upload)\b/i;

export function countGateSteps(yamlText) {
  const lines = String(yamlText).split(/\r?\n/);
  let n = 0;
  for (let i = 0; i < lines.length; i++) {
    const m = /^(\s*)- name:\s*(.+?)\s*$/.exec(lines[i]);
    if (!m) continue;
    const indent = m[1].length;
    let runs = false;
    for (let j = i + 1; j < lines.length; j++) {
      const l = lines[j];
      if (l.trim() === "") continue;
      const ind = l.length - l.trimStart().length;
      if (ind <= indent) break;
      if (ind === indent + 2 && /^run:/.test(l.trimStart())) runs = true;
    }
    if (runs && !SETUP_STEP.test(m[2].replace(/^["']/, ""))) n += 1;
  }
  return n;
}

// The configs tla-models.yml runs: the tree's own runner decides, so an orphan config or an
// invalid EXPECT value never counts (Rule 9: one source of truth for "checked by TLC").
const TLC_PLAN = [
  "import json, sys",
  "formal = sys.argv[1]",
  "sys.path.insert(0, formal)",
  "import tlc_expect as t",
  "specs = t.discover_specs(formal)",
  "plans, errors = t.plan(specs, formal)",
  "print(json.dumps({'specs': specs, 'configs': sum(len(v) for v in plans.values()), 'errors': errors}))",
].join("\n");

export function tlcConfigsInCi(root) {
  const formal = path.join(root, "src-tauri", "formal");
  const wf = path.join(root, ".github", "workflows", "tla-models.yml");
  if (!fs.existsSync(path.join(formal, "tlc_expect.py"))) return { configs: 0, errors: [] };
  if (!fs.existsSync(wf) || !/tlc_expect\.py --all\b/.test(fs.readFileSync(wf, "utf8"))) return { configs: 0, errors: [] };
  const r = spawnSync("python3", ["-I", "-B", "-c", TLC_PLAN, formal], { encoding: "utf8" });
  if (r.error || r.status !== 0) {
    throw new Error(`tlc_expect.py discover/plan failed: ${r.error?.message ?? r.stderr.trim()}`);
  }
  const p = JSON.parse(r.stdout);
  // With any plan error `tlc_expect.py --all` exits before TLC runs: CI checks nothing.
  return { configs: p.errors.length ? 0 : p.configs, errors: p.errors };
}

export function measureTree(root) {
  const formal = path.join(root, "src-tauri", "formal");
  let formalFiles = [];
  try {
    formalFiles = fs.readdirSync(formal);
  } catch {
    formalFiles = [];
  }
  const tla = formalFiles.filter((f) => f.endsWith(".tla")).length;
  const tlc = tlcConfigsInCi(root);

  const docs = [...walkMd(path.join(root, "docs"), root, []), ...walkMd(path.join(root, ".agentile"), root, [])];
  const missing = docs.filter((rel) => !hasRule5Frontmatter(fs.readFileSync(path.join(root, rel), "utf8")));

  let ciSteps = 0;
  for (const wf of GATE_WORKFLOWS) {
    const p = path.join(root, ".github", "workflows", wf);
    if (fs.existsSync(p)) ciSteps += countGateSteps(fs.readFileSync(p, "utf8"));
  }
  // The LQ-WP03 registry, once it exists: its live (not replaced) entries. A registry that is
  // present but unreadable is a refusal, never a silent fall back to the runner count.
  let registered = null;
  const regPath = path.join(root, ".agentile", "coverage", "tripwires.json");
  if (fs.existsSync(regPath)) {
    const reg = JSON.parse(fs.readFileSync(regPath, "utf8"));
    if (!Array.isArray(reg?.tripwires)) throw new Error(".agentile/coverage/tripwires.json has no tripwires array");
    registered = reg.tripwires.filter((t) => !t?.replaced_by).length;
  }
  return {
    specs: { tla, tlc_configs_in_ci: tlc.configs },
    tlc_plan_errors: tlc.errors,
    tripwires_registered: registered,
    frontmatter: { docs: docs.length, with: docs.length - missing.length, missing },
    ci_steps: ciSteps,
  };
}

export function buildRatchet({ cargo, vitest, tree, rev, measured }) {
  checkCargo(cargo);
  if (vitest.failed > 0 || vitest.failedSuites > 0 || !vitest.success) {
    throw new Error(
      `vitest json: ${vitest.failed} failed test(s), ${vitest.failedSuites} failed suite(s); a red run is not a baseline`,
    );
  }
  const t = tree ?? { specs: { tla: 0, tlc_configs_in_ci: 0 }, frontmatter: { docs: 0, with: 0 }, ci_steps: 0 };
  return {
    version: 1,
    rev: rev ?? null,
    measured: measured ?? new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    tests: {
      rust: { passed: cargo.passed, ignored: cargo.ignored },
      vitest: { passed: vitest.passed, skipped: vitest.skipped },
    },
    specs: { tla: t.specs.tla, tlc_configs_in_ci: t.specs.tlc_configs_in_ci },
    tripwires: {
      registered: t.tripwires_registered ?? cargo.tripwireTests.length + vitest.tripwireTests.length,
      ci_steps: t.ci_steps,
    },
    frontmatter: { docs: t.frontmatter.docs, with: t.frontmatter.with },
  };
}

// ---- LQ-WP02: compare (the check `lq / ratchet`) ----
//
// Rules:
// - No ratcheted axis may go down. A decrease passes only with the `ratchet-adr` label AND a
//   docs/adr/<file>.md linked in the PR body that exists in the head and names the axis and
//   every test the diff removes.
// - The committed floor (the head's .agentile/coverage/ratchet.json) is ratcheted the same way:
//   a head that lowers an axis in it is a decrease under the same rule, and a head that deletes
//   it is refused. An accepted measured decrease must lower the floor to the measured count (or
//   less) in the same PR; otherwise every later PR, the record PR included, is refused on that
//   axis. More generally the floor is never above the head's measured count on any axis, ADR or
//   not: a PR cannot raise ratchet.json past what its own tree measures.
// - A test the diff removes and does not add back (in any file of the diff) needs the same label
//   and ADR whatever the counts say: the floor only advances when the record PR lands, so the
//   totals alone can hide a deletion. A rename counts as a removal of the old name.
// - A focused test (it/test/describe/suite .only) fails anywhere in the tree or the diff.
// - A new #[ignore] or it/test/describe .skip, .todo, .skipIf needs a reason and an issue link,
//   on its line or in the comment line above it.
// - No baseline at the merge base (before LQ-WP01 lands ratchet.json) is said out loud; the
//   diff and tree rules still apply.
export const AXES = [
  "tests.rust.passed", "tests.vitest.passed",
  "specs.tla", "specs.tlc_configs_in_ci",
  "tripwires.registered", "tripwires.ci_steps",
];
const ISSUE = /(?:^|[^\w&[])#\d+\b|github\.com\/[\w.-]+\/[\w.-]+\/issues\/\d+/;
// A call, not a mention: `it.only(`, `describe.only.each(`; a test title that names it.only is not one.
const FOCUS = /\b(?:it|test|describe|suite)\.only\s*(?:\(|\.each\b)/;
const SKIPS = /\b(?:it|test|describe|suite)\.(?:skip|todo|skipIf)\s*(?:\(|\.each\b)/;
const JS_EXT = /\.(?:[cm]?js|jsx|tsx?)$/;
const isComment = (l) => /^\s*(?:\/\/|\/?\*)/.test(l);
// A reason is an issue link plus some words: "needs anvil, #812", "(https://github.com/o/r/issues/812) live only".
const hasReason = (c) => ISSUE.test(c) && /[A-Za-z]{3,}/.test(c.replace(ISSUE, ""));
const commentOf = (l) => { const i = l.search(/\/\/|\/\*/); return i >= 0 ? l.slice(i) : ""; };

const get = (d, k) => k.split(".").reduce((o, s) => (o == null ? undefined : o[s]), d);
const read = (p) => { try { return fs.readFileSync(p, "utf8"); } catch { return null; } };
function walk(dir, keep, out = []) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name === "node_modules" || e.name === ".git" || e.name === "target") continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, keep, out);
    else if (e.isFile() && keep(e.name)) out.push(p);
  }
  return out;
}

// Added focused / ignored / skipped tests and the names of removed and added tests, from a
// unified diff. A rust test is a `fn` under a #[test] or #[tokio::test] attribute on the same side
// of the diff (old side: removed and context lines; new side: added and context lines), so a fn
// renamed under an unchanged attribute line is a removal of the old name and an addition of the new.
const TEST_ATTR = /^\s*#\[(?:tokio::)?test\b/;
const RS_FN = /\bfn (\w+)/;
const JS_TEST = /(?<![.\w$])(?:it|test)(?:\.each\([^)]*\))?\(\s*(["'`])(.+?)\1/;
//
// Each side has its own path: a removed line belongs to the `--- a/<path>` file and an added line
// to the `+++ b/<path>` file, so a deleted file (`+++ /dev/null`) still reports every test it
// removes, and a new file (`--- /dev/null`) is scanned at its new path. Inside a hunk (by the
// line counts in its `@@` header) a line that reads `--- x` or `+++ x` is content, not a header.
const diffPath = (s, prefix) => {
  let p = s.replace(/\t.*$/, "");
  if (p.length > 1 && p.startsWith('"') && p.endsWith('"')) p = p.slice(1, -1);
  if (p === "/dev/null") return "";
  return p.startsWith(prefix) ? p.slice(prefix.length) : p;
};
export function scanDiff(diff) {
  const out = { focused: [], unreasoned: [], removedTests: [], addedTests: [] };
  let oldFile = "", newFile = "", line = 0, prev = "", oldAttr = false, newAttr = false;
  let oldLeft = 0, newLeft = 0; // lines of the current hunk still to come on each side
  // Tracks a pending test attribute on one side; returns the fn name when this line completes it.
  const step = (pending, text) => {
    if (TEST_ATTR.test(text)) return [true, null];
    if (pending && RS_FN.test(text)) return [false, text.match(RS_FN)[1]];
    if (pending && /^\s*(?:#\[|\/\/)/.test(text)) return [true, null];
    return [false, null];
  };
  for (const raw of (diff || "").split("\n")) {
    // A file header always ends the hunk before it: no content line starts with "d".
    if (raw.startsWith("diff --git ")) { oldFile = newFile = ""; oldLeft = newLeft = 0; oldAttr = newAttr = false; continue; }
    if (oldLeft === 0 && newLeft === 0) {
      if (raw.startsWith("--- ")) { oldFile = diffPath(raw.slice(4), "a/"); oldAttr = false; continue; }
      if (raw.startsWith("+++ ")) { newFile = diffPath(raw.slice(4), "b/"); newAttr = false; continue; }
    }
    const h = raw.match(/^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
    if (h) {
      oldLeft = h[1] === undefined ? 1 : +h[1];
      newLeft = h[3] === undefined ? 1 : +h[3];
      line = +h[2]; prev = ""; oldAttr = newAttr = false;
      continue;
    }
    const tag = raw[0], text = raw.slice(1);
    if (tag !== "+" && tag !== " " && tag !== "-") continue;
    if (tag !== "+") oldLeft = Math.max(0, oldLeft - 1);
    if (tag !== "-") newLeft = Math.max(0, newLeft - 1);
    if (tag !== "+" && oldFile.endsWith(".rs")) {
      const [p, name] = step(oldAttr, text);
      oldAttr = p;
      if (name && tag === "-") out.removedTests.push(name);
    }
    if (tag !== "-" && newFile.endsWith(".rs")) {
      const [p, name] = step(newAttr, text);
      newAttr = p;
      if (name && tag === "+") out.addedTests.push(name);
    }
    const file = tag === "-" ? oldFile : newFile;
    const t = JS_EXT.test(file) && tag !== " " && !isComment(text) && text.match(JS_TEST);
    if (t) (tag === "-" ? out.removedTests : out.addedTests).push(t[2]);
    if (tag === "-") continue;
    if (tag === "+") {
      const at = `${file}:${line}`;
      const ok = (m) => hasReason(commentOf(text)) || (isComment(prev) && hasReason(prev)) || m;
      if (file.endsWith(".rs") && /^\s*#\[ignore\b/.test(text)) {
        const m = text.match(/#\[ignore\s*=\s*"([^"]*)"/);
        if (!ok(Boolean(m && hasReason(m[1])))) out.unreasoned.push(`${at}: ${text.trim()}`);
      } else if (JS_EXT.test(file) && !isComment(text)) {
        if (FOCUS.test(text)) out.focused.push(`${at}: ${text.trim()}`);
        else if (SKIPS.test(text) && !ok(false)) out.unreasoned.push(`${at}: ${text.trim()}`);
      }
    }
    prev = text; line += 1;
  }
  return out;
}

export function scanTreeForFocus(root) {
  const hits = [];
  for (const dir of ["src", "scripts"]) {
    for (const f of walk(path.join(root, dir), (n) => /\.(?:test|spec)\.(?:[cm]?js|jsx|tsx?)$/.test(n))) {
      (read(f) || "").split("\n").forEach((l, i) => {
        if (!isComment(l) && FOCUS.test(l)) hits.push(`${path.relative(root, f).split(path.sep).join("/")}:${i + 1}`);
      });
    }
  }
  return hits;
}

export const FLOOR_FILE = ".agentile/coverage/ratchet.json";
// Every ratcheted value, higher is better: the AXES, and frontmatter as minus the count of docs
// without it (a doc added without frontmatter is a decrease; a doc removed is not).
const RATCHETED = [
  ...AXES.map((k) => ({ axis: k, val: (d) => get(d, k), show: (v) => v })),
  { axis: "frontmatter", val: (d) => (d?.frontmatter ? d.frontmatter.with - d.frontmatter.docs : undefined), show: (v) => -v },
];
const num = (v) => typeof v === "number" && Number.isFinite(v);

// floor: the head's committed ratchet.json (parsed), null when the head has none, or left out
// (undefined) to skip the floor rules. The CLI always supplies it from --root.
export function compare(base, head, { root = ".", diff = "", labels = [], body = "", floor } = {}) {
  const failures = [], notes = [];
  const scan = scanDiff(diff);
  const decreases = []; // { axis, msg }
  if (!base) {
    notes.push("no ratchet.json at the merge base (LQ-WP01 not landed yet): axis comparison skipped; diff and tree rules still apply");
  } else {
    if (floor === null) failures.push(`${FLOOR_FILE} is missing or unreadable in the head; the floor is never deleted`);
    for (const { axis, val, show } of RATCHETED) {
      const b = val(base), h = val(head), f = floor ? val(floor) : undefined;
      if (!num(b)) continue;
      if (!num(h) || h < b) {
        decreases.push({
          axis,
          msg: axis === "frontmatter"
            ? `frontmatter: docs without frontmatter ${show(b)} -> ${show(h)} (+${show(h) - show(b)})`
            : `${axis} decreased ${b} -> ${h} (${h - b})`,
        });
      }
      if (floor && (!num(f) || f < b)) {
        decreases.push({ axis, msg: `${FLOOR_FILE} lowers ${axis} ${show(b)} -> ${num(f) ? show(f) : f}${num(f) ? ` (${axis === "frontmatter" ? "+" : ""}${show(f) - show(b)})` : ""}` });
      }
    }
  }
  // The floor is never above what the head measures, on any axis, label or not: the next PR and
  // the record PR are judged against it, so a floor the tree does not reach refuses all of them.
  // A measured decrease (accepted or not) must lower the floor in the same PR for the same reason.
  if (floor) {
    for (const { axis, val, show } of RATCHETED) {
      const b = base ? val(base) : undefined, h = val(head), f = val(floor);
      if (!num(f) || !num(h) || f <= h) continue;
      if (num(b) && h < b) {
        const target = axis === "frontmatter" ? `${show(h)} or more docs without frontmatter` : `${h} or less`;
        failures.push(`${axis}: an accepted decrease must lower ${FLOOR_FILE} to ${target} in the same PR (it records ${show(f)}), or every later PR is refused on this axis`);
      } else if (axis === "frontmatter") {
        failures.push(`frontmatter: ${FLOOR_FILE} records ${show(f)} docs without frontmatter, below the ${show(h)} this head measures; a floor the tree does not reach refuses every later PR, the record PR included`);
      } else {
        failures.push(`${axis}: ${FLOOR_FILE} records ${show(f)}, above the ${show(h)} this head measures; a floor the tree does not reach refuses every later PR, the record PR included`);
      }
    }
  }
  const added = new Set(scan.addedTests);
  const gone = [...new Set(scan.removedTests.filter((n) => !added.has(n)))];
  const needs = [...decreases.map((d) => d.msg), ...gone.map((n) => `test removed and not added back: ${n}`)];
  if (needs.length) {
    const names = [...new Set(decreases.map((d) => d.axis)), ...gone];
    let accepted = null;
    if (labels.includes("ratchet-adr")) {
      const refs = [...new Set([...(body || "").matchAll(/docs\/adr\/([A-Za-z0-9_-][\w.-]*\.md)\b/g)].map((m) => `docs/adr/${m[1]}`))];
      if (!refs.length) failures.push("ratchet-adr is set but no docs/adr/<file>.md linked in the PR body");
      const why = [];
      for (const ref of refs) {
        const text = read(path.join(root, ref));
        if (text == null) { why.push(`${ref} does not exist in the head`); continue; }
        const missing = names.filter((n) => !text.includes(n));
        if (missing.length) why.push(`${ref} does not name: ${missing.join(", ")}`);
        else { accepted = ref; break; }
      }
      if (!accepted) failures.push(...why);
    }
    if (accepted) notes.push(...needs.map((d) => `${d}: accepted by ${accepted}`));
    else failures.push(...needs.map((d) => `${d}; needs label ratchet-adr and a linked docs/adr/<file>.md naming the axis and each removed test`));
  }
  for (const f of scan.focused) failures.push(`focused test added (never allowed): ${f}`);
  for (const f of scanTreeForFocus(root)) failures.push(`focused test in the tree (never allowed): ${f}`);
  for (const f of scan.unreasoned) failures.push(`ignored/skipped test without a reason and an issue link: ${f}`);
  return { ok: failures.length === 0, failures, notes };
}

function usage(msg) {
  if (msg) process.stderr.write(`ratchet: ${msg}\n`);
  process.stderr.write(
    "usage: ratchet.mjs cargo-counts --cargo-log <file> --cargo-exit <code> --out <file>\n" +
      "       ratchet.mjs measure (--cargo-counts <file> | --cargo-log <file> --cargo-exit <code>)\n" +
      "                           --vitest-json <file> [--root <repo>] [--rev <sha>] [--out <file>]\n" +
      "       ratchet.mjs compare --base <file|none> --head <file> [--root <repo>] [--diff <file>]\n" +
      "                           [--labels a,b] [--pr-body <file>]\n",
  );
  process.exit(2);
}

const OPTIONS = {
  "cargo-counts": ["cargo-log", "cargo-exit", "out"],
  measure: ["cargo-counts", "cargo-log", "cargo-exit", "vitest-json", "root", "rev", "out"],
  compare: ["base", "head", "root", "diff", "labels", "pr-body"],
};

function refuse(e) {
  process.stderr.write(`ratchet: refused: ${e.message}\n`);
  process.exit(1);
}

function runCompare(opt) {
  if (!opt.base || !opt.head) usage("compare needs --base <file|none> and --head <file>");
  const root = opt.root || ".";
  const json = (p) => {
    try {
      return JSON.parse(fs.readFileSync(p, "utf8"));
    } catch {
      return null;
    }
  };
  const head = json(opt.head);
  const base = opt.base === "none" ? null : json(opt.base);
  if (!head || (opt.base !== "none" && !base)) {
    process.stderr.write("ratchet: refused: base or head file unreadable; fails closed\n");
    return 1;
  }
  // The head's committed floor, from the tree being judged; unreadable counts as missing.
  const floor = json(path.join(root, FLOOR_FILE));
  const r = compare(base, head, {
    root,
    floor,
    diff: opt.diff ? read(opt.diff) || "" : "",
    labels: (opt.labels || "").split(",").map((s) => s.trim()).filter(Boolean),
    body: opt["pr-body"] ? read(opt["pr-body"]) || "" : "",
  });
  for (const n of r.notes) process.stdout.write(`ratchet: note: ${n}\n`);
  for (const f of r.failures) process.stdout.write(`::error title=lq / ratchet::${f}\n`);
  process.stdout.write(r.ok ? "ratchet: pass\n" : `ratchet: refused (${r.failures.length})\n`);
  return r.ok ? 0 : 1;
}

function main(argv) {
  const [cmd, ...rest] = argv;
  if (!Object.hasOwn(OPTIONS, cmd ?? "")) usage(cmd ? `unknown command ${cmd}` : "no command");
  const opt = {};
  for (let i = 0; i < rest.length; i += 2) {
    const k = rest[i];
    const v = rest[i + 1];
    if (!k?.startsWith("--") || v === undefined) usage(`bad argument ${k}`);
    opt[k.slice(2)] = v;
  }
  for (const k of Object.keys(opt)) {
    if (!OPTIONS[cmd].includes(k)) usage(`unknown option --${k} for ${cmd}`);
  }
  if (opt["cargo-log"] && opt["cargo-exit"] === undefined) {
    usage("--cargo-log needs --cargo-exit (the exit status of the cargo run that wrote the log)");
  }

  if (cmd === "cargo-counts") {
    if (!opt["cargo-log"] || !opt.out) usage("--cargo-log, --cargo-exit and --out are required");
    let counts;
    try {
      counts = cargoCounts(fs.readFileSync(opt["cargo-log"], "utf8"), opt["cargo-exit"]);
    } catch (e) {
      refuse(e);
    }
    fs.writeFileSync(opt.out, `${JSON.stringify(counts, null, 2)}\n`);
    process.stdout.write(
      `cargo-counts: ${counts.suites} targets, ${counts.passed} passed, ${counts.ignored} ignored, ` +
        `${counts.tripwireTests.length} tripwire tests\n`,
    );
    return;
  }

  if (cmd === "compare") {
    process.exitCode = runCompare(opt);
    return;
  }

  if (!opt["cargo-counts"] === !opt["cargo-log"]) usage("give exactly one of --cargo-counts or --cargo-log");
  if (!opt["vitest-json"]) usage("--vitest-json is required");
  if (opt.rev && !/^[0-9a-f]{40}$/.test(opt.rev)) usage("--rev must be a full 40-hex commit sha");

  let ratchet;
  let tree;
  try {
    const cargo = opt["cargo-counts"]
      ? readCargoCounts(fs.readFileSync(opt["cargo-counts"], "utf8"))
      : checkCargo({ ...parseCargoLog(fs.readFileSync(opt["cargo-log"], "utf8")), exit: exitStatus(opt["cargo-exit"]) });
    const vitest = parseVitestJson(fs.readFileSync(opt["vitest-json"], "utf8"));
    tree = measureTree(path.resolve(opt.root ?? "."));
    ratchet = buildRatchet({ cargo, vitest, tree, rev: opt.rev });
  } catch (e) {
    refuse(e);
  }
  const text = `${JSON.stringify(ratchet, null, 2)}\n`;
  if (opt.out) fs.writeFileSync(opt.out, text);
  process.stdout.write(text);
  if (tree.frontmatter.missing.length) {
    process.stderr.write(`ratchet: Rule-5 frontmatter missing in:\n  ${tree.frontmatter.missing.join("\n  ")}\n`);
  }
  if (tree.tlc_plan_errors.length) {
    process.stderr.write(`ratchet: tlc_expect.py plan errors (no config counts):\n  ${tree.tlc_plan_errors.join("\n  ")}\n`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
