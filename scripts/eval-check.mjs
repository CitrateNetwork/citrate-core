#!/usr/bin/env node
// =====================================================================
// citrate-core: the deterministic eval check, no model (HUP-S11.2, US-11.2)
//
//   node scripts/eval-check.mjs [--root <repo>] [--skills-sources <dir>]
//   node scripts/eval-check.mjs --fetch-skill-sources <dir>     shallow-fetch every skills.lock source
//   node scripts/eval-check.mjs --write-pins                    regenerate src/agent/eval/datasets.sha256
//
// What the `eval-check` pull-request workflow (.github/workflows/eval-check.yml) runs. It calls
// no model and needs no npm install (Node >= 22.18 strips the TypeScript types of the eval
// modules it imports). Checks, each printed as `ok   <name>` or `FAIL <name>: <why>`:
//
//   datasets            toolcall v1/v2, injection v1/v2 (header + fragments merged), workflow-v1
//                       and every qa-*.json set load through the same validators the eval CLIs use,
//                       against the real AGENT_TOOLS; a QA citation must exist in its anchor index.
//   dataset versions    every dataset file's `version` is its own file name.
//   dataset pins        src/agent/eval/datasets.sha256 pins every *.json under src/agent/eval by
//                       sha256 (no unpinned, no missing, no changed file), and the two v1 pins equal
//                       the frozen A50 hashes in src/agent/eval/frozenPins.ts.
//   skills.lock         the lock parses strictly, every skill names its source's commit, every hash
//                       is a sha256, and the sources match .agentile/skill-intake/intake.json. With
//                       --skills-sources <dir> (checkouts made by --fetch-skill-sources) the lock is
//                       also recomputed from the sources (scripts/skills-lock.mjs --check).
//   scorecard           every *.json in eval/results is a scorecard, the committed SCORECARD.md
//                       matches a fresh render, and each tier present renders on its own.
//   sidecar runtime pin eval/sidecar-runtime.rev names CitrateNetwork/citrate-agent-runtime and a
//                       full 40-hex commit (the rev eval.yml builds the sidecar from).
//
// Exit 0 all checks passed, 1 a check failed, 2 usage. Zero dependencies beyond Node.
// =====================================================================
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { FROZEN_V1_SHA256 } from "../src/agent/eval/frozenPins.ts";
import { loadInjectionDataset, loadToolcallDataset } from "../src/agent/eval/datasetFiles.ts";
import { parseWorkflowDataset } from "../src/agent/eval/sidecar.ts";
import { findMissingCitations, parseQaDataset } from "../src/agent/eval/qa.ts";
import { parseLockToml } from "./stage-skills-bundle.mjs";
import { checkLock, loadIntake } from "./skills-lock.mjs";
import { filterByTier, loadScorecards, renderScorecardMarkdown, splitFrontmatter } from "./eval-scorecard.mjs";

const HEX40 = /^[0-9a-f]{40}$/;
const HEX64 = /^[0-9a-f]{64}$/;
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const toPosix = (p) => p.split(path.sep).join("/");

export const PINS_FILE = "datasets.sha256";
export const RUNTIME_REPO = "CitrateNetwork/citrate-agent-runtime";

// ---------------------------------------------------------------------------------------------
// Dataset pins
// ---------------------------------------------------------------------------------------------

/** Every *.json under the eval dir (recursively), as sorted posix paths relative to it. */
export function listDatasetFiles(evalDir) {
  const out = [];
  const walk = (rel) => {
    for (const ent of fs.readdirSync(path.join(evalDir, rel), { withFileTypes: true })) {
      const r = rel ? `${rel}/${ent.name}` : ent.name;
      if (ent.isDirectory()) walk(r);
      else if (ent.isFile() && ent.name.endsWith(".json")) out.push(r);
    }
  };
  walk("");
  // Plain code-unit order, so the manifest is the same on every platform and locale.
  return out.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

const PINS_HEADER = [
  "# sha256 of every eval dataset file under src/agent/eval (HUP-S11.2). Checked by",
  "# scripts/eval-check.mjs in the eval-check pull-request job. A dataset change is a reviewed",
  "# change to this file: regenerate with `node scripts/eval-check.mjs --write-pins`.",
  "# toolcall-v1.json and injection-v1.json are frozen (A50, frozenPins.ts) and never move.",
  "# Format: `shasum -a 256` output, paths relative to src/agent/eval.",
];

/** The manifest text for the files on disk. */
export function renderPins(evalDir) {
  const lines = listDatasetFiles(evalDir).map((rel) => `${sha256(fs.readFileSync(path.join(evalDir, rel)))}  ${rel}`);
  return [...PINS_HEADER, ...lines, ""].join("\n");
}

/** Parse a manifest. Throws on any line that is not a comment, blank or `<sha256>  <safe rel path>`. */
export function parsePins(text) {
  const pins = new Map();
  text.split("\n").forEach((l, i) => {
    if (l === "" || l.startsWith("#")) return;
    const m = /^([0-9a-f]{64}) {2}(\S+)$/.exec(l);
    const rel = m?.[2];
    const safe = rel !== undefined && rel.endsWith(".json") && rel.split("/").every((s) => s && s !== "." && s !== "..");
    if (!m || !safe) throw new Error(`${PINS_FILE} line ${i + 1}: expected "<sha256>  <path>.json"`);
    if (pins.has(rel)) throw new Error(`${PINS_FILE} line ${i + 1}: ${rel} pinned twice`);
    pins.set(rel, m[1]);
  });
  return pins;
}

/** Problems with the manifest against the files on disk and the frozen v1 pins. */
export function checkPins(evalDir, pinsText) {
  let pins;
  try {
    pins = parsePins(pinsText);
  } catch (e) {
    return [e.message];
  }
  const problems = [];
  const onDisk = listDatasetFiles(evalDir);
  for (const rel of onDisk) {
    const actual = sha256(fs.readFileSync(path.join(evalDir, rel)));
    if (!pins.has(rel)) problems.push(`${rel}: not pinned in ${PINS_FILE} (sha256 ${actual})`);
    else if (pins.get(rel) !== actual) problems.push(`${rel}: sha256 ${actual}, but ${pins.get(rel)} is pinned`);
  }
  for (const rel of pins.keys()) {
    if (!onDisk.includes(rel)) problems.push(`${rel}: pinned but missing`);
  }
  for (const [rel, frozen] of Object.entries(FROZEN_V1_SHA256)) {
    if (pins.get(rel) !== frozen) problems.push(`${rel}: frozen (A50) at ${frozen}; the manifest must not move it`);
  }
  return problems;
}

// ---------------------------------------------------------------------------------------------
// Datasets
// ---------------------------------------------------------------------------------------------

/** Every top-level dataset file's `version` equals its file name (anchor indexes: their set's).
 *  A held-out manifest (`<set>.heldout.json`, HUP-S9.3) is not a dataset: it carries
 *  `format: "citrate-heldout-v1"` and names its set in `dataset`. */
export function checkDatasetVersions(evalDir) {
  const problems = [];
  for (const rel of listDatasetFiles(evalDir)) {
    if (rel.includes("/")) continue; // fragment files carry {added, by}, not a version
    let raw;
    try {
      raw = JSON.parse(fs.readFileSync(path.join(evalDir, rel), "utf8"));
    } catch (e) {
      problems.push(`${rel}: not JSON (${e.message})`);
      continue;
    }
    if (rel.endsWith(".heldout.json")) {
      const set = rel.slice(0, -".heldout.json".length);
      if (raw?.format !== "citrate-heldout-v1") problems.push(`${rel}: format ${JSON.stringify(raw?.format)}, expected "citrate-heldout-v1"`);
      if (raw?.dataset !== set) problems.push(`${rel}: dataset ${JSON.stringify(raw?.dataset)}, expected "${set}"`);
      continue;
    }
    const want = rel.replace(/\.anchors\.json$|\.json$/, "");
    if (raw?.version !== want) problems.push(`${rel}: version ${JSON.stringify(raw?.version)}, expected "${want}"`);
  }
  return problems;
}

/** Load every dataset the eval CLIs score, through their own validators. */
export async function checkDatasets(evalDir) {
  const problems = [];
  const dsFs = { readText: (p) => fs.readFileSync(p, "utf8"), listDir: (p) => fs.readdirSync(p) };
  const readJson = (rel) => JSON.parse(fs.readFileSync(path.join(evalDir, rel), "utf8"));
  const attempt = (label, fn) => {
    try {
      fn();
    } catch (e) {
      problems.push(`${label}: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  for (const gen of ["v1", "v2"]) {
    attempt(`toolcall-${gen}`, () => loadToolcallDataset(dsFs, evalDir, gen));
    attempt(`injection-${gen}`, () => loadInjectionDataset(dsFs, evalDir, gen));
  }
  attempt("workflow-v1", () => parseWorkflowDataset(readJson("workflow-v1.json")));
  const qaSets = fs
    .readdirSync(evalDir)
    .filter((f) => /^qa-[a-z0-9-]+\.json$/.test(f) && !f.endsWith(".anchors.json"))
    .sort();
  if (!qaSets.length) problems.push("no qa-*.json set found");
  for (const f of qaSets) {
    const name = f.slice(0, -".json".length);
    attempt(name, () => {
      const ds = parseQaDataset(readJson(f));
      const missing = findMissingCitations(ds, readJson(`${name}.anchors.json`));
      if (missing.length) throw new Error(`citations missing from ${name}.anchors.json: ${missing.join("; ")}`);
    });
  }
  return problems;
}

// ---------------------------------------------------------------------------------------------
// skills.lock
// ---------------------------------------------------------------------------------------------

/** Structural checks that need no source checkout. */
export function checkSkillsLock(lockText, intake) {
  let lock;
  try {
    lock = parseLockToml(lockText);
  } catch (e) {
    return [e.message];
  }
  const problems = [];
  const sources = new Map();
  for (const s of lock.sources) {
    if (sources.has(s.label)) problems.push(`source ${s.label}: listed twice`);
    sources.set(s.label, s);
    if (!HEX40.test(s.commit ?? "")) problems.push(`source ${s.label}: commit is not a full sha`);
  }
  const seen = new Set();
  for (const sk of lock.skills) {
    const src = sources.get(sk.source);
    if (!src) problems.push(`skill ${sk.name}: unknown source ${sk.source}`);
    else if (sk.commit !== src.commit) problems.push(`skill ${sk.name}: commit ${sk.commit} is not its source's (${src.commit})`);
    const key = `${sk.source}:${sk.path}`;
    if (seen.has(key)) problems.push(`skill ${sk.name}: ${key} listed twice`);
    seen.add(key);
    if (!HEX64.test(sk.skill_md_sha256 ?? "")) problems.push(`skill ${sk.name}: skill_md_sha256 is not a sha256`);
    if (sk.shipped_skill_md_sha256 !== undefined && !HEX64.test(sk.shipped_skill_md_sha256)) {
      problems.push(`skill ${sk.name}: shipped_skill_md_sha256 is not a sha256`);
    }
    if (sk.verdict === "exclude" && (sk.refs.length || sk.stripped.length)) problems.push(`skill ${sk.name}: excluded but lists files`);
  }
  const intakeSources = new Map((intake?.sources ?? []).map((s) => [s.label, s]));
  for (const s of lock.sources) {
    const i = intakeSources.get(s.label);
    if (!i) {
      problems.push(`source ${s.label}: not in intake.json`);
      continue;
    }
    for (const k of ["upstream", "commit", "local"]) {
      if (i[k] !== s[k]) problems.push(`source ${s.label}: ${k} differs from intake.json (${JSON.stringify(i[k])} vs ${JSON.stringify(s[k])})`);
    }
  }
  for (const label of intakeSources.keys()) {
    if (!sources.has(label)) problems.push(`source ${label}: in intake.json but not in skills.lock`);
  }
  return problems;
}

/** The fetch URL of a source: the repo named in "(via owner/repo)", else the upstream URL. */
function sourceRepoUrl(upstream) {
  const via = /\(via ([\w.-]+\/[\w.-]+)\)/.exec(upstream);
  if (via) return `https://github.com/${via[1]}`;
  const m = /^https:\/\/github\.com\/([\w.-]+\/[\w.-]+?)(?:\.git)?\/?$/.exec(upstream.trim());
  if (!m) throw new Error(`cannot derive a GitHub repo from upstream ${JSON.stringify(upstream)}`);
  return `https://github.com/${m[1]}`;
}

/**
 * One checkout per (repo, commit): the directory (relative to the sources base) to check it out
 * in, so every source's `local` path exists below it. Sources sharing a checkout use their common
 * parent directory.
 */
export function skillSourcePlan(lockText) {
  const lock = parseLockToml(lockText);
  const groups = new Map();
  for (const s of lock.sources) {
    const url = sourceRepoUrl(s.upstream);
    const key = `${url}@${s.commit}`;
    if (!groups.has(key)) groups.set(key, { url, commit: s.commit, locals: [] });
    groups.get(key).locals.push(s.local.split("/"));
  }
  const plan = [];
  for (const g of groups.values()) {
    let common = g.locals[0];
    if (g.locals.length > 1) {
      common = [];
      for (let i = 0; g.locals.every((l) => l.length > i + 1 && l[i] === g.locals[0][i]); i++) common.push(g.locals[0][i]);
    }
    if (!common.length || common.some((s) => !s || s === "." || s === "..")) {
      throw new Error(`sources at ${g.url} share no checkout directory`);
    }
    plan.push({ dir: common.join("/"), url: g.url, commit: g.commit });
  }
  return plan;
}

function fetchSkillSources(lockText, base) {
  for (const p of skillSourcePlan(lockText)) {
    const dir = path.join(base, p.dir);
    fs.mkdirSync(dir, { recursive: true });
    const git = (...a) => execFileSync("git", ["-C", dir, ...a], { stdio: ["ignore", "ignore", "inherit"] });
    git("init", "-q");
    git("fetch", "-q", "--depth", "1", p.url, p.commit);
    git("checkout", "-q", "--detach", "FETCH_HEAD");
    const head = execFileSync("git", ["-C", dir, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
    if (head !== p.commit) throw new Error(`${p.url}: checked out ${head}, expected ${p.commit}`);
    console.log(`fetched ${p.url} @ ${p.commit} -> ${p.dir}`);
  }
}

// ---------------------------------------------------------------------------------------------
// Scorecard
// ---------------------------------------------------------------------------------------------

export const SCORECARD_TIERS = ["T0", "T1", "T2", "none"];

/** The committed SCORECARD.md matches its JSON, every JSON is a scorecard, every tier renders. */
export function checkScorecard(resultsDir) {
  const problems = [];
  const cards = loadScorecards(resultsDir);
  for (const f of cards.skipped) problems.push(`${f}: not a tool-call, QA or sidecar scorecard`);
  const total = cards.tools.length + cards.qa.length + cards.sidecar.length;
  if (!total) return [...problems, `no scorecard in ${resultsDir}`];
  const mdFile = path.join(resultsDir, "SCORECARD.md");
  if (!fs.existsSync(mdFile)) return [...problems, "SCORECARD.md is missing"];
  const { front, body } = splitFrontmatter(fs.readFileSync(mdFile, "utf8"));
  const created = /created: (\S+)/.exec(front)?.[1];
  const branch = /branch: (\S+)/.exec(front)?.[1];
  if (!created || !branch) problems.push("SCORECARD.md has no created/branch frontmatter");
  const fresh = renderScorecardMarkdown(cards, { created: created ?? "", branch: branch ?? "", source: "eval/results" });
  if (splitFrontmatter(fresh).body !== body) {
    problems.push("SCORECARD.md does not match a fresh render of its JSON (run node scripts/eval-scorecard.mjs)");
  }
  let rendered = 0;
  for (const tier of SCORECARD_TIERS) {
    const sub = filterByTier(cards, tier);
    const n = sub.tools.length + sub.qa.length + sub.sidecar.length;
    if (!n) continue;
    const md = renderScorecardMarkdown(sub, { created: created ?? "", branch: branch ?? "", source: "eval/results", tier });
    const rows = md.split("\n").filter((l) => /^\| \S+\.json \|/.test(l)).length;
    if (rows < n) problems.push(`tier ${tier}: ${n} scorecards but ${rows} rows rendered`);
    rendered += n;
  }
  if (rendered !== total) problems.push(`per-tier renders cover ${rendered} of ${total} scorecards`);
  return problems;
}

// ---------------------------------------------------------------------------------------------
// Sidecar runtime pin
// ---------------------------------------------------------------------------------------------

/** `repo = <owner/name>` and `rev = <40 hex>` (comments and blank lines allowed). */
export function parseRuntimePin(text) {
  const kv = {};
  for (const l of text.split("\n")) {
    if (l.trim() === "" || l.startsWith("#")) continue;
    const m = /^(repo|rev) = (\S+)$/.exec(l);
    if (!m || m[1] in kv) throw new Error(`sidecar-runtime.rev: cannot read ${JSON.stringify(l.slice(0, 60))}`);
    kv[m[1]] = m[2];
  }
  return kv;
}

export function checkRuntimePin(text) {
  let kv;
  try {
    kv = parseRuntimePin(text);
  } catch (e) {
    return [e.message];
  }
  const problems = [];
  if (kv.repo !== RUNTIME_REPO) problems.push(`sidecar-runtime.rev: repo must be ${RUNTIME_REPO} (got ${kv.repo ?? "nothing"})`);
  if (!HEX40.test(kv.rev ?? "")) problems.push(`sidecar-runtime.rev: rev must be a full 40-hex commit (got ${kv.rev ?? "nothing"})`);
  return problems;
}

// ---------------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------------

const USAGE =
  "usage: node scripts/eval-check.mjs [--root <repo>] [--skills-sources <dir>] | --fetch-skill-sources <dir> | --write-pins [--root <repo>]";

function parseArgs(argv) {
  const out = {};
  const value = { "--root": "root", "--skills-sources": "skillsSources", "--fetch-skill-sources": "fetchTo" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--write-pins") {
      out.writePins = true;
      continue;
    }
    if (!(a in value)) throw new Error(`unknown argument ${JSON.stringify(a)}\n${USAGE}`);
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${USAGE}`);
    out[value[a]] = v;
    i++;
  }
  return out;
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  const root = path.resolve(args.root ?? path.join(path.dirname(fileURLToPath(import.meta.url)), ".."));
  const evalDir = path.join(root, "src", "agent", "eval");
  const lockText = fs.readFileSync(path.join(root, "skills.lock"), "utf8");

  if (args.fetchTo) {
    try {
      fetchSkillSources(lockText, path.resolve(args.fetchTo));
    } catch (e) {
      console.error(`fetch failed: ${e instanceof Error ? e.message : String(e)}`);
      process.exit(1);
    }
    return;
  }

  if (args.writePins) {
    const text = renderPins(evalDir);
    const frozen = checkPins(evalDir, text).filter((p) => p.includes("frozen (A50)"));
    if (frozen.length) {
      console.error(`refusing to write ${PINS_FILE}:\n  ${frozen.join("\n  ")}`);
      process.exit(1);
    }
    fs.writeFileSync(path.join(evalDir, PINS_FILE), text);
    console.log(`wrote ${toPosix(path.relative(root, path.join(evalDir, PINS_FILE)))} (${listDatasetFiles(evalDir).length} files)`);
    return;
  }

  const checks = [];
  checks.push(["datasets", await checkDatasets(evalDir)]);
  checks.push(["dataset versions", checkDatasetVersions(evalDir)]);
  const pinsPath = path.join(evalDir, PINS_FILE);
  checks.push(["dataset pins", fs.existsSync(pinsPath) ? checkPins(evalDir, fs.readFileSync(pinsPath, "utf8")) : [`${PINS_FILE} is missing`]]);
  let intake = null;
  const lockProblems = [];
  try {
    intake = loadIntake(path.join(root, ".agentile", "skill-intake", "intake.json"));
  } catch (e) {
    lockProblems.push(`intake.json: ${e instanceof Error ? e.message : String(e)}`);
  }
  lockProblems.push(...checkSkillsLock(lockText, intake));
  if (args.skillsSources && intake) {
    const r = checkLock(lockText, intake, path.resolve(args.skillsSources));
    if (!r.ok) lockProblems.push(...r.diffs.slice(0, 20).map((d) => `recomputed from the sources: ${d}`));
  }
  checks.push([args.skillsSources ? "skills.lock (recomputed from the pinned sources)" : "skills.lock", lockProblems]);
  checks.push(["scorecard", checkScorecard(path.join(root, "eval", "results"))]);
  const pinFile = path.join(root, "eval", "sidecar-runtime.rev");
  checks.push(["sidecar runtime pin", fs.existsSync(pinFile) ? checkRuntimePin(fs.readFileSync(pinFile, "utf8")) : ["eval/sidecar-runtime.rev is missing"]]);

  let failed = 0;
  for (const [name, problems] of checks) {
    if (!problems.length) {
      console.log(`ok   ${name}`);
      continue;
    }
    failed++;
    console.log(`FAIL ${name}:`);
    for (const p of problems) console.log(`       ${p}`);
  }
  if (failed) {
    console.log(`${failed} of ${checks.length} checks failed`);
    process.exit(1);
  }
  console.log(`all ${checks.length} checks passed`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(e instanceof Error ? e.stack : String(e));
    process.exit(1);
  });
}
