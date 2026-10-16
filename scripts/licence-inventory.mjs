#!/usr/bin/env node
// HUP gate g3-licence: check release/licences.json against what this repo actually ships.
//
//   node scripts/licence-inventory.mjs [--repo <dir>] [--inventory <file>] [--corpus <dir>]
//                                      [--json <out>] [--require-sign-off] [--require-notices]
//
// Required keys come from the repo itself, so a new sidecar, resource, tool, library or skills
// source cannot ship without a licence entry:
//   externalBin:<path>  resource:<path>  every src-tauri/tauri.bundle-*.conf.json
//   toolchain:<name>                     components/toolchain-bundle.json tools[]
//   library:<name>                       templates/deps.lock.json deps
//   skills:<label>                       skills.lock [[source]] labels
//   frontendDist:<path>                  src-tauri/tauri.conf.json build.frontendDist (the webview)
//   corpus:<id>                          a staged corpus manifest.json (--corpus), included sources
// Every key must be covered by exactly one entry; a cover that matches nothing is stale (except
// planned:<name>, and corpus:<id> when no corpus is given). Every licence text an entry names
// must exist, every file in src-tauri/licenses/ must be named by an entry, and every bundle
// config must ship `licenses/*`. A shipped copyleft entry must carry a source offer, and a
// third-party entry that ships with the app (installer, skills bundle, corpus text) must name at
// least one licence text under src-tauri/licenses/.
//
// third_party_notices (on a component or on app) points at the generated notices of the packages
// compiled into that program (scripts/third-party-notices.mjs): its file must exist under
// src-tauri/licenses/ and counts as named. --require-notices also fails while the app or a
// first-party component that ships a sidecar (an externalBin cover) or the webview bundle (a
// frontendDist cover) has none (release checklist).
//
// review states (ok / action / owner) are reported, not failed: they are the review's findings.
// --require-sign-off also fails while sign_off.status is not "signed" (for a release checklist).
//
// Exit 0 OK, 1 defects (or unsigned under --require-sign-off), 2 usage or unreadable input.
// Zero dependencies.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REVIEW = new Set(["ok", "action", "owner"]);
const SHIPS = new Set(["installer", "first-run-download", "skills-bundle", "corpus-text", "not-shipped"]);
const COPYLEFT = new Set(["none", "weak", "strong", "network"]);
const SIGN_OFF = new Set(["pending owner sign-off", "signed"]);
const LICENCE_DIR = "src-tauri/licenses";
const LICENCE_GLOB = "licenses/*";
const BUNDLED = new Set(["installer", "skills-bundle", "corpus-text"]);
const PER_SOURCE = "LicenseRef-per-source";

class Usage extends Error {}

/** "llama/*" -> "llama", "skills/**\/*" -> "skills", a plain file stays as is. */
export function resourceKey(glob) {
  return glob.replace(/(\/\*\*)?\/\*$/, "");
}

/** The labels of the [[source]] tables in skills.lock (the generated, strict TOML subset). */
export function skillsLockLabels(text) {
  const labels = [];
  let inSource = false;
  for (const line of text.split("\n")) {
    if (/^\[\[.*\]\]$/.test(line)) {
      inSource = line === "[[source]]";
      continue;
    }
    const m = /^label = "([^"]+)"$/.exec(line);
    if (inSource && m) labels.push(m[1]);
  }
  return labels;
}

/** Ids of the corpus sources a staged manifest actually includes. */
export function corpusSourceIds(manifest) {
  if (!manifest || !Array.isArray(manifest.sources)) throw new Usage("corpus manifest.json has no sources[]");
  return manifest.sources.filter((s) => s.included === true).map((s) => s.id);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (e) {
    throw new Usage(`cannot read ${file}: ${e.message}`);
  }
}

function bundleConfigs(repoRoot) {
  const dir = path.join(repoRoot, "src-tauri");
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((f) => /^tauri\.bundle-.+\.conf\.json$/.test(f))
    .sort()
    .map((f) => ({ name: f, json: readJson(path.join(dir, f)) }));
}

/** Every key the repo ships, mapped to where it came from. */
export function requiredKeys(repoRoot) {
  const keys = new Map();
  const add = (k, from) => {
    if (!keys.has(k)) keys.set(k, from);
  };
  for (const { name, json } of bundleConfigs(repoRoot)) {
    for (const b of json.bundle?.externalBin ?? []) add(`externalBin:${b}`, name);
    for (const r of json.bundle?.resources ?? []) add(`resource:${resourceKey(r)}`, name);
  }
  // The webview bundle Vite builds (the npm packages it imports ship inside it).
  const base = path.join(repoRoot, "src-tauri", "tauri.conf.json");
  if (fs.existsSync(base)) {
    const dist = readJson(base).build?.frontendDist;
    if (typeof dist === "string" && dist) add(`frontendDist:${dist}`, "tauri.conf.json");
  }
  const tb = path.join(repoRoot, "components", "toolchain-bundle.json");
  if (fs.existsSync(tb)) for (const t of readJson(tb).tools ?? []) add(`toolchain:${t.name}`, "components/toolchain-bundle.json");
  const dl = path.join(repoRoot, "templates", "deps.lock.json");
  if (fs.existsSync(dl)) for (const d of Object.keys(readJson(dl).deps ?? {})) add(`library:${d}`, "templates/deps.lock.json");
  const sl = path.join(repoRoot, "skills.lock");
  if (fs.existsSync(sl)) for (const l of skillsLockLabels(fs.readFileSync(sl, "utf8"))) add(`skills:${l}`, "skills.lock");
  return keys;
}

/**
 * Check an inventory. `corpusSources` (ids from a staged manifest) turns on the corpus check.
 * Returns { errors, summary }.
 */
export function checkInventory(inv, { repoRoot, corpusSources = null, requireNotices = false }) {
  const errors = [];
  const comps = Array.isArray(inv?.components) ? inv.components : [];
  if (!Array.isArray(inv?.components)) errors.push("inventory has no components[]");
  if (!SIGN_OFF.has(inv?.sign_off?.status)) errors.push(`sign_off.status must be one of: ${[...SIGN_OFF].join(", ")}`);

  const required = requiredKeys(repoRoot);
  if (corpusSources) for (const id of corpusSources) required.set(`corpus:${id}`, "corpus manifest.json");

  const coveredBy = new Map();
  const named = new Set();
  const ids = new Set();
  // Generated third-party notices: the file must exist in the bundled licence dir.
  const checkNotices = (owner, n) => {
    if (n == null) return false;
    const f = typeof n?.file === "string" ? path.normalize(n.file) : null;
    if (!f || !f.startsWith(path.normalize(LICENCE_DIR) + path.sep)) {
      errors.push(`${owner}: third_party_notices.file must be a file under ${LICENCE_DIR}/`);
      return false;
    }
    named.add(f);
    if (!fs.existsSync(path.join(repoRoot, f))) {
      errors.push(`${owner}: third-party notices ${n.file} do not exist (run scripts/third-party-notices.mjs collect and render)`);
      return false;
    }
    return true;
  };
  const appNotices = checkNotices("app", inv?.app?.third_party_notices);
  if (requireNotices && !appNotices) errors.push("app: no third_party_notices for the crates compiled into the app");
  for (const c of comps) {
    const id = c.id ?? "(no id)";
    if (!c.id || ids.has(c.id)) errors.push(`${id}: missing or duplicate id`);
    ids.add(c.id);
    if (typeof c.spdx !== "string" || !c.spdx.trim()) errors.push(`${id}: spdx is empty`);
    if (!REVIEW.has(c.review)) errors.push(`${id}: review must be one of ${[...REVIEW].join(", ")}`);
    if (!SHIPS.has(c.ships_as)) errors.push(`${id}: ships_as must be one of ${[...SHIPS].join(", ")}`);
    if (!COPYLEFT.has(c.copyleft)) errors.push(`${id}: copyleft must be one of ${[...COPYLEFT].join(", ")}`);
    if ((c.review === "action" || c.review === "owner") && !(typeof c.gap === "string" && c.gap.trim())) {
      errors.push(`${id}: review "${c.review}" needs a gap that says what is open`);
    }
    if ((c.copyleft === "strong" || c.copyleft === "network" || c.copyleft === "weak") && c.ships_as !== "not-shipped") {
      if (!(typeof c.source_offer === "string" && c.source_offer.trim())) {
        errors.push(`${id}: shipped ${c.copyleft} copyleft needs a source_offer`);
      }
    }
    if (typeof c.first_party !== "boolean") errors.push(`${id}: first_party must be true or false`);
    for (const t of c.licence_texts ?? []) {
      named.add(path.normalize(t));
      if (!fs.existsSync(path.join(repoRoot, t))) errors.push(`${id}: licence text ${t} does not exist`);
    }
    // Third-party code or text that ships with the app must carry its notice in the app: at least
    // one licence text under src-tauri/licenses/ (the `licenses/*` resource). First-run downloads
    // fetch from upstream, and per-source umbrellas defer to their per-source entries.
    if (c.first_party === false && BUNDLED.has(c.ships_as) && c.spdx !== PER_SOURCE) {
      const bundled = (c.licence_texts ?? []).some((t) => path.normalize(t).startsWith(path.normalize(LICENCE_DIR) + path.sep));
      if (!bundled) errors.push(`${id}: shipped third-party component names no licence text under ${LICENCE_DIR}/`);
    }
    const hasNotices = checkNotices(id, c.third_party_notices);
    const shipsSidecar = (c.covers ?? []).some((k) => k.startsWith("externalBin:"));
    if (requireNotices && c.first_party === true && c.ships_as === "installer" && shipsSidecar && !hasNotices) {
      errors.push(`${id}: no third_party_notices for the packages compiled into this sidecar`);
    }
    const shipsWebview = (c.covers ?? []).some((k) => k.startsWith("frontendDist:"));
    if (requireNotices && c.first_party === true && c.ships_as === "installer" && shipsWebview && !hasNotices) {
      errors.push(`${id}: no third_party_notices for the packages compiled into this bundle`);
    }
    if (!Array.isArray(c.covers) || c.covers.length === 0) errors.push(`${id}: covers is empty`);
    for (const k of c.covers ?? []) {
      if (coveredBy.has(k)) errors.push(`${k}: covered by ${coveredBy.get(k)} and ${id}`);
      else coveredBy.set(k, id);
    }
  }

  for (const [k, from] of required) {
    if (!coveredBy.has(k)) errors.push(`${k} (from ${from}): no licence entry`);
  }
  for (const [k, id] of coveredBy) {
    if (required.has(k) || k.startsWith("planned:")) continue;
    if (k.startsWith("corpus:")) {
      if (corpusSources) errors.push(`${k} (${id}): not in the corpus manifest`);
      continue;
    }
    errors.push(`${k} (${id}): not shipped by this repo (stale cover)`);
  }

  const licDir = path.join(repoRoot, LICENCE_DIR);
  if (fs.existsSync(licDir)) {
    for (const f of fs.readdirSync(licDir).sort()) {
      if (f === "README.md") continue;
      const rel = path.normalize(path.join(LICENCE_DIR, f));
      if (!named.has(rel)) errors.push(`${rel}: bundled licence text, but no entry names it`);
    }
  }
  for (const { name, json } of bundleConfigs(repoRoot)) {
    if (!(json.bundle?.resources ?? []).includes(LICENCE_GLOB)) {
      errors.push(`src-tauri/${name}: resources must include "${LICENCE_GLOB}" so the licence texts ship`);
    }
  }

  const count = (r) => comps.filter((c) => c.review === r).length;
  return {
    errors,
    summary: {
      components: comps.length,
      keys: required.size,
      ok: count("ok"),
      action: count("action"),
      owner: count("owner"),
      corpusChecked: Boolean(corpusSources),
      signOff: inv?.sign_off?.status ?? null,
    },
  };
}

function parseArgs(argv) {
  const a = { repo: process.cwd(), inventory: null, corpus: null, json: null, requireSignOff: false, requireNotices: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    const val = () => {
      const v = argv[++i];
      if (v === undefined || v.startsWith("--")) throw new Usage(`${k} needs a value`);
      return v;
    };
    if (k === "--repo") a.repo = path.resolve(val());
    else if (k === "--inventory") a.inventory = path.resolve(val());
    else if (k === "--corpus") a.corpus = path.resolve(val());
    else if (k === "--json") a.json = path.resolve(val());
    else if (k === "--require-sign-off") a.requireSignOff = true;
    else if (k === "--require-notices") a.requireNotices = true;
    else throw new Usage(`unknown argument ${k}`);
  }
  a.inventory ??= path.join(a.repo, "release", "licences.json");
  return a;
}

function main() {
  let args;
  let result;
  try {
    args = parseArgs(process.argv.slice(2));
    const inv = readJson(args.inventory);
    let corpusSources = null;
    if (args.corpus) corpusSources = corpusSourceIds(readJson(path.join(args.corpus, "manifest.json")));
    result = checkInventory(inv, { repoRoot: args.repo, corpusSources, requireNotices: args.requireNotices });
  } catch (e) {
    if (e instanceof Usage) {
      process.stderr.write(
        `licence-inventory: ${e.message}\nusage: node scripts/licence-inventory.mjs [--repo <dir>] [--inventory <file>] [--corpus <dir>] [--json <out>] [--require-sign-off] [--require-notices]\n`,
      );
      return 2;
    }
    throw e;
  }
  const { errors, summary } = result;
  if (args.json) fs.writeFileSync(args.json, JSON.stringify(result, null, 2) + "\n");
  for (const e of errors) process.stderr.write(`licence-inventory: ${e}\n`);
  const unsigned = args.requireSignOff && summary.signOff !== "signed";
  if (unsigned) process.stderr.write(`licence-inventory: sign-off is "${summary.signOff}", not "signed"\n`);
  const verdict = errors.length ? `FAIL (${errors.length} defect(s))` : unsigned ? "FAIL (unsigned)" : "OK";
  process.stdout.write(
    `licence inventory: ${verdict}\n` +
      `  ${summary.components} entries, ${summary.keys} shipped keys, corpus ${summary.corpusChecked ? "checked" : "not checked (pass --corpus)"}\n` +
      `  review ok: ${summary.ok}, actions open: ${summary.action}, owner decisions open: ${summary.owner}\n` +
      `  sign-off: ${summary.signOff}\n`,
  );
  return errors.length || unsigned ? 1 : 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
