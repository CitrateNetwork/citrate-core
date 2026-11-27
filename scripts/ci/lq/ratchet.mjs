#!/usr/bin/env node
// created: 2026-11-26 | branch: feat/v60-lq-wp01-baseline
// author: Larry Klosowski (LQ lane, 0.6.0)
// status: active
//
// LQ-WP01: the coverage ratchet's numbers are the test runners' own counts, never a grep.
//
//   node scripts/ci/lq/ratchet.mjs measure --cargo-log <file> --vitest-json <file>
//        [--root <repo>] [--rev <sha>] [--out <ratchet.json>]
//
// Data sources (Rule 7):
//   rust    the `test result:` summary lines of `cargo test --workspace --locked` (libtest), one
//           per test binary and doc-test target; passed and ignored are summed separately.
//   vitest  the json reporter file of `vitest run --reporter=json` (numPassedTests; skipped is
//           numPendingTests + numTodoTests).
//   tree    src-tauri/formal/*.tla and the *.cfg files carrying an `EXPECT:` line (what
//           tla-models.yml checks); Markdown under docs/ and .agentile/ with all four Rule-5 keys
//           (created, branch, author, status) in a leading frontmatter block; the named `run:`
//           steps of the required gate workflows (ci.yml, merge-gate.yml, eval-check.yml),
//           setup steps excepted.
//   tripwires  until the LQ-WP03 registry (.agentile/coverage/tripwires.json) exists,
//           `registered` is the number of tests the runners executed whose full name contains
//           "tripwire" (rust per-test lines plus vitest assertion full names).
//
// Exit codes: 0 written, 1 refused (a failed test, no summary line, a bad reporter file),
// 2 usage. A refused run writes nothing: a red run is never a baseline.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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

export function measureTree(root) {
  const formal = path.join(root, "src-tauri", "formal");
  let formalFiles = [];
  try {
    formalFiles = fs.readdirSync(formal);
  } catch {
    formalFiles = [];
  }
  const tla = formalFiles.filter((f) => f.endsWith(".tla")).length;
  const tlcConfigs = formalFiles
    .filter((f) => f.endsWith(".cfg"))
    .filter((f) => fs.readFileSync(path.join(formal, f), "utf8").includes("EXPECT:")).length;

  const docs = [...walkMd(path.join(root, "docs"), root, []), ...walkMd(path.join(root, ".agentile"), root, [])];
  const missing = docs.filter((rel) => !hasRule5Frontmatter(fs.readFileSync(path.join(root, rel), "utf8")));

  let ciSteps = 0;
  for (const wf of GATE_WORKFLOWS) {
    const p = path.join(root, ".github", "workflows", wf);
    if (fs.existsSync(p)) ciSteps += countGateSteps(fs.readFileSync(p, "utf8"));
  }
  return {
    specs: { tla, tlc_configs_in_ci: tlcConfigs },
    frontmatter: { docs: docs.length, with: docs.length - missing.length, missing },
    ci_steps: ciSteps,
  };
}

export function buildRatchet({ cargo, vitest, tree, rev, measured }) {
  if (cargo.suites === 0) throw new Error('cargo log: no "test result" line (empty or truncated log)');
  if (cargo.failed > 0) throw new Error(`cargo log: ${cargo.failed} failed test(s); a red run is not a baseline`);
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
    "usage: ratchet.mjs measure --cargo-log <file> --vitest-json <file> [--root <repo>] [--rev <sha>] [--out <file>]\n",
  );
  process.exit(2);
}

function main(argv) {
  const [cmd, ...rest] = argv;
  if (cmd !== "measure") usage(cmd ? `unknown command ${cmd}` : "no command");
  const opt = {};
  for (let i = 0; i < rest.length; i += 2) {
    const k = rest[i];
    const v = rest[i + 1];
    if (!k?.startsWith("--") || v === undefined) usage(`bad argument ${k}`);
    opt[k.slice(2)] = v;
  }
  for (const k of Object.keys(opt)) {
    if (!["cargo-log", "vitest-json", "root", "rev", "out"].includes(k)) usage(`unknown option --${k}`);
  }
  if (!opt["cargo-log"] || !opt["vitest-json"]) usage("--cargo-log and --vitest-json are required");
  if (opt.rev && !/^[0-9a-f]{40}$/.test(opt.rev)) usage("--rev must be a full 40-hex commit sha");

  let ratchet;
  let tree;
  try {
    const cargo = parseCargoLog(fs.readFileSync(opt["cargo-log"], "utf8"));
    const vitest = parseVitestJson(fs.readFileSync(opt["vitest-json"], "utf8"));
    tree = measureTree(path.resolve(opt.root ?? "."));
    ratchet = buildRatchet({ cargo, vitest, tree, rev: opt.rev });
  } catch (e) {
    process.stderr.write(`ratchet: refused: ${e.message}\n`);
    process.exit(1);
  }
  const text = `${JSON.stringify(ratchet, null, 2)}\n`;
  if (opt.out) fs.writeFileSync(opt.out, text);
  process.stdout.write(text);
  if (tree.frontmatter.missing.length) {
    process.stderr.write(`ratchet: Rule-5 frontmatter missing in:\n  ${tree.frontmatter.missing.join("\n  ")}\n`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
