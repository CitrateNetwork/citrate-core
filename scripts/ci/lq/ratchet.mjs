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
// Rules:
// - No ratcheted axis may go down. A decrease passes only with the `ratchet-adr` label AND a
//   docs/adr/<file>.md linked in the PR body that exists in the head and names the axis and
//   every test the diff removes.
// - A focused test (it/test/describe/suite .only) fails anywhere in the tree or the diff.
// - A new #[ignore] or it/test/describe .skip, .todo, .skipIf needs a reason and an issue link,
//   on its line or in the comment line above it.
// - No baseline at the merge base (before LQ-WP01 lands ratchet.json) is said out loud; the
//   diff and tree rules still apply.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const AXES = [
  "tests.rust.passed", "tests.vitest.passed",
  "specs.tla", "specs.tlc_configs_in_ci",
  "tripwires.registered", "tripwires.ci_steps",
];
const CI_WORKFLOWS = ["ci.yml", "merge-gate.yml", "eval-check.yml"]; // the merge-queue workflows
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
    else if (keep(e.name)) out.push(p);
  }
  return out;
}
const get = (d, k) => k.split(".").reduce((o, s) => (o == null ? undefined : o[s]), d);

export function parseCargoLog(text) {
  const r = { passed: 0, ignored: 0, failed: 0, binaries: 0 };
  for (const m of text.matchAll(/^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored/gm)) {
    r.passed += +m[1]; r.failed += +m[2]; r.ignored += +m[3]; r.binaries += 1;
  }
  return r;
}

export function parseVitestJson(v) {
  return {
    passed: v.numPassedTests | 0,
    skipped: (v.numPendingTests | 0) + (v.numTodoTests | 0),
    failed: (v.numFailedTests | 0) + (v.numFailedTestSuites | 0),
  };
}

export function measure({ root = ".", cargoLog, vitestJson, rev = "", now = new Date().toISOString() }) {
  const rust = parseCargoLog(cargoLog);
  if (rust.binaries === 0) throw new Error('cargo log has no "test result" lines; refusing to measure');
  if (rust.failed) throw new Error(`cargo log has ${rust.failed} failed test(s); a red run is not a measurement`);
  const vit = parseVitestJson(vitestJson);
  if (vit.failed) throw new Error(`vitest report has ${vit.failed} failed test(s) or suite(s); a red run is not a measurement`);
  const formal = path.join(root, "src-tauri", "formal");
  const inFormal = (ext) => { try { return fs.readdirSync(formal).filter((f) => f.endsWith(ext)); } catch { return []; } };
  const reg = JSON.parse(read(path.join(root, ".agentile/coverage/tripwires.json")) || '{"tripwires":[]}');
  const steps = CI_WORKFLOWS.map((w) => read(path.join(root, ".github/workflows", w)) || "")
    .reduce((n, t) => n + (t.match(/^\s*- (?:name|uses|run):/gm) || []).length, 0);
  const docs = [...walk(path.join(root, "docs"), (n) => n.endsWith(".md")),
    ...walk(path.join(root, ".agentile"), (n) => n.endsWith(".md"))];
  const withFm = docs.filter((f) => /^---\r?\n[\s\S]*?\r?\n---/.test(read(f) || "")).length;
  return {
    version: 1, rev, measured: now,
    tests: { rust: { passed: rust.passed, ignored: rust.ignored }, vitest: { passed: vit.passed, skipped: vit.skipped } },
    specs: {
      tla: inFormal(".tla").length,
      tlc_configs_in_ci: inFormal(".cfg").filter((f) => (read(path.join(formal, f)) || "").includes("EXPECT:")).length,
    },
    tripwires: { registered: (reg.tripwires || []).filter((t) => !t.replaced_by).length, ci_steps: steps },
    frontmatter: { docs: docs.length, with: withFm },
  };
}

// Added focused / ignored / skipped tests and the names of removed tests, from a unified diff.
export function scanDiff(diff) {
  const out = { focused: [], unreasoned: [], removedTests: [] };
  let file = "", line = 0, prev = "", removedAttr = false;
  for (const raw of (diff || "").split("\n")) {
    if (raw.startsWith("+++ ")) { file = raw.slice(4).replace(/^b\//, ""); continue; }
    if (raw.startsWith("--- ") || raw.startsWith("diff --git")) continue;
    const h = raw.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (h) { line = +h[1]; prev = ""; continue; }
    const tag = raw[0], text = raw.slice(1);
    if (tag === "-") {
      if (/^\s*#\[(?:tokio::)?test\b/.test(text)) removedAttr = true;
      else if (removedAttr && /\bfn (\w+)/.test(text)) { out.removedTests.push(text.match(/\bfn (\w+)/)[1]); removedAttr = false; }
      const t = JS_EXT.test(file) && !isComment(text) && text.match(/\b(?:it|test)(?:\.each\([^)]*\))?\(\s*(["'`])(.+?)\1/);
      if (t) out.removedTests.push(t[2]);
      continue;
    }
    if (tag !== "+" && tag !== " ") continue;
    removedAttr = false;
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

export function compare(base, head, { root = ".", diff = "", labels = [], body = "" } = {}) {
  const failures = [], notes = [];
  const scan = scanDiff(diff);
  const decreases = [];
  if (!base) {
    notes.push("no ratchet.json at the merge base (LQ-WP01 not landed yet): axis comparison skipped; diff and tree rules still apply");
  } else {
    for (const k of AXES) {
      const b = get(base, k), h = get(head, k);
      if (typeof b === "number" && (typeof h !== "number" || h < b)) decreases.push(`${k} decreased ${b} -> ${h} (${h - b})`);
    }
    const missB = base.frontmatter.docs - base.frontmatter.with, missH = head.frontmatter.docs - head.frontmatter.with;
    if (missH > missB) decreases.push(`frontmatter: docs without frontmatter ${missB} -> ${missH} (+${missH - missB})`);
  }
  if (decreases.length) {
    const axes = decreases.map((d) => d.split(/ decreased|:/)[0]);
    let accepted = null;
    if (labels.includes("ratchet-adr")) {
      const refs = [...new Set([...(body || "").matchAll(/docs\/adr\/([A-Za-z0-9_-][\w.-]*\.md)\b/g)].map((m) => `docs/adr/${m[1]}`))];
      if (!refs.length) failures.push("ratchet-adr is set but no docs/adr/<file>.md linked in the PR body");
      const why = [];
      for (const ref of refs) {
        const text = read(path.join(root, ref));
        if (text == null) { why.push(`${ref} does not exist in the head`); continue; }
        const missing = [...axes, ...scan.removedTests].filter((n) => !text.includes(n));
        if (missing.length) why.push(`${ref} does not name: ${missing.join(", ")}`);
        else { accepted = ref; break; }
      }
      if (!accepted) failures.push(...why);
    }
    if (accepted) notes.push(...decreases.map((d) => `${d}: accepted by ${accepted}`));
    else failures.push(...decreases.map((d) => `${d}; needs label ratchet-adr and a linked docs/adr/<file>.md naming the axis and each removed test`));
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
    let d;
    try { d = measure({ root, cargoLog, vitestJson: JSON.parse(vj), rev: opt.rev || "" }); } catch (e) { console.error(`lq/ratchet: ${e.message}`); return 1; }
    const s = `${JSON.stringify(d, null, 2)}\n`;
    if (opt.out) fs.writeFileSync(opt.out, s); else process.stdout.write(s);
    return 0;
  }
  if (cmd === "compare") {
    if (!opt.base || !opt.head) return usage("compare needs --base <file|none> and --head <file>");
    const head = JSON.parse(read(opt.head) || "null");
    const base = opt.base === "none" ? null : JSON.parse(read(opt.base) || "null");
    if (!head || (opt.base !== "none" && !base)) { console.error("lq/ratchet: base or head file unreadable; fails closed"); return 1; }
    const r = compare(base, head, {
      root, diff: opt.diff ? read(opt.diff) || "" : "",
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
