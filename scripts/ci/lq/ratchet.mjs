#!/usr/bin/env node
// created: 2026-11-26 | branch: feat/v60-lq-wp02-ratchet-job
// author: Claude Opus 5.5 (LQ-WP02 worker) for Larry Klosowski
// status: active
//
// LQ-WP02: the test-count ratchet behind the `lq / ratchet` check (Rule 2).
//
//   node scripts/ci/lq/ratchet.mjs measure --cargo-log <f> --vitest-json <f> [--root .] [--rev sha] [--out f]
//   node scripts/ci/lq/ratchet.mjs compare --base <ratchet.json|none> --head <f> [--root .]
//                                         [--diff <unified diff>] [--labels a,b] [--pr-body <f>]
//   exit 0 pass, 1 refused, 2 usage.
//
// Data sources (Rule 7): the rust job's `cargo test --workspace --locked --no-fail-fast` log and
// the node job's vitest json report (ci.yml, uploaded as artifacts), the tree for specs, EXPECT
// configs, the tripwire registry, CI steps and frontmatter, the merge base's
// .agentile/coverage/ratchet.json, and the PR diff, labels and body.
//
// Measure rules (LQ-WP01, the rules the committed baseline was measured with; a head is only
// comparable with its base when both are counted the same way):
//   rust    the `test result:` summary lines of the cargo log (libtest), one per test binary and
//           doc-test target; passed and ignored are summed separately.
//   vitest  the json reporter file (numPassedTests; skipped is numPendingTests + numTodoTests).
//   tree    src-tauri/formal/*.tla and the *.cfg files carrying an `EXPECT:` line (what
//           tla-models.yml checks); Markdown under docs/ and .agentile/ with all four Rule-5 keys
//           (created, branch, author, status) in a leading frontmatter block; the named `run:`
//           steps of the merge-queue gate workflows (ci.yml, merge-gate.yml, eval-check.yml),
//           setup steps excepted.
//   tripwires  the live (not replaced) entries of the LQ-WP03 registry
//           (.agentile/coverage/tripwires.json) once it exists; until then the tests the runners
//           passed whose full name contains "tripwire".
//
// Rules:
// - No ratcheted axis may go down. A decrease passes only with the `ratchet-adr` label AND a
//   docs/adr/<file>.md linked in the PR body that exists in the head and names the axis and
//   every test the diff removes.
// - The committed floor (the head's .agentile/coverage/ratchet.json) is ratcheted the same way:
//   a head that lowers an axis in it is a decrease under the same rule, and a head that deletes
//   it is refused. An accepted measured decrease must lower the floor to the measured count (or
//   less) in the same PR; otherwise every later PR, the record PR included, is refused on that
//   axis.
// - A test the diff removes and does not add back (in any file of the diff) needs the same label
//   and ADR whatever the counts say: the floor only advances when the record PR lands, so the
//   totals alone can hide a deletion. A rename counts as a removal of the old name.
// - A focused test (it/test/describe/suite .only) fails anywhere in the tree or the diff.
// - A new #[ignore] or it/test/describe .skip, .todo, .skipIf needs a reason and an issue link,
//   on its line or in the comment line above it.
// - No baseline at the merge base (before LQ-WP01 lands ratchet.json) is said out loud; the
//   diff and tree rules still apply.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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
const get = (d, k) => k.split(".").reduce((o, s) => (o == null ? undefined : o[s]), d);

const SUMMARY =
  /test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out/;
// libtest per-test line: `test <name> ... ok|ignored[, reason]|FAILED`. GitHub log exports put
// job, step and timestamp columns in front of it; anything before " test " is ignored.
const PER_TEST = /(?:^|\s)test (\S+) \.\.\. (ok|ignored|FAILED)\b/;
const TRIPWIRE = /tripwire/i;

export function parseCargoLog(text) {
  const out = { suites: 0, passed: 0, failed: 0, ignored: 0, tripwireTests: [] };
  for (const line of String(text).split(/\r?\n/)) {
    const s = SUMMARY.exec(line);
    if (s) {
      out.suites += 1;
      out.passed += Number(s[2]);
      out.failed += Number(s[3]);
      out.ignored += Number(s[4]);
      if (s[1] === "FAILED" && Number(s[3]) === 0) out.failed += 1;
      continue;
    }
    const t = PER_TEST.exec(line);
    if (t && t[2] !== "ignored" && TRIPWIRE.test(t[1])) out.tripwireTests.push(t[1]);
  }
  return out;
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

const RULE5_KEYS = ["created", "branch", "author", "status"];

export function hasRule5Frontmatter(text) {
  const lines = String(text).split(/\r?\n/);
  if (lines[0] !== "---") return false;
  const end = lines.indexOf("---", 1);
  if (end < 0) return false;
  const keys = new Set(lines.slice(1, end).map((l) => /^([A-Za-z_][\w-]*):/.exec(l)?.[1]).filter(Boolean));
  return RULE5_KEYS.every((k) => keys.has(k));
}

// The merge-queue gate workflows (branch protection: ci, secret-scan, rust, node; merge-gate's ci
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

export function measureTree(root) {
  const formal = path.join(root, "src-tauri", "formal");
  let formalFiles = [];
  try { formalFiles = fs.readdirSync(formal); } catch { formalFiles = []; }
  const tla = formalFiles.filter((f) => f.endsWith(".tla")).length;
  const tlcConfigs = formalFiles
    .filter((f) => f.endsWith(".cfg"))
    .filter((f) => (read(path.join(formal, f)) || "").includes("EXPECT:")).length;

  const rel = (p) => path.relative(root, p).split(path.sep).join("/");
  const docs = [...walk(path.join(root, "docs"), (n) => n.endsWith(".md")),
    ...walk(path.join(root, ".agentile"), (n) => n.endsWith(".md"))].map(rel);
  const missing = docs.filter((r) => !hasRule5Frontmatter(read(path.join(root, r)) || ""));

  let ciSteps = 0;
  for (const wf of GATE_WORKFLOWS) {
    const text = read(path.join(root, ".github", "workflows", wf));
    if (text != null) ciSteps += countGateSteps(text);
  }
  // The LQ-WP03 registry, once it exists: its live (not replaced) entries. A registry that is
  // present but unreadable is a refusal, never a silent fall back to the runner count.
  let registered = null;
  const regText = read(path.join(root, ".agentile", "coverage", "tripwires.json"));
  if (regText != null) {
    const reg = JSON.parse(regText);
    if (!Array.isArray(reg?.tripwires)) throw new Error(".agentile/coverage/tripwires.json has no tripwires array");
    registered = reg.tripwires.filter((t) => !t?.replaced_by).length;
  }
  return {
    specs: { tla, tlc_configs_in_ci: tlcConfigs },
    frontmatter: { docs: docs.length, with: docs.length - missing.length, missing },
    ci_steps: ciSteps,
    tripwires_registered: registered,
  };
}

export function buildRatchet({ cargo, vitest, tree, rev, measured }) {
  if (cargo.suites === 0) throw new Error('cargo log: no "test result" line (empty or truncated log)');
  if (cargo.failed > 0) throw new Error(`cargo log: ${cargo.failed} failed test(s); a red run is not a measurement`);
  if (vitest.failed > 0 || vitest.failedSuites > 0 || !vitest.success) {
    throw new Error(
      `vitest json: ${vitest.failed} failed test(s), ${vitest.failedSuites} failed suite(s); a red run is not a measurement`,
    );
  }
  const t = tree ?? { specs: { tla: 0, tlc_configs_in_ci: 0 }, frontmatter: { docs: 0, with: 0 }, ci_steps: 0 };
  const runnerTripwires = cargo.tripwireTests.length + vitest.tripwireTests.length;
  return {
    version: 1,
    rev: rev ?? null,
    measured: measured ?? new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    tests: {
      rust: { passed: cargo.passed, ignored: cargo.ignored },
      vitest: { passed: vitest.passed, skipped: vitest.skipped },
    },
    specs: { tla: t.specs.tla, tlc_configs_in_ci: t.specs.tlc_configs_in_ci },
    tripwires: { registered: t.tripwires_registered ?? runnerTripwires, ci_steps: t.ci_steps },
    frontmatter: { docs: t.frontmatter.docs, with: t.frontmatter.with },
  };
}

export function measure({ root = ".", cargoLog, vitestJson, rev = null, now }) {
  return buildRatchet({
    cargo: parseCargoLog(cargoLog), vitest: parseVitestJson(vitestJson),
    tree: measureTree(root), rev, measured: now,
  });
}

// Added focused / ignored / skipped tests and the names of removed and added tests, from a
// unified diff. A rust test is a `fn` under a #[test] or #[tokio::test] attribute on the same side
// of the diff (old side: removed and context lines; new side: added and context lines), so a fn
// renamed under an unchanged attribute line is a removal of the old name and an addition of the new.
const TEST_ATTR = /^\s*#\[(?:tokio::)?test\b/;
const RS_FN = /\bfn (\w+)/;
const JS_TEST = /(?<![.\w$])(?:it|test)(?:\.each\([^)]*\))?\(\s*(["'`])(.+?)\1/;
export function scanDiff(diff) {
  const out = { focused: [], unreasoned: [], removedTests: [], addedTests: [] };
  let file = "", line = 0, prev = "", oldAttr = false, newAttr = false;
  // Tracks a pending test attribute on one side; returns the fn name when this line completes it.
  const step = (pending, text) => {
    if (TEST_ATTR.test(text)) return [true, null];
    if (pending && RS_FN.test(text)) return [false, text.match(RS_FN)[1]];
    if (pending && /^\s*(?:#\[|\/\/)/.test(text)) return [true, null];
    return [false, null];
  };
  for (const raw of (diff || "").split("\n")) {
    if (raw.startsWith("+++ ")) { file = raw.slice(4).replace(/^b\//, ""); oldAttr = newAttr = false; continue; }
    if (raw.startsWith("--- ") || raw.startsWith("diff --git")) continue;
    const h = raw.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (h) { line = +h[1]; prev = ""; oldAttr = newAttr = false; continue; }
    const tag = raw[0], text = raw.slice(1);
    if (tag !== "+" && tag !== " " && tag !== "-") continue;
    const rs = file.endsWith(".rs");
    if (rs && tag !== "+") {
      const [p, name] = step(oldAttr, text);
      oldAttr = p;
      if (name && tag === "-") out.removedTests.push(name);
    }
    if (rs && tag !== "-") {
      const [p, name] = step(newAttr, text);
      newAttr = p;
      if (name && tag === "+") out.addedTests.push(name);
    }
    const t = JS_EXT.test(file) && tag !== " " && !isComment(text) && text.match(JS_TEST);
    if (t) (tag === "-" ? out.removedTests : out.addedTests).push(t[2]);
    if (tag === "-") continue;
    if (tag === "+") {
      const at = `${file}:${line}`;
      const ok = (m) => hasReason(commentOf(text)) || (isComment(prev) && hasReason(prev)) || m;
      if (rs && /^\s*#\[ignore\b/.test(text)) {
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
        // Accepted or not, a measured decrease the floor does not follow refuses every later PR.
        if (floor && num(h) && !(num(f) && f <= h)) {
          const target = axis === "frontmatter" ? `${show(h)} or more docs without frontmatter` : `${h} or less`;
          failures.push(`${axis}: an accepted decrease must lower ${FLOOR_FILE} to ${target} in the same PR (it records ${num(f) ? show(f) : f}), or every later PR is refused on this axis`);
        }
      }
      if (floor && (!num(f) || f < b)) {
        decreases.push({ axis, msg: `${FLOOR_FILE} lowers ${axis} ${show(b)} -> ${num(f) ? show(f) : f}${num(f) ? ` (${axis === "frontmatter" ? "+" : ""}${show(f) - show(b)})` : ""}` });
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

function main(argv) {
  const [cmd, ...rest] = argv;
  const opt = {};
  for (let i = 0; i < rest.length; i += 2) {
    if (!rest[i].startsWith("--") || rest[i + 1] === undefined) return usage(`bad argument ${rest[i]}`);
    opt[rest[i].slice(2)] = rest[i + 1];
  }
  const root = opt.root || ".";
  if (cmd === "measure") {
    if (!opt["cargo-log"] || !opt["vitest-json"]) return usage("measure needs --cargo-log and --vitest-json");
    const cargoLog = read(opt["cargo-log"]), vj = read(opt["vitest-json"]);
    if (cargoLog == null || vj == null) { console.error("lq/ratchet: a runner artifact is missing; fails closed"); return 1; }
    if (opt.rev && !/^[0-9a-f]{40}$/.test(opt.rev)) return usage("--rev must be a full 40-hex commit sha");
    let d;
    try { d = measure({ root, cargoLog, vitestJson: vj, rev: opt.rev || null }); } catch (e) { console.error(`lq/ratchet: refused: ${e.message}`); return 1; }
    const s = `${JSON.stringify(d, null, 2)}\n`;
    if (opt.out) fs.writeFileSync(opt.out, s);
    process.stdout.write(s);
    return 0;
  }
  if (cmd === "compare") {
    if (!opt.base || !opt.head) return usage("compare needs --base <file|none> and --head <file>");
    const head = JSON.parse(read(opt.head) || "null");
    const base = opt.base === "none" ? null : JSON.parse(read(opt.base) || "null");
    if (!head || (opt.base !== "none" && !base)) { console.error("lq/ratchet: base or head file unreadable; fails closed"); return 1; }
    // The head's committed floor, from the tree being judged; unreadable counts as missing.
    let floor = null;
    try { floor = JSON.parse(read(path.join(root, FLOOR_FILE)) || "null"); } catch { floor = null; }
    const r = compare(base, head, {
      root, floor, diff: opt.diff ? read(opt.diff) || "" : "",
      labels: (opt.labels || "").split(",").map((s) => s.trim()).filter(Boolean),
      body: opt["pr-body"] ? read(opt["pr-body"]) || "" : "",
    });
    for (const n of r.notes) console.log(`lq/ratchet: note: ${n}`);
    for (const f of r.failures) console.log(`::error title=lq / ratchet::${f}`);
    console.log(r.ok ? "lq/ratchet: pass" : `lq/ratchet: refused (${r.failures.length})`);
    return r.ok ? 0 : 1;
  }
  return usage(`unknown command ${cmd ?? ""}`);
}
function usage(msg) {
  console.error(`lq/ratchet: ${msg}\nusage: ratchet.mjs measure --cargo-log f --vitest-json f [--root d] [--rev sha] [--out f]\n       ratchet.mjs compare --base f|none --head f [--root d] [--diff f] [--labels a,b] [--pr-body f]`);
  return 2;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
