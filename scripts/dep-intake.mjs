#!/usr/bin/env node
// L0-WP10 dependency intake: a package enters Cargo.lock or package-lock.json only with a reviewed
// row in release/dep-intake.json.
//
//   node scripts/dep-intake.mjs check [--repo <dir>] [--base <git rev>]
//
// With --base, every third-party package (a Cargo.lock entry with a source, a package-lock.json
// install path) in the working tree's lockfiles that the base's lockfiles lack must be listed in
// exactly one row's deps, as "cargo:<name>" or "npm:<name>". A package counts as new when its name
// is new or when it comes from a source the base did not use for that name (a crates.io crate
// swapped for a git fork, an npm package resolved from a new host); a version bump on the same
// source is not new (cargo audit and review cover it). Workspace and path crates (crates/*,
// src-tauri, kit, ...) are first party and need no row. Without --base nothing counts as new
// (scheduled runs), and the checks below still run:
//   - each row has lane (L0..L9, LQ), wp, reason, licence, reviewer and a supply_chain review
//     {skill: "supply-chain-risk-auditor", result: "pass" | "pass-with-notes", notes};
//   - each row names only packages the lockfiles hold (a removed dependency takes its row along);
//   - a CitrateNetwork git crate is pinned by rev=, all crates from one federation repo share one
//     rev (the federation pin), and its row names the manifest [[drift]] entry (Rule 12);
//   - every `cargo audit --ignore` in .github/workflows/ci.yml other than the frozen baseline below
//     sits in a comment block carrying "EXPIRES YYYY-MM-DD", and that date has not passed.
//
// Exit 0 OK, 1 defects, 2 usage or unreadable input. Zero dependencies.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Ignores already in ci.yml when the ratchet started (2026-11-27). Only removals change this list.
const AUDIT_BASELINE = new Set(["RUSTSEC-2025-0055", "RUSTSEC-2026-0367", "RUSTSEC-2026-0368", "RUSTSEC-2026-0369", "RUSTSEC-2026-0429"]);
const LANE = /^(L[0-9]|LQ)$/;
const DEP = /^(cargo|npm):\S+$/;
const RESULTS = new Set(["pass", "pass-with-notes"]);
const FEDERATION = "git+https://github.com/CitrateNetwork/";

class Usage extends Error {}

/** Third-party Cargo.lock packages: "cargo:<name>" -> Set of sources. */
export function cargoLockPackages(text) {
  const pkgs = new Map();
  for (const block of text.split(/^\[\[package\]\]$/m).slice(1)) {
    const name = /^name = "([^"]+)"$/m.exec(block)?.[1];
    const source = /^source = "([^"]+)"$/m.exec(block)?.[1];
    if (!name) throw new Usage("Cargo.lock: a [[package]] has no name");
    if (!source) continue;
    const key = `cargo:${name}`;
    if (!pkgs.has(key)) pkgs.set(key, new Set());
    pkgs.get(key).add(source);
  }
  return pkgs;
}

/** Where a source comes from, without rev, commit or file: "git+https://host/repo.git", "https://registry.npmjs.org". */
export function sourceOrigin(src) {
  if (/^https?:\/\//.test(src)) return new URL(src).origin;
  return src.split("#")[0].replace(/\?rev=[^&]*&?/, "?").replace(/\?$/, "");
}

/** package-lock.json (v2/v3) packages: "npm:<name>" -> Set of resolved URLs ("link" for a workspace link). */
export function npmLockPackages(text) {
  let lock;
  try {
    lock = JSON.parse(text);
  } catch (e) {
    throw new Usage(`package-lock.json: ${e.message}`);
  }
  if (!lock?.packages || typeof lock.packages !== "object") throw new Usage("package-lock.json has no packages (lockfileVersion 2 or 3)");
  const pkgs = new Map();
  for (const [p, v] of Object.entries(lock.packages)) {
    const i = p.lastIndexOf("node_modules/");
    if (i < 0) continue;
    const key = `npm:${p.slice(i + "node_modules/".length)}`;
    if (!pkgs.has(key)) pkgs.set(key, new Set());
    pkgs.get(key).add(v?.link ? "link" : typeof v?.resolved === "string" ? v.resolved : "unresolved");
  }
  return pkgs;
}

const text = (v) => typeof v === "string" && v.trim() !== "";

function auditErrors(ciYml, today) {
  const errors = [];
  const blocks = [];
  let cur = null;
  for (const line of ciYml.split("\n")) {
    if (line.trim().startsWith("#")) (cur ??= blocks[blocks.push([]) - 1]).push(line);
    else cur = null;
  }
  const ids = new Set([...ciYml.matchAll(/--ignore[=\s]+(RUSTSEC-\d{4}-\d{4})/g)].map((m) => m[1]));
  for (const id of [...ids].sort()) {
    if (AUDIT_BASELINE.has(id)) continue;
    const dates = blocks.filter((b) => b.some((l) => l.includes(id))).flatMap((b) => [...b.join("\n").matchAll(/EXPIRES (\d{4}-\d{2}-\d{2})/g)].map((m) => m[1]));
    if (dates.length === 0) errors.push(`ci.yml: cargo audit --ignore ${id} needs a comment naming it with an EXPIRES YYYY-MM-DD date`);
    else if (dates.every((d) => d < today)) errors.push(`ci.yml: cargo audit --ignore ${id} expiry lapsed (${dates.sort().at(-1)}); fix the advisory or renew it in review`);
  }
  return errors;
}

/** Check an intake against base and head package sets. Returns a list of defects. */
export function checkIntake({ intake, base, head, ciYml, today }) {
  const errors = [];
  const rows = Array.isArray(intake?.rows) ? intake.rows : [];
  if (!Array.isArray(intake?.rows)) errors.push("release/dep-intake.json has no rows[]");
  const present = new Set([...head.cargo.keys(), ...head.npm.keys()]);
  const fedRepo = (src) => src.slice(FEDERATION.length).split(/[?#]/)[0];

  const listed = new Map();
  rows.forEach((r, i) => {
    const at = `release/dep-intake.json rows[${i}]`;
    const deps = Array.isArray(r?.deps) ? r.deps : [];
    if (deps.length === 0 || !deps.every((d) => DEP.test(d))) errors.push(`${at}: deps must be a non-empty list of "cargo:<name>" or "npm:<name>"`);
    if (!LANE.test(r?.lane ?? "")) errors.push(`${at}: lane must be L0..L9 or LQ`);
    for (const f of ["wp", "reason", "licence", "reviewer"]) if (!text(r?.[f])) errors.push(`${at}: ${f} is required`);
    const sc = r?.supply_chain;
    if (sc?.skill !== "supply-chain-risk-auditor" || !RESULTS.has(sc?.result) || !text(sc?.notes)) {
      errors.push(`${at}: supply_chain must be {skill: "supply-chain-risk-auditor", result: "pass" or "pass-with-notes", notes}`);
    }
    for (const d of deps) {
      if (listed.has(d)) errors.push(`${d}: listed in more than one row`);
      listed.set(d, r);
      if (!present.has(d)) errors.push(`${d}: ${at} names a package not in Cargo.lock or package-lock.json`);
      const fed = [...(head.cargo.get(d) ?? [])].some((s) => s.startsWith(FEDERATION));
      if (fed && !text(r?.drift)) errors.push(`${d}: a federation crate needs drift naming its manifest.toml [[drift]] entry (Rule 12)`);
    }
  });

  // New name, or a source origin the base never used for that name.
  const added = (h, b) =>
    [...h]
      .flatMap(([k, srcs]) => {
        const was = new Set([...(b.get(k) ?? [])].map(sourceOrigin));
        const fresh = [...new Set([...srcs].map(sourceOrigin))].filter((o) => !was.has(o));
        return !b.has(k) ? [[k, ""]] : fresh.map((o) => [k, ` (new source ${o})`]);
      })
      .sort(([a], [c]) => (a < c ? -1 : a > c ? 1 : 0));
  for (const [d, why] of [...added(head.cargo, base.cargo), ...added(head.npm, base.npm)]) {
    if (!listed.has(d)) errors.push(`${d}: new in this change${why} and has no row in release/dep-intake.json`);
  }

  const revs = new Map();
  for (const [name, sources] of head.cargo) {
    for (const s of sources) {
      if (!s.startsWith(FEDERATION)) continue;
      const rev = /[?&]rev=([0-9a-f]{40})(#|&|$)/.exec(s)?.[1];
      if (!rev) {
        errors.push(`${name}: federation git source must pin a 40-hex rev= (the federation pin), not a branch or tag`);
        continue;
      }
      const repo = fedRepo(s);
      if (!revs.has(repo)) revs.set(repo, new Set());
      revs.get(repo).add(rev);
    }
  }
  for (const [repo, set] of revs) if (set.size > 1) errors.push(`CitrateNetwork/${repo}: federation crates must stay on one rev (found ${[...set].sort().join(", ")})`);

  return [...errors, ...auditErrors(ciYml, today)];
}

function read(file) {
  try {
    return fs.readFileSync(file, "utf8");
  } catch (e) {
    throw new Usage(`cannot read ${file}: ${e.message}`);
  }
}

/** A file at a git rev; "" when the rev lacks it. */
function atRev(repo, rev, file) {
  try {
    return execFileSync("git", ["-C", repo, "show", `${rev}:${file}`], { encoding: "utf8", maxBuffer: 64 << 20, stdio: ["ignore", "pipe", "pipe"] });
  } catch (e) {
    if (/does not exist|exists on disk, but not in/.test(String(e.stderr))) return "";
    throw new Usage(`git show ${rev}:${file}: ${String(e.stderr).trim() || e.message}`);
  }
}

function main(argv) {
  const [cmd, ...rest] = argv;
  if (cmd !== "check") throw new Usage("usage: dep-intake.mjs check [--repo <dir>] [--base <git rev>]");
  const opt = { repo: path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."), base: null };
  for (let i = 0; i < rest.length; i += 2) {
    const k = rest[i].replace(/^--/, "");
    if (!(k in opt) || rest[i + 1] === undefined) throw new Usage(`unknown or incomplete option ${rest[i]}`);
    opt[k] = rest[i + 1];
  }
  // git show reads a leading "-" as an option (for example --output=<file>).
  if (opt.base !== null && !/^[0-9A-Za-z_][0-9A-Za-z_./~^@{}-]*$/.test(opt.base)) throw new Usage(`--base must be a git revision, got ${opt.base}`);
  const repo = path.resolve(opt.repo);
  const head = { cargo: cargoLockPackages(read(path.join(repo, "Cargo.lock"))), npm: npmLockPackages(read(path.join(repo, "package-lock.json"))) };
  const base = opt.base
    ? { cargo: cargoLockPackages(atRev(repo, opt.base, "Cargo.lock")), npm: npmLockPackages(atRev(repo, opt.base, "package-lock.json") || '{"packages":{}}') }
    : head;
  let intake;
  try {
    intake = JSON.parse(read(path.join(repo, "release", "dep-intake.json")));
  } catch (e) {
    throw e instanceof Usage ? e : new Usage(`release/dep-intake.json: ${e.message}`);
  }
  const ciYml = read(path.join(repo, ".github", "workflows", "ci.yml"));
  const errors = checkIntake({ intake, base, head, ciYml, today: new Date().toISOString().slice(0, 10) });
  for (const e of errors) console.error(`dep-intake: ${e}`);
  if (errors.length === 0) console.log(`dep-intake: ok (${head.cargo.size} crates, ${head.npm.size} npm packages, ${intake.rows.length} intake rows${opt.base ? `, base ${opt.base}` : ""})`);
  return errors.length === 0 ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(`dep-intake: ${e.message}`);
    process.exitCode = e instanceof Usage ? 2 : 1;
  }
}
