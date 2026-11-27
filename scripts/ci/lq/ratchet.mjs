#!/usr/bin/env node
// created: 2026-11-26 | branch: feat/v60-lq-wp01-baseline
// author: Larry Klosowski (LQ lane, 0.6.0)
// status: active
//
// LQ-WP01: the coverage ratchet's numbers are the test runners' own counts, never a grep.
//
//   node scripts/ci/lq/ratchet.mjs cargo-counts --cargo-log <file> --cargo-exit <code> --out <file>
//   node scripts/ci/lq/ratchet.mjs measure (--cargo-counts <file> | --cargo-log <file> --cargo-exit <code>)
//        --vitest-json <file> [--root <repo>] [--rev <sha>] [--out <ratchet.json>]
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
//           tla-models.yml calls them; 0 when either is absent or the plan has an error); Markdown under docs/ and .agentile/ with all four Rule-5 keys
//           (created, branch, author, status) in a leading frontmatter block; the named `run:`
//           steps of the required gate workflows (ci.yml, merge-gate.yml, eval-check.yml),
//           setup steps excepted.
//   tripwires  until the LQ-WP03 registry (.agentile/coverage/tripwires.json) exists,
//           `registered` is the number of tests the runners executed whose full name contains
//           "tripwire" (rust per-test lines plus vitest assertion full names).
//
// Exit codes: 0 written, 1 refused (a failed test, a non-zero cargo exit, a target with no summary
// line, a bad reporter or counts file), 2 usage. A refused run writes nothing: a red run is never a baseline.
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
  return {
    specs: { tla, tlc_configs_in_ci: tlc.configs },
    tlc_plan_errors: tlc.errors,
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
    tripwires: { registered: cargo.tripwireTests.length + vitest.tripwireTests.length, ci_steps: t.ci_steps },
    frontmatter: { docs: t.frontmatter.docs, with: t.frontmatter.with },
  };
}

function usage(msg) {
  if (msg) process.stderr.write(`ratchet: ${msg}\n`);
  process.stderr.write(
    "usage: ratchet.mjs cargo-counts --cargo-log <file> --cargo-exit <code> --out <file>\n" +
      "       ratchet.mjs measure (--cargo-counts <file> | --cargo-log <file> --cargo-exit <code>)\n" +
      "                           --vitest-json <file> [--root <repo>] [--rev <sha>] [--out <file>]\n",
  );
  process.exit(2);
}

const OPTIONS = {
  "cargo-counts": ["cargo-log", "cargo-exit", "out"],
  measure: ["cargo-counts", "cargo-log", "cargo-exit", "vitest-json", "root", "rev", "out"],
};

function refuse(e) {
  process.stderr.write(`ratchet: refused: ${e.message}\n`);
  process.exit(1);
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
