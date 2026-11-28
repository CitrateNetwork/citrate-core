#!/usr/bin/env node
// created: 2026-11-28 | branch: feat/v60-lq-wp18-review-record
// author: Larry Klosowski (LQ lane, LQ-WP18; Claude Opus 5.5)
// status: active
//
// lq / review-record (LQ-WP18, planset 2026-11-07-v060 lanes/LQ_NON_REGRESSION_GUARD.md): the
// two-independent-reviewer protocol as a check. The required Trail of Bits skills are computed from
// the diff's paths (the matrix below, step 4 of the protocol), never chosen by the author, and the
// PR body's `## Review record` must show that two independent reviewers between them ran every one:
//
//   1. At least two reviewer blocks, each with reviewer:, context:, skills:, findings:, measured:.
//      Template placeholders (<...>) and text inside HTML comments do not count.
//   2. No two blocks share a context id ("reviewers not independent").
//   3. The union of the blocks' skills covers every required skill (each missing one is named).
//   4. A money, authz or state-machine path needs a mutation campaign (mutation: ... surviving N)
//      in some block, and a surviving mutant there blocks (step 5).
//
// Honest limits: the record is author-written text. The check proves the record is complete and
// self-consistent, not that the reviews happened; the reviewer agents' posted approvals and the
// owner review (step 3, CODEOWNERS) are the evidence behind it. "At least one reviewer is another
// model family" is not machine-verifiable and is not enforced here.
//
// Data sources (Rule 7): `git diff --name-only` and `--numstat` from the merge-base of --base and
// HEAD; the PR body, title and labels from the read-only REST API at run time (event mode), so an
// edit followed by a re-run is judged on the current body.
//
// Usage:
//   node scripts/ci/lq/review-matrix.mjs --base <ref> [--root <dir>] [--title t] [--labels a,b]
//        prints the required skills and why
//   node scripts/ci/lq/review-matrix.mjs check --base <ref> --body <file> [--root] [--title] [--labels]
//   node scripts/ci/lq/review-matrix.mjs check --from-event [--base <ref>]   (GitHub Actions)
//   node scripts/ci/lq/review-matrix.mjs --self-test
// Exit: 0 pass, 1 refused (or unreadable input: fail closed), 2 usage.
//
// Zero dependencies: node built-ins and git.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

class UsageError extends Error {}

// Text that came from the PR is printed with control characters replaced, so it can never start a
// line and so never forms a workflow command.
export function clean(s) {
  return String(s).replace(/[\u0000-\u001f\u007f]/g, "?");
}

function globToRegExp(glob) {
  let re = "";
  for (let i = 0; i < glob.length; ) {
    if (glob.startsWith("**/", i)) { re += "(?:.*/)?"; i += 3; continue; }
    if (glob.startsWith("**", i)) { re += ".*"; i += 2; continue; }
    const ch = glob[i++];
    if (ch === "*") re += "[^/]*";
    else re += ch.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${re}$`, "i");
}

const TEST_FILE = [/(^|\/)tests?\//, /_tests?\.rs$/, /\.test\.[cm]?[jt]sx?$/, /\.spec\.[cm]?[jt]sx?$/, /(^|\/)fixtures\//];
export const isTestFile = (f) => TEST_FILE.some((re) => re.test(f));
const PROD_CODE = /\.(rs|ts|tsx|js|jsx|mjs|cjs|py|sh|sol)$/;

// The matrix (LQ-WP18 step 4). `paths` are globs over the changed files; `prod: true` rows ignore
// test files (a test-only change to a money path does not need a mutation campaign).
export const MATRIX = [
  { id: "every-pr", skills: ["differential-review"], always: true },
  { id: "config", skills: ["sharp-edges", "insecure-defaults"],
    paths: ["src/shell/state.ts", "src/shell/state/**", "**/tauri.conf.json", "src-tauri/capabilities/**", "src-tauri/permissions/**", "src-tauri/config/**", "**/*flags*.json", "**/*flags*.ts", "**/*feature_flags*"] },
  { id: "dependency", skills: ["supply-chain-risk-auditor"],
    paths: ["**/Cargo.toml", "**/Cargo.lock", "**/package.json", "**/package-lock.json", "**/npm-shrinkwrap.json", "release/components.json", "src-tauri/runtime-deps.sha256", "**/requirements*.txt", "**/.cargo/config*"] },
  { id: "workflow", skills: ["agentic-actions-auditor"], paths: [".github/workflows/**", ".github/actions/**"] },
  { id: "crypto", skills: ["constant-time-analysis", "zeroize-audit"],
    paths: ["kit/src/custody*", "**/secret.rs", "**/secret_*.rs", "**/siwe*", "**/eip712*", "**/*keychain*", "**/*mls*", "**/*pqc*", "**/updater*sign*", "**/*signing*", "**/*crypto*"], prod: true },
  { id: "contracts", skills: ["entry-point-analyzer", "building-secure-contracts", "spec-to-code-compliance"], paths: ["contracts/**", "**/*.sol"] },
  { id: "money-authz-state", skills: ["property-based-testing", "mutation-testing"], mutation: true,
    paths: ["kit/src/ceremony*", "kit/src/custody*", "**/*payout*", "**/*moneypath*", "**/*grant*", "**/*approval*", "**/*_gate.rs", "**/*Gate.ts", "**/*Gate.tsx", "**/*authz*", "**/*entitlement*", "**/*budget*", "**/*state_machine*", "**/*parser*", "**/*parse.rs"], prod: true },
  { id: "production-code", skills: ["semgrep", "codeql"], prodCode: true },
  { id: "protocol", skills: ["spec-to-code-compliance"], paths: ["**/*acp*", "**/*mcp*", "**/*oauth*", "**/*cimd*", "**/*hic*"], prod: true },
  { id: "large-surface", skills: ["audit-context-building"], minLines: 1000 },
  { id: "finding-fixed", skills: ["variant-analysis"], fix: true },
  { id: "units", skills: ["dimensional-analysis"],
    paths: ["release/budgets.json", "release/perf-budgets.json", "**/*budget*", "**/*gas*", "**/*fee*", "**/*quota*", "**/*_limit*"], prod: true },
];
const COMPILED = MATRIX.map((r) => ({ ...r, res: (r.paths || []).map(globToRegExp) }));
const FIX_TITLE = /^fix(\([^)]*\))?!?:/i;

// Which rows fire, with the files that fired each. opts: { title, labels, changedLines }.
export function requiredRows(files, opts = {}) {
  const out = [];
  for (const r of COMPILED) {
    let hits = [];
    if (r.always) hits = ["(every PR)"];
    else if (r.prodCode) hits = files.filter((f) => PROD_CODE.test(f) && !isTestFile(f) && !f.startsWith(".github/"));
    else if (r.minLines) hits = (opts.changedLines || 0) >= r.minLines ? [`${opts.changedLines} changed lines`] : [];
    else if (r.fix) hits = FIX_TITLE.test(opts.title || "") || (opts.labels || []).includes("finding-fix") ? ["fix title or finding-fix label"] : [];
    else hits = files.filter((f) => (!r.prod || !isTestFile(f)) && r.res.some((re) => re.test(f)));
    if (hits.length) out.push({ row: r, hits });
  }
  return out;
}

export function requiredSkills(files, opts = {}) {
  const seen = new Map();
  for (const { row, hits } of requiredRows(files, opts)) {
    for (const s of row.skills) if (!seen.has(s)) seen.set(s, { skill: s, why: `${row.id}: ${hits.slice(0, 3).join(", ")}${hits.length > 3 ? ", ..." : ""}` });
  }
  return [...seen.values()];
}

// "static-analysis:codeql", "trailofbits/Sharp-Edges " -> "codeql", "sharp-edges".
export function normalizeSkill(s) {
  return String(s).trim().replace(/^[`*_]+|[`*_]+$/g, "").split(/[:/]/).pop().trim().toLowerCase();
}

const PLACEHOLDER = /^(<[^>]*>|tbd|todo|n\/a|none|-|\.\.\.)?$/i;
const KEYS = ["reviewer", "context", "model", "skills", "findings", "measured", "mutation"];

// The `## Review record` section of a PR body, as blocks of fields. A block starts at reviewer:.
export function parseReviewRecord(body) {
  const text = String(body || "").replace(/\r\n?/g, "\n").replace(/<!--[\s\S]*?(-->|$)/g, "");
  const lines = text.split("\n");
  const start = lines.findIndex((l) => /^#{2,3}\s*review record\s*$/i.test(l.trim()));
  if (start < 0) return { found: false, blocks: [] };
  const blocks = [];
  let cur = null, lastKey = null;
  for (let i = start + 1; i < lines.length; i++) {
    const raw = lines[i];
    if (/^#{1,2}\s/.test(raw.trim()) && !/^###/.test(raw.trim())) break;
    const l = raw.replace(/^\s*(?:>\s*)?(?:[-*+]\s+)?/, "").replace(/\*\*/g, "").replace(/`/g, "");
    const m = /^([A-Za-z]+)\s*:\s*(.*)$/.exec(l.trim());
    const key = m && KEYS.includes(m[1].toLowerCase()) ? m[1].toLowerCase() : null;
    if (key) {
      if (key === "reviewer") { cur = { skills: [] }; blocks.push(cur); }
      if (!cur) continue;
      lastKey = key;
      const v = m[2].trim();
      if (key === "skills") cur.skills.push(...v.split(",").map(normalizeSkill).filter((s) => s && !PLACEHOLDER.test(s)));
      else cur[key] = PLACEHOLDER.test(v) ? "" : v;
    } else if (cur && lastKey === "skills" && /^\s+[-*+]\s+\S/.test(raw)) {
      const s = normalizeSkill(raw.replace(/^\s+[-*+]\s+/, ""));
      if (s && !PLACEHOLDER.test(s)) cur.skills.push(s);
    } else if (cur && lastKey && lastKey !== "skills" && /^\s+\S/.test(raw) && cur[lastKey]) {
      cur[lastKey] += ` ${raw.trim()}`;
    } else if (raw.trim() === "") {
      lastKey = null;
    }
  }
  return { found: true, blocks };
}

// The rules. input: { files, body, title, labels, changedLines }.
export function evaluate({ files, body, title = "", labels = [], changedLines = 0 }) {
  const errors = [];
  const rows = requiredRows(files, { title, labels, changedLines });
  const required = requiredSkills(files, { title, labels, changedLines });
  const rec = parseReviewRecord(body);
  if (!rec.found) {
    errors.push("the PR body has no `## Review record` section (one block per reviewer: reviewer:, context:, skills:, findings:, measured:)");
    return { ok: false, errors, required, blocks: [] };
  }
  const blocks = rec.blocks.filter((b) => b.reviewer);
  if (blocks.length < 2) errors.push(`two independent reviewers are required; the review record has ${blocks.length} reviewer block(s)`);
  blocks.forEach((b, i) => {
    const who = `reviewer ${i + 1} (${clean(b.reviewer).slice(0, 60)})`;
    for (const k of ["context", "findings", "measured"]) if (!b[k]) errors.push(`${who}: ${k}: is missing or a placeholder`);
    if (!b.skills.length) errors.push(`${who}: skills: lists no skill`);
  });
  const ctx = new Map();
  for (const b of blocks) {
    if (!b.context) continue;
    const k = b.context.trim().toLowerCase();
    if (ctx.has(k)) errors.push(`reviewers not independent: two review blocks carry the same context id '${clean(b.context).slice(0, 80)}'`);
    ctx.set(k, b);
  }
  const ran = new Set(blocks.flatMap((b) => b.skills));
  for (const r of required) if (!ran.has(r.skill)) errors.push(`missing skill ${r.skill} (required by ${clean(r.why)}): no review record lists it`);
  if (rows.some(({ row }) => row.mutation)) {
    const campaigns = blocks.map((b) => b.mutation).filter(Boolean);
    if (!campaigns.length) errors.push("money, authz or state-machine path changed: the review record needs a mutation campaign (mutation: generated N, killed N, surviving N, and why)");
    for (const c of campaigns) {
      const m = /surviv\w*\s*[:=]?\s*(\d+)/i.exec(c);
      if (!m) errors.push(`mutation campaign does not state its surviving count: '${clean(c).slice(0, 120)}'`);
      else if (Number(m[1]) > 0) errors.push(`mutation campaign has ${m[1]} surviving mutant(s) on a money, authz or state-machine path: kill them before merge`);
    }
  }
  return { ok: errors.length === 0, errors, required, blocks };
}

// ---- git --------------------------------------------------------------------------------------

function gitIn(root) {
  return (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 << 20 });
}

export function diffOf(root, base, head = "HEAD") {
  const git = gitIn(root);
  let mb;
  try { mb = git("merge-base", base, head).trim(); } catch { throw new UsageError(`no merge-base for '${clean(base)}' and '${clean(head)}'`); }
  const files = git("diff", "--name-only", "-z", "--no-renames", mb, head).split("\0").filter(Boolean);
  let changedLines = 0;
  for (const l of git("diff", "--numstat", "--no-renames", mb, head).split("\n")) {
    const [a, d] = l.split("\t");
    if (/^\d+$/.test(a)) changedLines += Number(a);
    if (/^\d+$/.test(d)) changedLines += Number(d);
  }
  return { mergeBase: mb, files, changedLines };
}

// ---- GitHub (event mode) ---------------------------------------------------------------------

async function eventContext(env) {
  const name = env.GITHUB_EVENT_NAME;
  if (!env.GITHUB_EVENT_PATH) throw new UsageError("--from-event needs GITHUB_EVENT_PATH");
  const ev = JSON.parse(fs.readFileSync(env.GITHUB_EVENT_PATH, "utf8"));
  const token = env.GH_TOKEN || env.GITHUB_TOKEN;
  const apiUrl = (env.GITHUB_API_URL || "https://api.github.com").replace(/\/$/, "");
  const repo = env.GITHUB_REPOSITORY || "";
  if (!token) throw new Error("no GH_TOKEN for the read-only GitHub API (fail closed)");
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) throw new Error("GITHUB_REPOSITORY is not owner/name");
  let number, base;
  if (name === "pull_request") { number = ev.pull_request?.number; base = ev.pull_request?.base?.sha; }
  else if (name === "merge_group") {
    const m = /\/pr-(\d+)-[0-9a-f]+$/.exec(ev.merge_group?.head_ref || "");
    if (!m) throw new Error("merge_group head_ref names no PR (fail closed)");
    number = Number(m[1]);
    base = "HEAD^1"; // the group commit's parent: main plus the entries ahead of this one
  } else throw new UsageError(`unsupported event '${clean(name)}'`);
  if (!Number.isInteger(number)) throw new Error("event has no pull request (fail closed)");
  // The body is read at run time, not from the event payload, so a re-run sees the current record.
  const r = await fetch(`${apiUrl}/repos/${repo}/pulls/${number}`, { headers: { Authorization: `Bearer ${token}`, Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" } });
  if (!r.ok) throw new Error(`GitHub API ${r.status} reading the pull request (fail closed)`);
  const pr = await r.json();
  return { number, base, body: pr.body || "", title: pr.title || "", labels: (pr.labels || []).map((l) => l.name) };
}

// ---- self-test -------------------------------------------------------------------------------

export function selfTest() {
  const failures = [], lines = [];
  const want = (label, cond) => (cond ? lines : failures).push(label);
  const has = (files, skill, opts) => requiredSkills(files, opts).some((r) => r.skill === skill);
  want("workflow -> agentic-actions-auditor", has([".github/workflows/ci.yml"], "agentic-actions-auditor"));
  want("README -> differential-review only", requiredSkills(["README.md"]).length === 1);
  want("test file is not production code", !has(["src/a.test.ts"], "semgrep"));
  want("money path -> mutation-testing", has(["kit/src/ceremony.rs"], "mutation-testing"));
  const rec = "## Review record\nreviewer: a\ncontext: x\nskills: differential-review\nfindings: 0\nmeasured: m\n\nreviewer: b\ncontext: X\nskills: differential-review\nfindings: 0\nmeasured: m\n";
  want("same context id refused", evaluate({ files: ["README.md"], body: rec }).errors.some((e) => /not independent/.test(e)));
  want("distinct contexts pass", evaluate({ files: ["README.md"], body: rec.replace("context: X", "context: y") }).ok);
  want("control characters cleaned", clean("a\u001b::b") === "a?::b");
  return { failures, lines };
}

// ---- CLI -------------------------------------------------------------------------------------

function parseArgs(argv) {
  const o = { labels: [] };
  const flags = new Set(["--base", "--root", "--body", "--title", "--labels"]);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--from-event") { o.fromEvent = true; continue; }
    if (!flags.has(a)) throw new UsageError(`unknown argument '${clean(a)}'`);
    const v = argv[++i];
    if (v === undefined) throw new UsageError(`${a} needs a value`);
    o[a.slice(2)] = v;
  }
  if (typeof o.labels === "string") o.labels = o.labels.split(",").map((s) => s.trim()).filter(Boolean);
  return o;
}

async function main(argv) {
  if (argv[0] === "--self-test") {
    const r = selfTest();
    for (const l of r.lines) console.log(`  ok: ${l}`);
    for (const l of r.failures) console.log(`  SELFTEST FAIL: ${l}`);
    console.log(r.failures.length ? "SELFTEST: FAIL" : "SELFTEST: PASS");
    return r.failures.length ? 1 : 0;
  }
  const isCheck = argv[0] === "check";
  const o = parseArgs(isCheck ? argv.slice(1) : argv);
  const root = o.root || process.cwd();
  let body = null, title = o.title || "", labels = o.labels, base = o.base, pr = null;
  if (o.fromEvent) {
    const ev = await eventContext(process.env);
    ({ body, title, labels } = ev);
    base = o.base || ev.base;
    pr = ev.number;
  } else if (o.body) body = fs.readFileSync(o.body, "utf8");
  if (!base) throw new UsageError("usage: review-matrix.mjs --base <ref> | check --base <ref> --body <file> | check --from-event | --self-test");
  const d = diffOf(root, base);
  const required = requiredSkills(d.files, { title, labels, changedLines: d.changedLines });
  console.log(`lq / review-record: ${pr ? `PR #${pr} ` : ""}merge-base ${d.mergeBase.slice(0, 12)}; ${d.files.length} changed file(s), ${d.changedLines} line(s)`);
  console.log("required skills:");
  for (const r of required) console.log(`  - ${r.skill}  (${clean(r.why)})`);
  if (!isCheck) return 0;
  if (body === null) throw new UsageError("check needs --body <file> or --from-event");
  const r = evaluate({ files: d.files, body, title, labels, changedLines: d.changedLines });
  for (const [i, b] of r.blocks.entries()) console.log(`  record ${i + 1}: reviewer ${clean(b.reviewer || "?").slice(0, 60)}, context ${clean(b.context || "?").slice(0, 60)}, skills [${b.skills.map(clean).join(", ")}]`);
  if (r.ok) { console.log("PASS"); return 0; }
  console.log("REFUSED:");
  for (const e of r.errors) console.log(`  - ${clean(e)}`);
  return 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then((code) => process.exit(code), (e) => {
    if (e instanceof UsageError) { console.error(`review-matrix: ${clean(e.message)}`); process.exit(2); }
    console.error(`review-matrix: REFUSED (fail closed): ${clean(e.message)}`);
    process.exit(1);
  });
}
