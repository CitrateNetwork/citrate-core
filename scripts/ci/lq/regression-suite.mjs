#!/usr/bin/env node
// created: 2026-11-28 | branch: feat/v60-lq-wp12-regression-suite
// author: Larry Klosowski (LQ lane, LQ-WP12; Claude Opus 5.5)
// status: active
//
// LQ-WP12 (planset 2026-11-07-v060, lanes/LQ_NON_REGRESSION_GUARD.md): the shared regression suite,
// check `lq / regression-suite` (.github/workflows/lq-regression.yml), on every PR and merge group.
//
// Registry (data source, Rule 7): every suite in ci/characterization.json (L0) and
// ci/characterization.d/*.json (one per lane), plus the surface map in
// .agentile/coverage/characterization.json (LQ; schema v1 frozen: surface, owner_lane, paths and
// the existing suites that characterize the surface at lane start). The verdict reads the runners'
// own output: the libtest `test <name> ... ok|FAILED|ignored` lines of one `cargo test --lib` run
// with the registry's filters, and the vitest json report of one run of the registry's files.
//
// Rules (exit 1 on any):
//   1. every registered suite ran and passed: each cargo filter selected at least one test and none
//      failed, each vitest file reported at least one passed test and none failed, and every test a
//      suite lists by name passed (ignored, skipped or absent is a failure);
//   2. a suite id registered at the merge base is still registered (removal needs ratchet-adr);
//   3. characterization first: a product-lane PR (feat/v60-l<1-9>-*) that changes a non-test file on
//      a surface needs a suite in the merge base's ci/ registries whose covers match that file.
//
// CLI:
//   node scripts/ci/lq/regression-suite.mjs list [--surface <id>] [--root <dir>]
//   node scripts/ci/lq/regression-suite.mjs plan [--surface <id>] --out <plan.json>
//   node scripts/ci/lq/regression-suite.mjs cargo-results --log <file> --exit <n> --out <file.json>
//   node scripts/ci/lq/regression-suite.mjs check --base <rev> --cargo-results <f> --vitest-json <f>
//        [--branch <name>] [--labels-json <json array>] [--surface <id>] [--root <dir>]
//   node scripts/ci/lq/regression-suite.mjs run [--surface <id>]   (local: runs both, then check)
// Exit codes: 0 pass, 1 refused, 2 usage or unreadable input.
import fs from "node:fs";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const SURFACES_FILE = ".agentile/coverage/characterization.json";
export const L0_FILE = "ci/characterization.json";
export const LANE_DIR = "ci/characterization.d";
const LANE_BRANCH = /^feat\/v60-l([1-9])-/; // product lanes L1..L9 (L0 is the integration lane)
const TEST_FILE = /(\.test\.[cm]?[jt]sx?|_tests?\.rs|\.snap)$|(^|\/)(characterization|fixtures|__snapshots__)\//;
const ADR_LABEL = "ratchet-adr";

// Text from a PR or a registry is printed with control characters replaced, so it can never start
// a line of its own and so never forms a workflow command.
const clean = (s) => String(s).replace(/[\u0000-\u001f\u007f]/g, "?");

class UsageError extends Error {}

// '**' (a whole segment) matches zero or more segments, a trailing '/' everything under it, '*'
// within one segment; anything else is literal.
export function matchGlob(glob, p) {
  if (glob.endsWith("/")) return p.startsWith(glob);
  const re = glob
    .split("/")
    .map((seg) => (seg === "**" ? "\u0000" : seg.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*")))
    .join("/")
    .replace(/\/\u0000$/, "(?:/.*)?")
    .replace(/^\u0000\//, "(?:.*/)?")
    .replace(/\/\u0000\//g, "/(?:.*/)?");
  return new RegExp(`^${re}$`).test(p);
}

export function fsReader(root) {
  return {
    read: (p) => {
      try {
        return fs.readFileSync(path.join(root, p), "utf8");
      } catch {
        return null;
      }
    },
    list: (dir) => {
      try {
        return fs.readdirSync(path.join(root, dir));
      } catch {
        return [];
      }
    },
  };
}

export function gitReader(root, rev) {
  const git = (args) => execFileSync("git", ["-C", root, ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  return {
    read: (p) => {
      try {
        return git(["show", `${rev}:${p}`]);
      } catch {
        return null;
      }
    },
    list: (dir) => {
      try {
        return git(["ls-tree", "--name-only", `${rev}:${dir}`]).split("\n").filter(Boolean);
      } catch {
        return [];
      }
    },
  };
}

const arr = (v) => (Array.isArray(v) ? v.map(String) : []);

// All suites (ci/ registries, then the surface map's suites as LQ:<surface>/<runner>) and surfaces.
export function loadRegistry(reader) {
  const errors = [];
  const suites = [];
  const parse = (file, required) => {
    const text = reader.read(file);
    if (text === null) {
      if (required) errors.push(`${file}: missing`);
      return null;
    }
    try {
      const doc = JSON.parse(text);
      if (doc?.version !== 1) throw new Error("version must be 1");
      return doc;
    } catch (e) {
      errors.push(`${file}: unreadable (${e.message})`);
      return null;
    }
  };
  const laneFiles = reader.list(LANE_DIR).filter((f) => f.endsWith(".json")).sort().map((f) => `${LANE_DIR}/${f}`);
  for (const file of [L0_FILE, ...laneFiles]) {
    const doc = parse(file, file === L0_FILE);
    if (!doc) continue;
    if (!Array.isArray(doc.suites)) errors.push(`${file}: suites must be an array`);
    for (const s of doc.suites || []) {
      suites.push({ id: String(s?.id ?? ""), lane: s?.lane, runner: s?.runner, package: s?.package, filters: s?.filter ? [String(s.filter)] : [], files: arr(s?.files), tests: arr(s?.tests), covers: arr(s?.covers), source: file, registered: true });
    }
  }
  const surfaceDoc = parse(SURFACES_FILE, true);
  const surfaces = [];
  for (const s of surfaceDoc?.surfaces || []) {
    const id = String(s?.surface ?? "");
    if (!id || !arr(s?.paths).length) errors.push(`${SURFACES_FILE}: surface ${clean(id || "(no id)")} needs surface and paths`);
    surfaces.push({ surface: id, owner_lane: s?.owner_lane, paths: arr(s?.paths) });
    const vitest = (s?.suites || []).filter((x) => x?.runner === "vitest").map((x) => String(x.file));
    if (vitest.length) suites.push({ id: `LQ:${id}/vitest`, runner: "vitest", files: vitest, filters: [], tests: [], covers: [], source: SURFACES_FILE });
    const byPkg = new Map();
    for (const x of (s?.suites || []).filter((x) => x?.runner === "cargo")) {
      const pkg = String(x.package || "citrate-core");
      byPkg.set(pkg, [...(byPkg.get(pkg) || []), String(x.filter)]);
    }
    for (const [pkg, filters] of byPkg) suites.push({ id: `LQ:${id}/cargo/${pkg}`, runner: "cargo", package: pkg, files: [], filters, tests: [], covers: [], source: SURFACES_FILE });
    for (const x of (s?.suites || []).filter((x) => !["vitest", "cargo"].includes(x?.runner))) errors.push(`${SURFACES_FILE}: ${clean(id)}: runner ${clean(x?.runner)} is not cargo or vitest`);
  }
  const seen = new Set();
  for (const s of suites) {
    const where = `${s.source}: ${clean(s.id || "(no id)")}`;
    if (!s.id) errors.push(`${where}: every suite needs an id`);
    if (seen.has(s.id)) errors.push(`${where}: registered twice`);
    seen.add(s.id);
    if (s.runner === "cargo" && (!s.package || !s.filters.length || s.filters.some((f) => !f || f === "undefined"))) errors.push(`${where}: a cargo suite needs package and filter`);
    else if (s.runner === "vitest" && !s.files.length) errors.push(`${where}: a vitest suite needs files`);
    else if (!["cargo", "vitest"].includes(s.runner)) errors.push(`${where}: runner must be cargo or vitest`);
    // A file or filter becomes a runner argument: never one that reads as an option.
    for (const x of [...s.files, ...s.filters]) if (!/^[\w@][\w@.:/-]*$/.test(x)) errors.push(`${where}: ${clean(x)} is not a plain path or test filter`);
    for (const f of s.files) if (reader.read(f) === null) errors.push(`${where}: ${clean(f)} does not exist`);
  }
  return { suites, surfaces, errors };
}

export function surfacesFor(p, surfaces) {
  return surfaces.filter((s) => s.paths.some((g) => matchGlob(g, p)));
}

export function selectSurface(reg, surface) {
  if (!surface) return reg.suites;
  if (!reg.surfaces.some((s) => s.surface === surface)) throw new UsageError(`unknown surface ${clean(surface)}`);
  const paths = reg.surfaces.find((s) => s.surface === surface).paths;
  return reg.suites.filter((s) => s.id.startsWith(`LQ:${surface}/`) || s.covers.some((c) => paths.some((g) => matchGlob(g, c) || matchGlob(c, g))));
}

// One cargo filter set and one vitest file list for the whole registry.
export function plan(suites) {
  const uniq = (xs) => [...new Set(xs)].sort();
  const cargo = suites.filter((s) => s.runner === "cargo");
  return {
    cargo: { packages: uniq(cargo.map((s) => s.package)), filters: uniq(cargo.flatMap((s) => s.filters)) },
    vitest: { files: uniq(suites.filter((s) => s.runner === "vitest").flatMap((s) => s.files)) },
  };
}

// Unit tests only (--lib): every registered cargo suite is a module test block inside its crate's
// library. A suite outside it selects nothing and fails loudly rather than passing.
export function cargoArgs(p) {
  return ["test", "--locked", "--offline", "--no-fail-fast", ...p.cargo.packages.flatMap((x) => ["-p", x]), "--lib", "--", ...p.cargo.filters];
}

export function parseCargoLog(text) {
  const out = {};
  for (const line of String(text).split(/\r?\n/)) {
    const m = line.match(/^test (\S+) \.\.\. (ok|FAILED|ignored)\b/);
    if (m) out[m[1]] = m[2];
  }
  return out;
}

const selects = (filter, name) => name.includes(filter);

export function verify({ suites, cargo, vitest, root }) {
  const results = [];
  const files = new Map();
  for (const f of vitest?.testResults || []) {
    const rel = path.isAbsolute(String(f.name)) ? path.relative(root, String(f.name)).split(path.sep).join("/") : String(f.name);
    files.set(rel, f.assertionResults || []);
  }
  for (const s of suites) {
    const problems = [];
    let ran = 0;
    if (s.runner === "cargo") {
      if (!cargo || typeof cargo.tests !== "object") problems.push("no cargo results");
      else {
        const names = Object.keys(cargo.tests);
        for (const f of s.filters) {
          const mine = names.filter((n) => selects(f, n));
          const passed = mine.filter((n) => cargo.tests[n] === "ok");
          ran += passed.length;
          if (!passed.length) problems.push(`filter ${clean(f)} selects no test that ran`);
          const failed = mine.filter((n) => cargo.tests[n] === "FAILED");
          if (failed.length) problems.push(`failed: ${failed.map(clean).join(", ")}`);
        }
        for (const t of s.tests) if (!names.some((n) => s.filters.some((f) => selects(f, n)) && (n === t || n.endsWith(`::${t}`)) && cargo.tests[n] === "ok")) problems.push(`listed test ${clean(t)} did not pass`);
        if (cargo.exit !== 0 && !problems.length) problems.push(`cargo test exited ${clean(cargo.exit)}`);
      }
    } else {
      if (!vitest || !Array.isArray(vitest.testResults)) problems.push("no vitest report");
      else {
        const all = [];
        for (const f of s.files) {
          // Keyed by repo-relative path; a report written under another checkout path still matches.
          const a = files.get(f) ?? [...files].find(([k]) => k.endsWith(`/${f}`))?.[1];
          if (!a) {
            problems.push(`${clean(f)} is not in the vitest report`);
            continue;
          }
          all.push(...a);
          const failed = a.filter((t) => t.status === "failed");
          if (failed.length) problems.push(`${clean(f)} failed: ${failed.map((t) => clean(t.fullName || t.title)).join("; ")}`);
          if (!a.some((t) => t.status === "passed")) problems.push(`${clean(f)} ran no test`);
          ran += a.filter((t) => t.status === "passed").length;
        }
        for (const t of s.tests) if (!all.some((x) => x.title === t && x.status === "passed")) problems.push(`listed test ${clean(t)} did not pass`);
      }
    }
    results.push({ id: s.id, ok: problems.length === 0, ran, reason: problems.join("; ") });
  }
  return results;
}

export function removedSuites(baseSuites, headSuites, labels) {
  if ((labels || []).includes(ADR_LABEL)) return [];
  const head = new Set(headSuites.map((s) => s.id));
  return baseSuites.filter((s) => s.registered && !head.has(s.id)).map((s) => `suite ${clean(s.id)} (${s.source}) is registered at the merge base and missing at the head; removing one needs label ${ADR_LABEL} and an ADR`);
}

export function gate({ changed, branch, base }) {
  const lane = String(branch || "").match(LANE_BRANCH);
  if (!lane) return [];
  const errors = [];
  for (const p of changed) {
    if (TEST_FILE.test(p) || p.startsWith(`${LANE_DIR}/`) || p === L0_FILE || p === SURFACES_FILE) continue;
    for (const s of surfacesFor(p, base.surfaces)) {
      const covered = base.suites.some((x) => x.registered && x.covers.some((g) => matchGlob(g, p)));
      if (!covered) errors.push(`${clean(p)} is on surface ${clean(s.surface)} (owner ${clean(s.owner_lane)}) and no registered suite at the merge base covers it: register characterization for ${clean(s.surface)} first (a characterization PR, then this one)`);
    }
  }
  return errors;
}

function args(argv, flags) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (!flags.includes(k) || i + 1 >= argv.length) throw new UsageError(`unknown or incomplete option ${clean(k)}`);
    o[k.slice(2).replace(/-(\w)/g, (_, c) => c.toUpperCase())] = argv[++i];
  }
  return o;
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (e) {
    throw new UsageError(`cannot read ${clean(file)}: ${clean(e.message)}`);
  }
}

function headRegistry(root, surface) {
  const reg = loadRegistry(fsReader(root));
  if (reg.errors.length) {
    for (const e of reg.errors) console.error(`::error title=lq / regression-suite::${clean(e)}`);
    throw Object.assign(new Error("registry invalid"), { code: 1 });
  }
  return { reg, suites: selectSurface(reg, surface) };
}

function check(o) {
  const root = path.resolve(o.root || ".");
  const { reg, suites } = headRegistry(root, o.surface);
  const errors = [];
  const cargo = o.cargoResults ? readJson(o.cargoResults) : null;
  const vitest = o.vitestJson ? readJson(o.vitestJson) : null;
  const results = verify({ suites, cargo, vitest, root });
  for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"} ${clean(r.id)} (${r.ran} passed)${r.ok ? "" : `: ${r.reason}`}`);
  for (const r of results.filter((r) => !r.ok)) errors.push(`${clean(r.id)}: ${r.reason}`);
  const labels = o.labelsJson ? JSON.parse(o.labelsJson) : [];
  if (!Array.isArray(labels)) throw new UsageError("--labels-json must be a JSON array");
  if (o.base) {
    const base = loadRegistry(gitReader(root, o.base));
    errors.push(...removedSuites(base.suites, reg.suites, labels.map(String)));
    const changed = execFileSync("git", ["-C", root, "diff", "--name-only", "--no-renames", o.base, "HEAD"], { encoding: "utf8" }).split("\n").filter(Boolean);
    errors.push(...gate({ changed, branch: o.branch, base }));
  }
  for (const e of errors) console.log(`::error title=lq / regression-suite::${e}`);
  console.log(`lq / regression-suite: ${results.length} suites, ${results.filter((r) => r.ok).length} green, ${errors.length} refusal(s)`);
  return errors.length ? 1 : 0;
}

function main(argv) {
  const [cmd, ...rest] = argv;
  if (cmd === "list" || cmd === "plan") {
    const o = args(rest, ["--surface", "--root", "--out"]);
    const { suites } = headRegistry(path.resolve(o.root || "."), o.surface);
    if (cmd === "list") for (const s of suites) console.log(`${s.id}\t${s.runner}\t${s.runner === "cargo" ? `${s.package} ${s.filters.join(" ")}` : s.files.join(" ")}`);
    else fs.writeFileSync(o.out || "/dev/stdout", JSON.stringify({ ...plan(suites), cargoArgs: cargoArgs(plan(suites)) }, null, 1) + "\n");
    return 0;
  }
  if (cmd === "cargo-results") {
    const o = args(rest, ["--log", "--exit", "--out"]);
    if (!o.log || !o.out || !/^\d+$/.test(o.exit ?? "")) throw new UsageError("cargo-results --log <file> --exit <n> --out <file>");
    const text = fs.existsSync(o.log) ? fs.readFileSync(o.log, "utf8") : "";
    fs.writeFileSync(o.out, JSON.stringify({ exit: Number(o.exit), tests: parseCargoLog(text) }) + "\n");
    return 0;
  }
  if (cmd === "check") return check(args(rest, ["--base", "--cargo-results", "--vitest-json", "--branch", "--labels-json", "--surface", "--root"]));
  if (cmd === "run") {
    const o = args(rest, ["--surface"]);
    const { suites } = headRegistry(process.cwd(), o.surface);
    const p = plan(suites);
    const tmp = fs.mkdtempSync(path.join(process.env.TMPDIR || "/tmp", "lq-regression-"));
    let cargoFile;
    if (p.cargo.filters.length) {
      const r = spawnSync("cargo", cargoArgs(p), { cwd: process.cwd(), encoding: "utf8", maxBuffer: 1 << 30 });
      process.stdout.write(r.stdout || "");
      fs.writeFileSync((cargoFile = path.join(tmp, "cargo.json")), JSON.stringify({ exit: r.status ?? 1, tests: parseCargoLog(r.stdout) }));
    }
    let vitestFile;
    if (p.vitest.files.length) {
      vitestFile = path.join(tmp, "vitest.json");
      spawnSync("npx", ["vitest", "run", "--reporter=default", "--reporter=json", `--outputFile.json=${vitestFile}`, ...p.vitest.files], { stdio: "inherit" });
    }
    return check({ cargoResults: cargoFile, vitestJson: vitestFile, surface: o.surface });
  }
  throw new UsageError("usage: regression-suite.mjs list|plan|cargo-results|check|run (see the header)");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (e) {
    console.error(`lq / regression-suite: ${clean(e.message)}`);
    process.exit(e instanceof UsageError ? 2 : e.code === 1 ? 1 : 2);
  }
}
