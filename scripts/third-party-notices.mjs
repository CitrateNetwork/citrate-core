#!/usr/bin/env node
// =====================================================================
// citrate-core: third-party notices for the code compiled into what the installer ships
// (HUP gate g3-licence, release step)
//
//   node scripts/third-party-notices.mjs collect [--federation-root ..] [--only <component>]
//        [--target <triple>] [--work <dir>]  run cargo-about / go-licenses (once per release target),
//                                            keep their raw output
//   node scripts/third-party-notices.mjs render [--work <dir>]
//                                            normalise, write the notices file, update release/licences.json
//   node scripts/third-party-notices.mjs check
//                                            the committed notices file matches release/notices.json,
//                                            and the app and webview sections match Cargo.lock and
//                                            package-lock.json (re-render after a lockfile change)
//
// The licence inventory (release/licences.json) covers each sidecar as one first-party component,
// but each binary also contains hundreds of third-party Rust crates or Go modules whose licences
// (MIT, Apache-2.0, BSD, ...) ask for their notice to travel with the binary. This script
// generates that notice from the sources the sidecars are built from, as configured in
// release/notices.json:
//
//   rust: `cargo about generate --format json` (config release/about.toml) on each sidecar crate
//         (and the app itself) at the federation checkout, once for every target triple an
//         installer ships (release/notices.json targets: macOS, Linux and Windows pull in different
//         crates, for example GTK on Linux and windows-sys / webview2-com on Windows);
//   go:   `go-licenses report` on Kubo's `cmd/ipfs` at the pinned version, once per platform (one
//         GOOS/GOARCH per target), plus the Go standard library (compiled into every Go binary);
//   npm:  a Vite build of the webview (frontendDist) with write off, recording the npm packages
//         whose modules (after tree shaking) or assets (fonts) end up in the output, with each
//         package's licence file from node_modules (run `npm ci` first).
//
// The scans of one component are merged: the union of its packages, each listed once with the
// targets it is on when that is not every target. The header lists the targets covered.
//
// First-party packages (path crates and Citrate git dependencies, by source) are left out: the
// inventory entry covers them. Every third-party package must resolve to a licence in PERMISSIVE
// (else refused, so a copyleft or unknown licence is a reviewed change, not a silent one); Go
// modules the scanner cannot classify are resolved by the `clarify` table in release/notices.json.
// A crate offered under a choice that includes a licence outside PERMISSIVE (for example ittapi,
// GPL-2.0-only OR BSD-3-Clause) needs an `elect` entry naming the permissive licence it is used
// under; the notices print that choice next to the expression.
//
// Output (committed, bundled through the `licenses/*` resource): src-tauri/licenses/
// THIRD-PARTY-NOTICES.txt, with every distinct licence text once (deduplicated across components
// after whitespace normalisation) and, per component, every package with its licence and text ids.
// `render` also records, on each covered component in release/licences.json, the notices file and
// the source revision it was generated from (`third_party_notices`). The file is deterministic:
// the same inputs give the same bytes. Exit 0 ok, 1 refused or stale, 2 usage or a tool failed.
// =====================================================================
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(here, "..");
export const CONFIG = "release/notices.json";
const USAGE = [
  "usage: node scripts/third-party-notices.mjs collect [--federation-root <dir>] [--only <component>] [--target <triple>] [--work <dir>]",
  "       node scripts/third-party-notices.mjs render [--work <dir>]",
  "       node scripts/third-party-notices.mjs check",
].join("\n");

/** Licences a third-party package may ship under without a review (SPDX ids). */
export const PERMISSIVE = new Set([
  "MIT",
  "MIT-0",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "CC0-1.0",
  "0BSD",
  "BSL-1.0",
  "CDLA-Permissive-2.0",
  "OpenSSL",
  "Unlicense",
  // Weak, file-level copyleft: unmodified MPL files may ship in a larger work with their notice
  // and a pointer to their source (the upstream URL is listed with the package).
  "MPL-2.0",
]);

export const sha256 = (s) => createHash("sha256").update(s).digest("hex");

/** Hash of a licence text with whitespace runs collapsed, so re-wrapped copies of one text dedupe. */
export const textKey = (text) => sha256(text.replace(/\s+/g, " ").trim());

const COPYRIGHT_LINE = /^\s*(copyright\b|\(c\)|©|all rights reserved)/i;
const TITLE_LINE = /^\s*(the\s+)?[\w .,-]{0,40}\blicen[cs]e\b[\w .,()-]{0,40}$/i;

/**
 * Split the copyright lines off the top of a licence text: { copyright, body }. Only the leading
 * block is examined (blank lines, a title such as "MIT License", copyright and "All rights
 * reserved" lines); the first other line starts the body, and the body keeps the title. Most
 * MIT, BSD and ISC texts differ only in that block, so packages share one body and each keeps its
 * own copyright lines, which the notices print above the shared body.
 */
export function splitCopyright(text) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const copyright = [];
  const body = [];
  let inHeader = true;
  for (const line of lines) {
    if (inHeader && COPYRIGHT_LINE.test(line)) {
      copyright.push(line.trim());
      continue;
    }
    if (inHeader && line.trim() !== "" && !TITLE_LINE.test(line)) inHeader = false;
    body.push(line);
  }
  // A text that is nothing but copyright lines stays whole (an empty body would dedupe unrelated texts).
  if (!body.join("").trim()) return { copyright: "", body: lines.join("\n") };
  return { copyright: copyright.join("\n"), body: body.join("\n") };
}

/**
 * Add one package's use of a licence text to `texts` (key -> { id, body, notices }) and return the
 * key. `notices` maps a copyright block to the packages it applies to.
 */
function addText(texts, id, text, pkgLabel) {
  const { copyright, body } = splitCopyright(text);
  const key = textKey(body);
  texts[key] ??= { id, body, notices: {} };
  if (copyright) {
    const list = (texts[key].notices[copyright] ??= []);
    if (!list.includes(pkgLabel)) list.push(pkgLabel);
  }
  return key;
}

class Refused extends Error {}
class Usage extends Error {}

/** Read and minimally validate release/notices.json. */
export function readConfig(repoRoot = REPO) {
  const file = path.join(repoRoot, CONFIG);
  let cfg;
  try {
    cfg = JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (e) {
    throw new Usage(`cannot read ${file}: ${e.message}`);
  }
  if (typeof cfg.output !== "string" || !cfg.output.startsWith("src-tauri/licenses/")) {
    throw new Usage(`${CONFIG}: output must be a file under src-tauri/licenses/`);
  }
  if (!Array.isArray(cfg.targets) || cfg.targets.length === 0 || cfg.targets.some((t) => typeof t !== "string" || !t)) {
    throw new Usage(`${CONFIG}: targets (the Rust target triples the installers ship) is required`);
  }
  if (new Set(cfg.targets).size !== cfg.targets.length) throw new Usage(`${CONFIG}: duplicate target in targets`);
  for (const g of cfg.go ?? []) {
    const got = (g.platforms ?? []).map((p) => p.target);
    if (JSON.stringify([...got].sort()) !== JSON.stringify([...cfg.targets].sort()) || (g.platforms ?? []).some((p) => !p.goos || !p.goarch)) {
      throw new Usage(`${CONFIG}: go ${g.component} platforms must give goos/goarch for exactly the targets [${cfg.targets.join(", ")}]`);
    }
  }
  const ids = new Set();
  for (const c of [...(cfg.rust ?? []), ...(cfg.go ?? []), ...(cfg.npm ?? [])]) {
    if (!c.component || ids.has(c.component)) throw new Usage(`${CONFIG}: missing or duplicate component ${c.component}`);
    ids.add(c.component);
  }
  return cfg;
}

/** True when a package source is first party (a path crate, or matches a first_party_sources prefix). */
export function isFirstParty(source, prefixes) {
  if (source == null) return true;
  return prefixes.some((p) => source.startsWith(p));
}

/** The licence ids of an SPDX expression, "X WITH exception" kept whole ("(MIT OR Apache-2.0) AND ISC" -> 3 ids). */
export function licenceIds(expr) {
  return String(expr)
    .replace(/[()]/g, " ")
    .split(/\s+(?:OR|AND)\s+/)
    .map((x) => x.trim().replace(/\s+/g, " "))
    .filter(Boolean);
}

/**
 * The licence a crate is used under when its expression offers a licence outside PERMISSIVE:
 * null when every alternative is permissive; else the `elect` entry, which must be one of the
 * alternatives of a plain choice (no AND), on PERMISSIVE, and the only licence cargo-about
 * resolved for the crate. Refuses otherwise, so a copyleft choice is a reviewed change.
 */
export function electedLicence(label, name, expr, resolvedIds, elect = {}) {
  const ids = licenceIds(expr);
  const outside = ids.filter((id) => !PERMISSIVE.has(id));
  if (outside.length === 0) return null;
  const e = elect[name];
  if (!e) throw new Refused(`${label}: offered under ${expr}, which includes ${outside.join(", ")}; add the licence it is used under to elect in ${CONFIG}`);
  if (/\bAND\b/.test(expr) || !ids.includes(e) || !PERMISSIVE.has(e)) {
    throw new Refused(`${label}: elect ${e} is not a permissive alternative of ${expr}`);
  }
  if (resolvedIds.some((id) => id !== e)) throw new Refused(`${label}: elected ${e}, but cargo-about resolved ${[...new Set(resolvedIds)].join(", ")}`);
  return e;
}

/**
 * Normalise cargo-about JSON into { packages, texts }. `packages` are third-party crates
 * [{ name, version, licence, texts: [key], url }]; `texts` maps key -> { id, text }.
 * Refuses a third-party crate whose licence texts are missing or outside PERMISSIVE, or whose
 * licence choice includes a non-permissive licence without an election (`elect`).
 */
export function normaliseCargoAbout(json, { firstPartySources = [], elect = {} } = {}) {
  if (!json || !Array.isArray(json.crates) || !Array.isArray(json.licenses)) throw new Refused("cargo-about output has no crates[] / licenses[]");
  const textsByCrate = new Map();
  for (const l of json.licenses) {
    if (typeof l.text !== "string" || !l.text.trim()) continue;
    for (const u of l.used_by ?? []) {
      const id = u.crate?.id;
      if (!id) continue;
      if (!textsByCrate.has(id)) textsByCrate.set(id, []);
      textsByCrate.get(id).push({ id: l.id, text: l.text });
    }
  }
  const packages = [];
  const texts = {};
  for (const c of json.crates) {
    const p = c.package ?? {};
    if (isFirstParty(p.source ?? null, firstPartySources)) continue;
    const got = textsByCrate.get(p.id) ?? [];
    if (got.length === 0) throw new Refused(`${p.name} ${p.version}: no licence text resolved (licence ${c.license})`);
    for (const g of got) {
      if (!PERMISSIVE.has(g.id)) throw new Refused(`${p.name} ${p.version}: ships under ${g.id}, which is not on the permissive list (needs a licence review)`);
    }
    const label = `${p.name} ${p.version}`;
    const expr = String(c.license ?? "").replace(/\s+/g, " ").trim();
    const resolved = got.map((g) => g.id);
    let licence;
    if (expr === "" || expr === "Unknown" || expr === "NOASSERTION") {
      // No SPDX expression in Cargo.toml (only `license-file`): cargo-about identified the file's text.
      licence = `${[...new Set(resolved)].sort().join(" AND ")} (identified from its licence file; Cargo.toml names no SPDX licence)`;
    } else {
      const elected = electedLicence(label, p.name, expr, resolved, elect);
      licence = elected ? `${expr} (used under ${elected})` : expr;
    }
    packages.push({
      name: p.name,
      version: p.version,
      licence,
      texts: [...new Set(got.map((g) => addText(texts, g.id, g.text, label)))].sort(),
      url: p.repository ?? (p.source?.startsWith("registry+") ? `https://crates.io/crates/${p.name}/${p.version}` : null),
    });
  }
  return { packages: sortPackages(packages), texts };
}

/**
 * Normalise a go-licenses report (one line per package: name, version, licence, licence path, URL,
 * tab separated) into { packages, texts }. Packages of one module collapse to the module. `clarify`
 * maps a module path to { spdx, file } (file relative to the module dir, or "GOROOT/LICENSE") for
 * what the scanner reports as Unknown. `readText(path)` reads a licence file.
 */
export function normaliseGoLicenses(tsv, { mainModule, clarify = {}, readText, goroot, moduleDir }) {
  const byModule = new Map();
  for (const line of tsv.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const [name, version, licence, licencePath, url] = line.split("\t");
    if (!name || name === mainModule || name.startsWith(`${mainModule}/`)) continue;
    const mod = Object.keys(clarify).find((m) => name === m || name.startsWith(`${m}/`));
    let spdx = licence;
    let file = licencePath;
    if (mod) {
      spdx = clarify[mod].spdx;
      file = clarify[mod].file === "GOROOT/LICENSE" ? path.join(goroot, "LICENSE") : path.join(moduleDir(mod, version), clarify[mod].file);
    }
    if (!spdx || spdx === "Unknown" || !file || file === "Unknown") throw new Refused(`${name} ${version}: licence unknown; add it to clarify in ${CONFIG}`);
    if (!PERMISSIVE.has(spdx)) throw new Refused(`${name} ${version}: ships under ${spdx}, which is not on the permissive list (needs a licence review)`);
    const key = mod ?? moduleOf(name, licencePath);
    if (!byModule.has(key)) byModule.set(key, { name: key, version, licence: spdx, file, url: url && url !== "Unknown" ? url : null });
  }
  const texts = {};
  const packages = [];
  for (const m of byModule.values()) {
    const k = addText(texts, m.licence, readText(m.file), `${m.name} ${m.version}`);
    packages.push({ name: m.name, version: m.version, licence: m.licence, texts: [k], url: m.url });
  }
  return { packages: sortPackages(packages), texts };
}

/** Module path of a package, from its licence file path in the module cache (".../<module>@<version>/LICENSE"). */
function moduleOf(pkg, licencePath) {
  const m = /\/pkg\/mod\/(.+?)@[^/]+\//.exec(licencePath ?? "");
  if (!m) return pkg;
  // The cache escapes upper case as "!x".
  return m[1].replace(/!([a-z])/g, (_, c) => c.toUpperCase());
}

/** The package root ("…/node_modules/<name>" or "…/node_modules/@scope/<name>", innermost) of a bundled module id or asset path; null for app code. */
export function packageRootOf(id) {
  const i = id.lastIndexOf("node_modules/");
  if (i < 0) return null;
  const rest = id.slice(i + "node_modules/".length).split("/");
  const n = rest[0]?.startsWith("@") ? 2 : 1;
  if (rest.length <= n) return null;
  return id.slice(0, i) + "node_modules/" + rest.slice(0, n).join("/");
}

/** SPDX ids of an npm `license` expression ("(MIT AND BSD-3-Clause)", "MIT OR Apache-2.0"). */
const spdxIds = (expr) =>
  String(expr)
    .replace(/[()]/g, " ")
    .split(/\s+(?:OR|AND)\s+|\s+/)
    .map((x) => x.trim())
    .filter(Boolean);

/**
 * Normalise a collected npm component into { packages, texts }. Every SPDX id of a package's
 * licence must be on PERMISSIVE or on the component's `allow` list (reviewed per component, for
 * example bundled fonts under OFL-1.1). A package with no licence file of its own needs a
 * `clarify` entry naming the files (collected into raw.clarified) that carry its licence.
 */
export function normaliseNpm(raw, entry) {
  const allow = new Set(entry.allow ?? []);
  const clarify = entry.clarify ?? {};
  const texts = {};
  const packages = [];
  for (const p of raw.packages ?? []) {
    const label = `${p.name} ${p.version}`;
    const c = clarify[p.name];
    const licence = c?.spdx ?? (typeof p.license === "string" ? p.license : null);
    if (!licence) throw new Refused(`${label}: no licence in package.json; add it to clarify in ${CONFIG}`);
    for (const l of spdxIds(licence)) {
      if (!PERMISSIVE.has(l) && !allow.has(l)) throw new Refused(`${label}: ships under ${l}, which is not on the permissive list (needs a licence review)`);
    }
    let files = Object.entries(p.files ?? {});
    if (c?.files?.length) {
      files = c.files.map((f) => {
        if (!(f in (raw.clarified ?? {}))) throw new Refused(`${label}: clarified licence file ${f} was not collected; re-run collect`);
        return [f, raw.clarified[f]];
      });
    }
    if (files.length === 0) throw new Refused(`${label}: no licence file in the package; add it to clarify in ${CONFIG}`);
    const keys = files.map(([, text]) => addText(texts, spdxIds(licence)[0], text, label));
    packages.push({ name: p.name, version: p.version, licence, texts: [...new Set(keys)].sort(), url: p.url ?? null });
  }
  return { packages: sortPackages(packages), texts };
}

const sortPackages = (ps) => ps.sort((a, b) => (a.name === b.name ? a.version.localeCompare(b.version) : a.name < b.name ? -1 : 1));

/**
 * Merge the per-target scans of one component ([{ target, platform, component, source, tool,
 * packages, texts }], tool without the target) into one normalised component: the union of the
 * packages (by name and version; each keeps the targets it is on) and of the texts. The scans must
 * come from one source revision and one tool, and cover exactly `targets`.
 */
export function mergeTargets(scans, targets) {
  if (scans.length === 0) throw new Refused("nothing to merge");
  const { component, source, tool } = scans[0];
  const got = scans.map((s) => s.target);
  if (JSON.stringify([...got].sort()) !== JSON.stringify([...targets].sort())) {
    throw new Refused(`${component}: scanned for [${got.join(", ")}], configured targets are [${targets.join(", ")}]; re-run collect`);
  }
  for (const s of scans) {
    if (s.component !== component || s.source !== source || s.tool !== tool) {
      throw new Refused(`${component}: the ${s.target} scan is from ${s.source} (${s.tool}), the ${scans[0].target} scan from ${source} (${tool}); re-run collect`);
    }
  }
  const order = new Map(targets.map((t, i) => [t, i]));
  const ordered = [...scans].sort((a, b) => order.get(a.target) - order.get(b.target));
  const byKey = new Map();
  const texts = {};
  for (const s of ordered) {
    for (const [k, t] of Object.entries(s.texts)) {
      texts[k] ??= { id: t.id, body: t.body, notices: {} };
      for (const [cr, pkgs] of Object.entries(t.notices)) {
        const list = (texts[k].notices[cr] ??= []);
        for (const p of pkgs) if (!list.includes(p)) list.push(p);
      }
    }
    for (const p of s.packages) {
      const key = `${p.name} ${p.version}`;
      const have = byKey.get(key);
      if (!have) {
        byKey.set(key, { ...p, texts: [...p.texts], targets: [s.target] });
        continue;
      }
      if (have.licence !== p.licence) throw new Refused(`${component}: ${key} is ${have.licence} on ${have.targets[0]} but ${p.licence} on ${s.target}`);
      have.texts = [...new Set([...have.texts, ...p.texts])].sort();
      have.targets.push(s.target);
    }
  }
  const platforms = ordered.map((s) => s.platform ?? s.target);
  return { component, source, tool: `${tool}, ${platforms.join(", ")}`, targets: [...targets], packages: sortPackages([...byKey.values()]), texts };
}

/**
 * Render the notices file from normalised components [{ component, source, tool, packages, texts }].
 * `targets` (the release targets) is listed in the header; a package whose `targets` is not all of
 * them is marked with the ones it is on. Returns the text. Text ids are T1.. in order of first use
 * by sorted component, then package.
 */
export function renderNotices(components, { targets = [] } = {}) {
  const comps = [...components].sort((a, b) => (a.component < b.component ? -1 : 1));
  // Merge texts across components: one body, the union of copyright blocks and their packages.
  const all = {};
  for (const c of comps) {
    for (const [k, t] of Object.entries(c.texts)) {
      all[k] ??= { id: t.id, body: t.body, notices: {} };
      for (const [cr, pkgs] of Object.entries(t.notices)) {
        const list = (all[k].notices[cr] ??= []);
        for (const p of pkgs) if (!list.includes(p)) list.push(p);
      }
    }
  }
  const ids = new Map();
  for (const c of comps) for (const p of c.packages) for (const k of p.texts) if (!ids.has(k)) ids.set(k, `T${ids.size + 1}`);
  const out = [];
  out.push("Citrate Core: third-party notices");
  out.push("");
  out.push("The programs in this app contain the third-party packages listed below. Each is used under the");
  out.push("licence shown, whose full text follows: each distinct text once, under the copyright lines of");
  out.push("the packages it applies to. Generated by");
  out.push("scripts/third-party-notices.mjs from release/notices.json; do not edit by hand.");
  out.push("");
  if (targets.length) {
    out.push(`Targets: ${targets.join(", ")}`);
    out.push("");
  }
  out.push("Components:");
  for (const c of comps) out.push(`  ${c.component}: ${c.source}, ${c.tool}, ${c.packages.length} third-party packages`);
  out.push("");
  for (const c of comps) {
    out.push(`## ${c.component}`);
    out.push("");
    for (const p of c.packages) {
      const only = p.targets && targets.length && p.targets.length < targets.length ? `  (on ${p.targets.join(", ")} only)` : "";
      out.push(`${p.name} ${p.version}  ${p.licence}  [${p.texts.map((k) => ids.get(k)).join(", ")}]${p.url ? `  ${p.url}` : ""}${only}`);
    }
    out.push("");
  }
  out.push("## Licence texts");
  out.push("");
  for (const [k, id] of ids) {
    const t = all[k];
    out.push(`---- ${id}: ${t.id} ----`);
    out.push("");
    const blocks = Object.keys(t.notices).sort();
    for (const cr of blocks) {
      out.push(cr);
      out.push(`  applies to: ${[...t.notices[cr]].sort().join(", ")}`);
    }
    if (blocks.length) out.push("");
    out.push(t.body.replace(/\r\n/g, "\n").replace(/[ \t]+$/gm, "").replace(/^\n+|\n+$/g, ""));
    out.push("");
  }
  return out.join("\n");
}

/**
 * Serialise release/licences.json the way it is written by hand: two-space indent, arrays of
 * strings, numbers or null on one line, everything else expanded. Keeps render's diff to the lines
 * it changes.
 */
export function formatInventory(value, indent = "", prefix = 0) {
  const inner = `${indent}  `;
  if (Array.isArray(value)) {
    if (value.length === 0) return "[]";
    if (value.every((v) => v === null || ["string", "number", "boolean"].includes(typeof v))) {
      const flat = `[${value.map((v) => JSON.stringify(v)).join(", ")}]`;
      if (prefix + flat.length <= INLINE_WIDTH) return flat;
    }
    return `[\n${value.map((v) => `${inner}${formatInventory(v, inner, inner.length)}`).join(",\n")}\n${indent}]`;
  }
  if (value && typeof value === "object") {
    const entries = Object.entries(value);
    if (entries.length === 0) return "{}";
    return `{\n${entries
      .map(([k, v]) => {
        const head = `${inner}${JSON.stringify(k)}: `;
        return `${head}${formatInventory(v, inner, head.length)}`;
      })
      .join(",\n")}\n${indent}}`;
  }
  return JSON.stringify(value);
}

/** Longest line an inline array may make in release/licences.json (its hand-written style). */
const INLINE_WIDTH = 138;

/** Record the notices file on each covered component of an inventory. Returns the updated inventory. */
export function updateInventory(inv, components, output) {
  const byId = new Map(inv.components.map((c) => [c.id, c]));
  for (const c of components) {
    const target = c.component === "app" ? inv.app : byId.get(c.component);
    if (!target) throw new Refused(`${CONFIG} component ${c.component} has no entry in release/licences.json`);
    target.third_party_notices = { file: output, source: c.source, packages: c.packages.length };
  }
  return inv;
}

// ---- Lockfile freshness (the app's own notices against this repo's lockfiles) ----------------
// The app and webview sections are generated from this repo, so they go stale whenever a later
// change moves Cargo.lock or package-lock.json; `check` compares the file with notices.json only
// and could not see it (0.5.2: the app section stayed at the 0.5.1 bump while chacha20 and a
// webpki update landed). `render` records a digest of each lockfile's package set on the app and
// app-webview inventory entries (`lock`), and `check` refuses a release while (a) a listed package
// is no longer in the lockfile or (b) the lockfile's package set differs from the rendered one.

/** "name version" of every registry crate reachable from `root` in a Cargo.lock (all dependency
 *  kinds and targets: Cargo.lock does not record them, so this is a superset of what ships). */
export function cargoLockPackages(lockText, root = "citrate-core") {
  const pkgs = lockText.split(/^\[\[package\]\]$/m).slice(1).map((b) => {
    const get = (k) => new RegExp(`^${k} = "([^"]*)"`, "m").exec(b)?.[1];
    const deps = (/^dependencies = \[([\s\S]*?)^\]/m.exec(b)?.[1] ?? "")
      .split("\n")
      .map((s) => s.trim().replace(/^"|",?$/g, ""))
      .filter(Boolean);
    return { name: get("name"), version: get("version"), source: get("source") ?? "", deps };
  });
  const byName = new Map();
  for (const p of pkgs) byName.set(p.name, [...(byName.get(p.name) ?? []), p]);
  const resolve = (d) => {
    const [n, v] = d.split(" ");
    const c = byName.get(n) ?? [];
    return v ? c.find((p) => p.version === v) : c.length === 1 ? c[0] : undefined;
  };
  const start = (byName.get(root) ?? [])[0];
  if (!start) throw new Refused(`Cargo.lock has no package ${root}`);
  const seen = new Set();
  const out = new Set();
  const stack = [start];
  while (stack.length) {
    const p = stack.pop();
    const key = `${p.name} ${p.version}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (p.source.startsWith("registry+")) out.add(key);
    for (const d of p.deps) {
      const q = resolve(d);
      if (!q) throw new Refused(`Cargo.lock: ${p.name} depends on ${d}, which does not resolve to one package`);
      stack.push(q);
    }
  }
  return out;
}

/** "name version" of every installed package in a package-lock.json (lockfileVersion 2 or 3). */
export function npmLockPackages(lock) {
  const out = new Set();
  for (const [key, p] of Object.entries(lock?.packages ?? {})) {
    const i = key.lastIndexOf("node_modules/");
    if (i < 0 || !p?.version) continue;
    out.add(`${p.name ?? key.slice(i + "node_modules/".length)} ${p.version}`);
  }
  return out;
}

/** A stable digest of a package set. */
export function lockDigest(set) {
  return `sha256:${createHash("sha256").update([...set].sort().join("\n")).digest("hex")}`;
}

/** "name version" of each package listed in one component's section of a notices file. */
export function noticeSectionPackages(text, component) {
  const lines = text.split("\n");
  const start = lines.indexOf(`## ${component}`);
  if (start < 0) return [];
  const out = [];
  for (const l of lines.slice(start + 1)) {
    if (l.startsWith("## ")) break;
    const m = /^(\S+) (\S+)  \S/.exec(l);
    if (m) out.push(`${m[1]} ${m[2]}`);
  }
  return out;
}

/** The repo-built components whose notices follow this repo's lockfiles. */
export const LOCK_COMPONENTS = [
  { component: "app", lockfile: "Cargo.lock", packages: (repo) => cargoLockPackages(fs.readFileSync(path.join(repo, "Cargo.lock"), "utf8")) },
  { component: "app-webview", lockfile: "package-lock.json", packages: (repo) => npmLockPackages(JSON.parse(fs.readFileSync(path.join(repo, "package-lock.json"), "utf8"))) },
];

/** The current package set of each repo-built component, from this repo's lockfiles. */
export const lockSets = (repo) => Object.fromEntries(LOCK_COMPONENTS.map((l) => [l.component, l.packages(repo)]));

const inventoryEntry = (inv, component) => (component === "app" ? inv.app : inv.components.find((c) => c.id === component));

/** render: record each lockfile's package digest on its component's inventory entry. */
export function recordLockDigests(inv, sets) {
  for (const [component, set] of Object.entries(sets)) {
    const n = inventoryEntry(inv, component)?.third_party_notices;
    if (n) n.lock = lockDigest(set);
  }
  return inv;
}

/** check: the notices of the repo-built components against the lockfiles; [] when fresh. */
export function lockFreshness(text, inv, sets, lockfiles = {}) {
  const problems = [];
  for (const [component, set] of Object.entries(sets)) {
    const lockfile = lockfiles[component] ?? "the lockfile";
    const gone = noticeSectionPackages(text, component).filter((k) => !set.has(k));
    if (gone.length) problems.push(`${component}: listed but not in ${lockfile}: ${gone.join(", ")}`);
    const recorded = inventoryEntry(inv, component)?.third_party_notices?.lock;
    if (!recorded) problems.push(`${component}: release/licences.json records no ${lockfile} digest`);
    else if (recorded !== lockDigest(set)) problems.push(`${component}: ${lockfile} changed since the notices were rendered`);
  }
  return problems;
}

function run(cmd, args, opts = {}) {
  try {
    return execFileSync(cmd, args, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"], ...opts });
  } catch (e) {
    const err = new Usage(`${cmd} ${args.join(" ")} failed: ${(e.stderr || e.message || "").toString().split("\n").filter(Boolean).slice(-5).join(" | ")}`);
    throw err;
  }
}

/** "<repo>@<sha>" (with -dirty when the work tree has tracked changes) for a checkout. */
export function sourceRev(dir, label) {
  const sha = run("git", ["-C", dir, "rev-parse", "HEAD"]).trim();
  const dirty = run("git", ["-C", dir, "status", "--porcelain", "--untracked-files=no"]).trim() !== "";
  return `${label}@${sha}${dirty ? "-dirty" : ""}`;
}

const escapeModule = (m) => m.replace(/[A-Z]/g, (c) => `!${c.toLowerCase()}`);

/** Scan one Rust component for one target; returns the raw cargo-about output with where it came from. */
function collectRust(cfg, entry, fedRoot, target) {
  const repoDir = entry.repo === "." ? REPO : path.join(fedRoot, entry.repo);
  const manifest = path.join(repoDir, entry.manifest);
  if (!fs.existsSync(manifest)) throw new Usage(`${entry.component}: ${manifest} not found (pass --federation-root)`);
  const args = ["about", "generate", "--format", "json", "--fail", "-c", path.join(REPO, "release", "about.toml"), "-m", manifest, "--locked", "--target", target];
  if (entry.no_default_features) args.push("--no-default-features");
  if (entry.features?.length) args.push("--features", entry.features.join(" "));
  const about = JSON.parse(run("cargo", args, { cwd: repoDir }));
  const version = run("cargo", ["about", "--version"]).trim();
  return { component: entry.component, kind: "rust", target, source: sourceRev(repoDir, entry.repo === "." ? "citrate-core" : entry.repo), tool: version, about };
}

/** Scan one Go component for one platform ({ target, goos, goarch }); returns the raw report plus every licence file it may need. */
function collectGo(cfg, entry, platform) {
  const dl = JSON.parse(run("go", ["mod", "download", "-json", `${entry.module}@${entry.version}`]));
  if (!dl.Dir) throw new Usage(`${entry.module}@${entry.version}: go mod download gave no Dir`);
  // Ask inside the module: its go.mod may select a newer toolchain, and the scanner must see that
  // toolchain's standard library (a mismatched GOROOT reports every std package as unknown).
  const goroot = run("go", ["env", "GOROOT"], { cwd: dl.Dir }).trim();
  const goVersion = run("go", ["env", "GOVERSION"], { cwd: dl.Dir }).trim();
  const modCache = run("go", ["env", "GOMODCACHE"]).trim();
  const tpl = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "notices-go-")), "report.tpl");
  fs.writeFileSync(tpl, "{{range .}}{{.Name}}\t{{.Version}}\t{{.LicenseName}}\t{{.LicensePath}}\t{{.LicenseURL}}\n{{end}}");
  const env = { ...process.env, GOROOT: goroot, GOOS: platform.goos, GOARCH: platform.goarch, CGO_ENABLED: "0", GOFLAGS: "-mod=readonly" };
  const tool = entry.tool ?? "go-licenses";
  let tsv;
  try {
    tsv = execFileSync(tool, ["report", entry.package, "--template", tpl], { cwd: dl.Dir, env, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"] });
  } catch (e) {
    // go-licenses exits non-zero when it cannot inspect assembly files; the report is still complete.
    tsv = e.stdout?.toString() ?? "";
    if (!tsv.trim()) throw new Usage(`${tool} report failed: ${(e.stderr ?? e.message).toString().split("\n").filter(Boolean).slice(-3).join(" | ")}`);
  }
  const files = {};
  const keep = (f) => {
    if (f && f !== "Unknown" && !(f in files) && fs.existsSync(f)) files[f] = fs.readFileSync(f, "utf8");
  };
  for (const line of tsv.split("\n")) {
    const [name, version, , licencePath] = line.split("\t");
    keep(licencePath);
    for (const [mod, c] of Object.entries(entry.clarify ?? {})) {
      if (name === mod || name?.startsWith(`${mod}/`)) keep(c.file === "GOROOT/LICENSE" ? path.join(goroot, "LICENSE") : path.join(modCache, `${escapeModule(mod)}@${version}`, c.file));
    }
  }
  keep(path.join(goroot, "LICENSE"));
  return {
    component: entry.component,
    kind: "go",
    target: platform.target,
    platform: `${platform.goos}/${platform.goarch}`,
    source: `${entry.module}@${entry.version} ${entry.package}`,
    tool: `go-licenses, ${goVersion}`,
    tsv,
    files,
    goroot,
    goVersion,
    modCache,
  };
}

const LICENCE_FILE = /^(licen[cs]e|copying)([._-].*)?$/i;

/** Build the webview with Vite (nothing written) and collect the npm packages in its output. */
async function collectNpm(cfg, entry) {
  const req = createRequire(path.join(REPO, "package.json"));
  let vite;
  try {
    vite = await import(pathToFileURL(req.resolve("vite")).href);
  } catch (e) {
    throw new Usage(`${entry.component}: cannot load vite from this repo (run npm ci first): ${e.message}`);
  }
  const ids = new Set();
  await vite.build({
    root: REPO,
    configFile: path.join(REPO, entry.config ?? "vite.config.ts"),
    logLevel: "error",
    build: { write: false, emptyOutDir: false },
    plugins: [
      {
        name: "citrate-third-party-notices",
        generateBundle(_opts, bundle) {
          for (const f of Object.values(bundle)) {
            if (f.type === "chunk") {
              for (const [id, m] of Object.entries(f.modules)) if (m.renderedLength > 0) ids.add(id);
            } else {
              for (const o of f.originalFileNames ?? []) ids.add(path.resolve(REPO, o));
            }
          }
        },
      },
    ],
  });
  const roots = new Set();
  for (const id of ids) {
    const r = packageRootOf(id.replace(/^\0/, "").split("?")[0]);
    if (r) roots.add(path.resolve(REPO, r));
  }
  const packages = [];
  for (const dir of [...roots].sort()) {
    const pj = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    const files = {};
    for (const f of fs.readdirSync(dir).sort()) if (LICENCE_FILE.test(f) && !/\.spdx$/i.test(f)) files[f] = fs.readFileSync(path.join(dir, f), "utf8");
    packages.push({
      name: pj.name,
      version: pj.version,
      license: typeof pj.license === "string" ? pj.license : null,
      url: `https://www.npmjs.com/package/${pj.name}/v/${pj.version}`,
      files,
    });
  }
  const clarified = {};
  for (const c of Object.values(entry.clarify ?? {})) for (const f of c.files ?? []) clarified[f] = fs.readFileSync(path.join(REPO, f), "utf8");
  const viteVersion = JSON.parse(fs.readFileSync(path.join(path.dirname(req.resolve("vite/package.json")), "package.json"), "utf8")).version;
  return {
    component: entry.component,
    kind: "npm",
    source: sourceRev(REPO, "citrate-core"),
    tool: `vite ${viteVersion} build (${entry.config ?? "vite.config.ts"}), ${ids.size} bundled modules and assets`,
    packages,
    clarified,
  };
}

/** Turn one collected component into { component, source, tool, packages, texts } (clarifications from the config). */
export function normaliseCollected(raw, cfg) {
  const meta = { component: raw.component, source: raw.source, tool: raw.tool };
  if (raw.target) meta.target = raw.target;
  if (raw.platform) meta.platform = raw.platform;
  if (raw.kind === "rust") return { ...meta, ...normaliseCargoAbout(raw.about, { firstPartySources: cfg.first_party_sources ?? [], elect: cfg.elect ?? {} }) };
  if (raw.kind === "npm") {
    const npmEntry = (cfg.npm ?? []).find((e) => e.component === raw.component);
    if (!npmEntry) throw new Refused(`${raw.component}: not configured in ${CONFIG}`);
    return { ...meta, ...normaliseNpm(raw, npmEntry) };
  }
  if (raw.kind !== "go") throw new Refused(`${raw.component}: unknown collected kind ${raw.kind}`);
  const entry = (cfg.go ?? []).find((e) => e.component === raw.component);
  if (!entry) throw new Refused(`${raw.component}: not configured in ${CONFIG}`);
  const readText = (f) => {
    if (!(f in raw.files)) throw new Refused(`${raw.component}: licence file ${f} was not collected; re-run collect`);
    return raw.files[f];
  };
  const n = normaliseGoLicenses(raw.tsv, {
    mainModule: entry.module,
    clarify: entry.clarify ?? {},
    readText,
    goroot: raw.goroot,
    moduleDir: (mod, version) => path.join(raw.modCache, `${escapeModule(mod)}@${version}`),
  });
  // The Go standard library is compiled into every Go binary.
  const k = addText(n.texts, "BSD-3-Clause", readText(path.join(raw.goroot, "LICENSE")), `go (standard library) ${raw.goVersion}`);
  n.packages = sortPackages([...n.packages, { name: "go (standard library)", version: raw.goVersion, licence: "BSD-3-Clause", texts: [k], url: "https://go.dev/LICENSE" }]);
  return { ...meta, ...n };
}

function parseArgs(argv) {
  const [cmd, ...rest] = argv;
  if (!["collect", "render", "check"].includes(cmd)) throw new Usage(cmd ? `unknown command ${cmd}` : "a command is required");
  const a = { cmd, fedRoot: path.resolve(REPO, ".."), only: null, target: null, work: path.join(REPO, "target", "third-party-notices") };
  for (let i = 0; i < rest.length; i++) {
    const k = rest[i];
    const v = () => {
      const x = rest[++i];
      if (x === undefined || x.startsWith("--")) throw new Usage(`${k} needs a value`);
      return x;
    };
    if (k === "--federation-root") a.fedRoot = path.resolve(v());
    else if (k === "--only") a.only = v();
    else if (k === "--target") a.target = v();
    else if (k === "--work") a.work = path.resolve(v());
    else throw new Usage(`unknown argument ${k}`);
  }
  return a;
}

/** Where collect keeps one raw scan: `<component>@<target>.json` for rust and go, `<component>.json` for npm. */
const collectedFile = (work, component, target = null) => path.join(work, target ? `${component}@${target}.json` : `${component}.json`);

function loadCollected(work, cfg) {
  const read = (component, target) => {
    const f = collectedFile(work, component, target);
    if (!fs.existsSync(f)) throw new Refused(`${component}${target ? ` (${target})` : ""}: not collected yet (${f}); run collect first`);
    return normaliseCollected(JSON.parse(fs.readFileSync(f, "utf8")), cfg);
  };
  const comps = [];
  for (const e of [...(cfg.rust ?? []), ...(cfg.go ?? [])]) {
    comps.push(mergeTargets(cfg.targets.map((t) => read(e.component, t)), cfg.targets));
  }
  for (const e of cfg.npm ?? []) comps.push(read(e.component, null));
  return comps;
}

/** Target triples on the notices file's "Targets:" header line ([] when it has none). */
export function noticeTargets(text) {
  const line = text.split("\n").find((l) => l.startsWith("Targets: "));
  return line ? line.slice("Targets: ".length).split(", ").filter(Boolean) : [];
}

/** Components named in the committed notices file's header, for `check`. */
export function noticeComponents(text) {
  const out = [];
  const lines = text.split("\n");
  const start = lines.indexOf("Components:");
  if (start < 0) return out;
  for (const l of lines.slice(start + 1)) {
    const m = /^ {2}([^:]+): /.exec(l);
    if (!m) break;
    out.push(m[1]);
  }
  return out;
}

async function main(argv) {
  const a = parseArgs(argv);
  const cfg = readConfig();
  if (a.cmd === "collect") {
    fs.mkdirSync(a.work, { recursive: true });
    if (a.target && !cfg.targets.includes(a.target)) throw new Usage(`--target ${a.target} is not one of the configured targets [${cfg.targets.join(", ")}]`);
    const targets = a.target ? [a.target] : cfg.targets;
    for (const e of cfg.rust ?? []) {
      if (a.only && a.only !== e.component) continue;
      for (const t of targets) {
        process.stderr.write(`collecting ${e.component} for ${t} (cargo-about)\n`);
        const c = collectRust(cfg, e, a.fedRoot, t);
        fs.writeFileSync(collectedFile(a.work, e.component, t), JSON.stringify(c));
        process.stdout.write(`${e.component} (${t}): collected from ${c.source}\n`);
      }
    }
    for (const e of cfg.go ?? []) {
      if (a.only && a.only !== e.component) continue;
      for (const p of e.platforms.filter((x) => targets.includes(x.target))) {
        process.stderr.write(`collecting ${e.component} for ${p.goos}/${p.goarch} (go-licenses)\n`);
        const c = collectGo(cfg, e, p);
        fs.writeFileSync(collectedFile(a.work, e.component, p.target), JSON.stringify(c));
        process.stdout.write(`${e.component} (${p.goos}/${p.goarch}): collected from ${c.source}\n`);
      }
    }
    for (const e of cfg.npm ?? []) {
      if (a.only && a.only !== e.component) continue;
      process.stderr.write(`collecting ${e.component} (vite build)\n`);
      const c = await collectNpm(cfg, e);
      fs.writeFileSync(collectedFile(a.work, e.component), JSON.stringify(c));
      process.stdout.write(`${e.component}: collected ${c.packages.length} packages from ${c.source}\n`);
    }
    return 0;
  }
  if (a.cmd === "render") {
    const comps = loadCollected(a.work, cfg);
    const text = renderNotices(comps, { targets: cfg.targets });
    fs.writeFileSync(path.join(REPO, cfg.output), text);
    const invPath = path.join(REPO, "release", "licences.json");
    const inv = recordLockDigests(updateInventory(JSON.parse(fs.readFileSync(invPath, "utf8")), comps, cfg.output), lockSets(REPO));
    fs.writeFileSync(invPath, formatInventory(inv) + "\n");
    process.stdout.write(`wrote ${cfg.output} (${Buffer.byteLength(text)} bytes, ${comps.length} components, ${cfg.targets.length} targets) and updated release/licences.json\n`);
    return 0;
  }
  // check: the committed file names exactly the configured components and targets, and the
  // inventory points at it.
  const file = path.join(REPO, cfg.output);
  if (!fs.existsSync(file)) throw new Refused(`${cfg.output} does not exist; run collect and render`);
  const committed = fs.readFileSync(file, "utf8");
  const gotTargets = noticeTargets(committed);
  if (JSON.stringify(gotTargets) !== JSON.stringify(cfg.targets)) {
    throw new Refused(`${cfg.output} covers targets [${gotTargets.join(", ")}], ${CONFIG} configures [${cfg.targets.join(", ")}]; re-run collect and render`);
  }
  const want = [...(cfg.rust ?? []), ...(cfg.go ?? []), ...(cfg.npm ?? [])].map((e) => e.component).sort();
  const got = noticeComponents(committed).sort();
  if (JSON.stringify(want) !== JSON.stringify(got)) throw new Refused(`${cfg.output} covers [${got.join(", ")}], ${CONFIG} configures [${want.join(", ")}]; re-run collect and render`);
  const inv = JSON.parse(fs.readFileSync(path.join(REPO, "release", "licences.json"), "utf8"));
  for (const id of want) {
    const target = id === "app" ? inv.app : inv.components.find((c) => c.id === id);
    if (target?.third_party_notices?.file !== cfg.output) throw new Refused(`release/licences.json ${id} does not point at ${cfg.output}; re-run render`);
  }
  const stale = lockFreshness(committed, inv, lockSets(REPO), Object.fromEntries(LOCK_COMPONENTS.map((l) => [l.component, l.lockfile])));
  if (stale.length) throw new Refused(`${cfg.output} is stale against this repo's lockfiles; re-run collect and render:\n  ${stale.join("\n  ")}`);
  process.stdout.write(`third-party notices: OK (${want.length} components, ${cfg.targets.length} targets in ${cfg.output})\n`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code;
    },
    (e) => {
      process.stderr.write(`third-party-notices: ${e.message}\n${e instanceof Usage ? `${USAGE}\n` : ""}`);
      process.exitCode = e instanceof Refused ? 1 : 2;
    },
  );
}
