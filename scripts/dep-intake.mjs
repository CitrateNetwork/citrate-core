#!/usr/bin/env node
// L0-WP10 dependency intake: a package enters Cargo.lock or package-lock.json only with a reviewed
// row in release/dep-intake.json.
//
//   node scripts/dep-intake.mjs check [--repo <dir>] [--base <git rev>]
//
// With --base, every third-party package (a Cargo.lock entry with a source; a package-lock.json
// entry, named by its lock "name" when set (an npm alias) and else by its install path) in the
// working tree's lockfiles that the base's lockfiles lack must be listed in exactly one row's deps,
// as "cargo:<name>" or "npm:<name>". A package counts as new when its name is new or when it comes
// from a source origin the base did not use for that name (a crates.io crate swapped for a git
// fork; an npm tarball from another host or another registry package); a version bump on the same
// origin is not new (cargo audit and review cover it). Workspace and path crates (crates/*,
// src-tauri, kit, ...) and linked npm workspace packages are first party and need no row. Without
// --base nothing counts as new (scheduled runs), and the checks below still run:
//   - each row has lane (L0..L9, LQ), wp, reason, licence, reviewer, a supply_chain review
//     {skill: "supply-chain-risk-auditor", result: "pass" | "pass-with-notes", notes}, and origins:
//     the reviewed source origins, which must hold every origin its deps come from (so a reviewed
//     package later moved to a fork or another package fails until its row is reviewed again);
//   - each row names only packages the lockfiles hold (a removed dependency takes its row along);
//   - a CitrateNetwork git crate (any spelling: https, http, ssh, any case, with or without .git)
//     is pinned by a 40-hex rev=, all crates from one federation repo share one rev, that rev is the
//     federation pin when release/components.json records one for the repo (federation.rev), and
//     its row names the manifest [[drift]] entry (Rule 12). A federation repo no component records
//     has no pin in this repo; there the check proves one rev, not which rev (review covers it);
//   - every `cargo audit --ignore` in .github/workflows/ci.yml other than the frozen baseline below
//     has a comment line naming its id with "EXPIRES YYYY-MM-DD", and that date has not passed.
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
// A git source on github.com/CitrateNetwork, in any spelling cargo accepts; group 1 is the repo.
const FEDERATION = /^git\+(?:(?:https?|ssh|git):\/\/)?(?:[^@/?#]+@)?(?:www\.)?github\.com(?:(?::\d+)?\/|:)citratenetwork\/([^/?#]+?)(?:\.git)?\/?(?:[?#]|$)/i;
const HEX40 = /^[0-9a-f]{40}$/;

class Usage extends Error {}
const text = (v) => typeof v === "string" && v.trim() !== "";

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

/** The CitrateNetwork repo a Cargo.lock source comes from, lowercased and without .git; null for any other source. */
export function fedRepo(src) {
  return FEDERATION.exec(src)?.[1].toLowerCase() ?? null;
}

/**
 * Where a source comes from, without rev, commit or version: "git+https://host/repo.git", and for a
 * registry tarball the package path ("https://registry.npmjs.org/react", ".../@scope/name"). Any
 * other tarball URL is kept whole, so a new URL is a new origin.
 */
export function sourceOrigin(src) {
  if (/^https?:\/\//i.test(src)) {
    const u = new URL(src);
    const i = u.pathname.indexOf("/-/");
    return `${u.origin}${i < 0 ? u.pathname : u.pathname.slice(0, i)}`;
  }
  return src.split("#")[0].replace(/([?&])rev=[^&]*&?/, "$1").replace(/[?&]$/, "");
}

/** package-lock.json (v2/v3) packages: "npm:<name>" -> Set of resolved URLs. The name is the entry's own (an alias installs it under another path); a workspace link to a path inside the repo is first party and left out; any other link is a package. */
export function npmLockPackages(lockText) {
  let lock;
  try {
    lock = JSON.parse(lockText);
  } catch (e) {
    throw new Usage(`package-lock.json: ${e.message}`);
  }
  if (!lock?.packages || typeof lock.packages !== "object") throw new Usage("package-lock.json has no packages (lockfileVersion 2 or 3)");
  const pkgs = new Map();
  for (const [p, v] of Object.entries(lock.packages)) {
    const i = p.lastIndexOf("node_modules/");
    if (i < 0) continue;
    const local = typeof v?.resolved === "string" && !/^[a-z][a-z0-9+.-]*:|^[\\/]|(^|[\\/])\.\.([\\/]|$)/i.test(v.resolved);
    if (v?.link === true && local) continue;
    const key = `npm:${text(v?.name) ? v.name : p.slice(i + "node_modules/".length)}`;
    if (!pkgs.has(key)) pkgs.set(key, new Set());
    pkgs.get(key).add(v?.link === true ? `link:${v.resolved}` : typeof v?.resolved === "string" ? v.resolved : "unresolved");
  }
  return pkgs;
}

function auditErrors(ciYml, today) {
  const errors = [];
  const comments = ciYml.split("\n").filter((l) => l.trim().startsWith("#"));
  const ids = new Set([...ciYml.matchAll(/--ignore[=\s]+(RUSTSEC-\d{4}-\d{4})/g)].map((m) => m[1]));
  for (const id of [...ids].sort()) {
    if (AUDIT_BASELINE.has(id)) continue;
    const dates = comments.filter((l) => l.includes(id)).flatMap((l) => [...l.matchAll(/EXPIRES (\d{4}-\d{2}-\d{2})/g)].map((m) => m[1]));
    if (dates.length === 0) errors.push(`ci.yml: cargo audit --ignore ${id} needs a comment line naming it with an EXPIRES YYYY-MM-DD date`);
    else if (dates.every((d) => d < today)) errors.push(`ci.yml: cargo audit --ignore ${id} expiry lapsed (${dates.sort().at(-1)}); fix the advisory or renew it in review`);
  }
  return errors;
}

/** Federation pins from release/components.json: lowercased CitrateNetwork repo -> federation.rev. */
export function federationPins(componentsJson) {
  const pins = new Map();
  for (const c of Array.isArray(componentsJson?.components) ? componentsJson.components : []) {
    const repo = /^CitrateNetwork\/([^/]+)$/i.exec(c?.source?.repo ?? "")?.[1].toLowerCase();
    if (repo && HEX40.test(c?.federation?.rev ?? "")) pins.set(repo, c.federation.rev);
  }
  return pins;
}

/** Check an intake against base and head package sets. Returns a list of defects. */
export function checkIntake({ intake, base, head, ciYml, today, pins = new Map() }) {
  const errors = [];
  const rows = Array.isArray(intake?.rows) ? intake.rows : [];
  if (!Array.isArray(intake?.rows)) errors.push("release/dep-intake.json has no rows[]");
  const present = new Set([...head.cargo.keys(), ...head.npm.keys()]);
  const sourcesOf = (d) => head.cargo.get(d) ?? head.npm.get(d) ?? new Set();

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
      const origins = Array.isArray(r?.origins) ? r.origins : [];
      for (const o of [...new Set([...sourcesOf(d)].map(sourceOrigin))].sort()) {
        if (!origins.includes(o)) errors.push(`${d}: source ${o} is not in ${at} origins (the reviewed sources); a changed source needs the row reviewed again`);
      }
      const fed = [...(head.cargo.get(d) ?? [])].some((s) => fedRepo(s) !== null);
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
      const repo = fedRepo(s);
      if (repo === null) continue;
      const rev = /[?&]rev=([0-9a-f]{40})(#|&|$)/.exec(s)?.[1];
      if (!rev) {
        errors.push(`${name}: federation git source must pin a 40-hex rev= (the federation pin), not a branch or tag`);
        continue;
      }
      if (!revs.has(repo)) revs.set(repo, new Set());
      revs.get(repo).add(rev);
    }
  }
  for (const [repo, set] of revs) {
    if (set.size > 1) errors.push(`CitrateNetwork/${repo}: federation crates must stay on one rev (found ${[...set].sort().join(", ")})`);
    const pin = pins.get(repo);
    for (const rev of [...set].sort()) {
      if (pin && rev !== pin) errors.push(`CitrateNetwork/${repo}: crate rev ${rev} is not the federation pin ${pin} (release/components.json federation.rev; move both with the manifest, Rule 12)`);
    }
  }

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
  let pins;
  try {
    pins = federationPins(JSON.parse(read(path.join(repo, "release", "components.json"))));
  } catch (e) {
    throw e instanceof Usage ? e : new Usage(`release/components.json: ${e.message}`);
  }
  const errors = checkIntake({ intake, base, head, ciYml, pins, today: new Date().toISOString().slice(0, 10) });
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
