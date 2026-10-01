#!/usr/bin/env node
// =====================================================================
// citrate-core: installer size budget check (HUP-S11.0, gate g5-size)
//
//   node scripts/size-budget.mjs [--bundle-dir target/release/bundle] [--budgets release/budgets.json]
//        [--arch aarch64|x86_64] [--strict] [--markdown <file>] [--json <file>]
//
// Measures what a Tauri build left in the bundle dir and compares it with release/budgets.json:
//
//   artifacts   the files a member downloads: dmg, app.tar.gz (macOS updater), appimage,
//               appimage.tar.gz, deb, rpm, msi, nsis. Signatures (.sig) and build helpers
//               (bundle_dmg.sh) are not artifacts.
//   components  what is inside the app payload: every binary next to the main executable
//               (sidecars, llama-server), every top-level Resources entry (llama dylibs,
//               models, docs-corpus, capsules, ...), and the payload total ("app").
//
// Ids are "<os>-<arch>/<kind>" for artifacts and "<os>-<arch>/bin/<name>",
// "<os>-<arch>/resources/<name>", "<os>-<arch>/app" for components. Symlinks count 0 bytes
// (they ship as links); "dup" is the bytes held by byte-identical copies inside a component.
//
// Exit 0: every gated row is within budget. Exit 1: a row is over budget (or, with --strict,
// a measured row has no budget). Exit 2: usage error, unreadable budgets, or no artifacts.
// A budget of null means "measured and reported, not gated" (no baseline yet for that row).
// No dependencies beyond Node's standard library.
// =====================================================================
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Shipped v0.4.2 release assets (GitHub release v0.4.2, macOS aarch64), the budget baseline. */
export const V042_SHIPPED = Object.freeze({ dmg: 394_782_331, appTarGz: 366_205_330 });

// [subdir, filename test, kind, os]. Order matters: ".AppImage.tar.gz" before ".AppImage".
const ARTIFACT_RULES = [
  ["dmg", (n) => n.endsWith(".dmg"), "dmg", "macos"],
  ["macos", (n) => n.endsWith(".app.tar.gz"), "app.tar.gz", "macos"],
  ["appimage", (n) => n.endsWith(".AppImage.tar.gz"), "appimage.tar.gz", "linux"],
  ["appimage", (n) => n.endsWith(".AppImage"), "appimage", "linux"],
  ["deb", (n) => n.endsWith(".deb"), "deb", "linux"],
  ["rpm", (n) => n.endsWith(".rpm"), "rpm", "linux"],
  ["msi", (n) => n.endsWith(".msi"), "msi", "windows"],
  ["nsis", (n) => n.endsWith(".exe"), "nsis", "windows"],
];

const ARCH_ALIASES = [
  [/(?:^|[_.-])(aarch64|arm64)(?:[_.-]|$)/i, "aarch64"],
  [/(?:^|[_.-])(x86_64|amd64|x64)(?:[_.-]|$)/i, "x86_64"],
  [/(?:^|[_.-])(universal)(?:[_.-]|$)/i, "universal"],
];

export function archFromName(name) {
  for (const [re, arch] of ARCH_ALIASES) if (re.test(name)) return arch;
  return null;
}

function isDir(p) {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

function listDir(p) {
  try {
    return fs.readdirSync(p, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
  } catch {
    return [];
  }
}

const hashCache = new Map();

function sha256File(p, st) {
  const key = `${p}\0${st.size}\0${st.mtimeMs}`;
  let h = hashCache.get(key);
  if (h === undefined) {
    h = createHash("sha256").update(fs.readFileSync(p)).digest("hex");
    hashCache.set(key, h);
  }
  return h;
}

/** Walk a file or dir without following symlinks. Returns {bytes, files, dupBytes}. */
export function measurePath(p) {
  const seen = new Map(); // `${size}:${sha}` -> true
  let bytes = 0;
  let files = 0;
  let dupBytes = 0;
  const visit = (q) => {
    const st = fs.lstatSync(q);
    if (st.isSymbolicLink()) return; // ships as a link: no bytes, not a file copy
    if (st.isDirectory()) {
      for (const e of listDir(q)) visit(path.join(q, e.name));
      return;
    }
    if (!st.isFile()) return;
    files += 1;
    bytes += st.size;
    if (st.size > 0) {
      const key = `${st.size}:${sha256File(q, st)}`;
      if (seen.has(key)) dupBytes += st.size;
      else seen.set(key, true);
    }
  };
  visit(p);
  return { bytes, files, dupBytes };
}

/** Find the app payload: {binDir, resDir, root}, or null when the build left none. */
function findPayload(bundleDir, os) {
  if (os === "macos") {
    for (const e of listDir(path.join(bundleDir, "macos"))) {
      const contents = path.join(bundleDir, "macos", e.name, "Contents");
      if (e.name.endsWith(".app") && isDir(contents)) {
        return { root: contents, binDir: path.join(contents, "MacOS"), resDir: path.join(contents, "Resources") };
      }
    }
    return null;
  }
  if (os === "linux") {
    const roots = [];
    for (const e of listDir(path.join(bundleDir, "appimage"))) {
      if (e.name.endsWith(".AppDir")) roots.push([path.join(bundleDir, "appimage", e.name, "usr"), e.name.slice(0, -7)]);
    }
    for (const e of listDir(path.join(bundleDir, "deb"))) {
      if (e.isDirectory()) roots.push([path.join(bundleDir, "deb", e.name, "data", "usr"), e.name.split("_")[0]]);
    }
    for (const [usr, product] of roots) {
      if (!isDir(usr)) continue;
      const lib = path.join(usr, "lib");
      const dirs = listDir(lib).filter((d) => d.isDirectory());
      const prod = dirs.find((d) => d.name === product) ?? dirs.find((d) => !/-linux-gnu$/.test(d.name));
      return { root: usr, binDir: path.join(usr, "bin"), resDir: prod ? path.join(lib, prod.name) : null };
    }
    return null;
  }
  return null; // Windows installers are measured as artifacts only (no unpacked payload dir).
}

/**
 * Measure a Tauri bundle dir. Throws when the dir is missing or holds no installer artifact.
 * @param {string} bundleDir
 * @param {{arch?: string}} [opts]
 */
export function measureBundle(bundleDir, opts = {}) {
  if (!isDir(bundleDir)) throw new Error(`bundle dir is not a directory: ${bundleDir}`);
  const found = [];
  for (const [sub, test, kind, os] of ARTIFACT_RULES) {
    for (const e of listDir(path.join(bundleDir, sub))) {
      if (!e.isFile() || !test(e.name)) continue;
      if (found.some((f) => f.name === e.name && f.sub === sub)) continue; // matched an earlier rule
      const p = path.join(bundleDir, sub, e.name);
      found.push({ sub, name: e.name, kind, os, path: p, bytes: fs.statSync(p).size });
    }
  }
  if (found.length === 0) {
    throw new Error(`no installer artifacts (dmg, app.tar.gz, AppImage, deb, rpm, msi, nsis) under ${bundleDir}`);
  }
  const os = found[0].os;
  const arch = opts.arch ?? found.map((f) => archFromName(f.name)).find((a) => a !== null) ?? "unknown";
  const target = { os, arch };
  const artifacts = found.map((f) => ({
    id: `${f.os}-${arch}/${f.kind}`,
    kind: f.kind,
    path: f.path,
    bytes: f.bytes,
    files: 1,
    dupBytes: 0,
  }));

  const components = [];
  const payload = findPayload(bundleDir, os);
  if (payload) {
    const prefix = `${os}-${arch}`;
    for (const e of listDir(payload.binDir)) {
      const m = measurePath(path.join(payload.binDir, e.name));
      components.push({ id: `${prefix}/bin/${e.name}`, kind: "bin", path: path.join(payload.binDir, e.name), ...m });
    }
    if (payload.resDir) {
      for (const e of listDir(payload.resDir)) {
        const p = path.join(payload.resDir, e.name);
        components.push({ id: `${prefix}/resources/${e.name}`, kind: "resources", path: p, ...measurePath(p) });
      }
    }
    components.push({ id: `${prefix}/app`, kind: "app", path: payload.root, ...measurePath(payload.root) });
  }
  return { bundleDir, target, artifacts, components };
}

/** Validate a parsed budgets object; throws with the offending key. Returns it unchanged. */
export function validateBudgets(b) {
  if (!b || typeof b !== "object") throw new Error("budgets: not an object");
  if (b.version !== 1) throw new Error(`budgets: version must be 1 (got ${JSON.stringify(b.version)})`);
  for (const section of ["artifacts", "components"]) {
    const s = b[section];
    if (!s || typeof s !== "object" || Array.isArray(s)) throw new Error(`budgets: "${section}" must be an object`);
    for (const [id, row] of Object.entries(s)) {
      if (!row || typeof row !== "object" || !("maxBytes" in row)) {
        throw new Error(`budgets: ${section}["${id}"] needs maxBytes (an integer >= 0, or null)`);
      }
      const v = row.maxBytes;
      if (v !== null && !(Number.isSafeInteger(v) && v >= 0)) {
        throw new Error(`budgets: ${section}["${id}"].maxBytes must be an integer >= 0 or null (got ${JSON.stringify(v)})`);
      }
    }
  }
  return b;
}

export function loadBudgets(file) {
  let raw;
  try {
    raw = JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (e) {
    throw new Error(`budgets: cannot read ${file}: ${e instanceof Error ? e.message : String(e)}`);
  }
  return validateBudgets(raw);
}

/**
 * Compare a measurement with budgets. Row status: ok | over | tracked (budget null) | unbudgeted.
 * `notBuilt` lists budgeted ids for this target that the build did not produce (informational).
 */
export function checkBudgets(measured, budgets, opts = {}) {
  const strict = opts.strict === true;
  const rows = [];
  const add = (m, section) => {
    const b = budgets[section][m.id];
    let status;
    let maxBytes = null;
    if (!b) status = "unbudgeted";
    else if (b.maxBytes === null) status = "tracked";
    else {
      maxBytes = b.maxBytes;
      status = m.bytes > b.maxBytes ? "over" : "ok";
    }
    rows.push({ id: m.id, section, bytes: m.bytes, dupBytes: m.dupBytes, maxBytes, status });
  };
  for (const a of measured.artifacts) add(a, "artifacts");
  for (const c of measured.components) add(c, "components");
  const prefix = `${measured.target.os}-${measured.target.arch}/`;
  const ids = new Set(rows.map((r) => r.id));
  const notBuilt = Object.keys(budgets.artifacts).filter((id) => id.startsWith(prefix) && !ids.has(id));
  const over = rows.filter((r) => r.status === "over");
  const unbudgeted = rows.filter((r) => r.status === "unbudgeted");
  const ok = over.length === 0 && (!strict || unbudgeted.length === 0);
  return { target: measured.target, bundleDir: measured.bundleDir, strict, rows, over, unbudgeted, notBuilt, ok };
}

export function formatBytes(n) {
  if (n < 1000) return `${n} B`;
  if (n < 1e6) return `${(n / 1e3).toFixed(1)} kB`;
  if (n < 1e9) return `${(n / 1e6).toFixed(1)} MB`;
  return `${(n / 1e9).toFixed(2)} GB`;
}

const withCommas = (n) => n.toLocaleString("en-US");

function cells(r) {
  return [
    r.id,
    formatBytes(r.bytes),
    r.maxBytes === null ? "-" : formatBytes(r.maxBytes),
    r.maxBytes ? `${((r.bytes / r.maxBytes) * 100).toFixed(1)}%` : "-",
    r.dupBytes ? formatBytes(r.dupBytes) : "-",
    r.status === "over" ? "OVER" : r.status,
  ];
}

function verdict(res) {
  const parts = [`${res.over.length} over budget`, `${res.unbudgeted.length} unbudgeted`];
  if (res.notBuilt.length) parts.push(`not built: ${res.notBuilt.join(", ")}`);
  return `${res.ok ? "PASS" : "FAIL"} (${res.target.os}-${res.target.arch}${res.strict ? ", strict" : ""}): ${parts.join("; ")}`;
}

/** Plain-text table for the terminal. */
export function renderTable(res) {
  const head = ["id", "size", "budget", "use", "dup", "status"];
  const body = res.rows.map(cells);
  const w = head.map((h, i) => Math.max(h.length, ...body.map((r) => r[i].length)));
  const line = (r) => r.map((c, i) => (i === 0 ? c.padEnd(w[i]) : c.padStart(w[i]))).join("  ");
  return [line(head), w.map((n) => "-".repeat(n)).join("  "), ...body.map(line), "", verdict(res)].join("\n");
}

/** Markdown table (CI step summary / release notes). Sizes carry the exact byte count. */
export function renderMarkdown(res) {
  const out = [
    `### Installer size budget: ${res.target.os}-${res.target.arch}`,
    "",
    "| id | size | budget | use | dup | status |",
    "|---|---:|---:|---:|---:|---|",
  ];
  for (const r of res.rows) {
    const c = cells(r);
    const size = `${withCommas(r.bytes)} B (${c[1]})`;
    const budget = r.maxBytes === null ? "-" : `${withCommas(r.maxBytes)} B`;
    out.push(`| ${c[0]} | ${size} | ${budget} | ${c[3]} | ${c[4]} | ${c[5]} |`);
  }
  out.push("", verdict(res), "");
  return out.join("\n");
}

const USAGE =
  "usage: node scripts/size-budget.mjs [--bundle-dir target/release/bundle] [--budgets release/budgets.json] " +
  "[--arch aarch64|x86_64] [--strict] [--markdown <file>] [--json <file>]";

export function parseArgs(argv, root) {
  const out = {
    bundleDir: path.join(root, "target", "release", "bundle"),
    budgets: path.join(root, "release", "budgets.json"),
    strict: false,
  };
  const flags = { "--bundle-dir": "bundleDir", "--budgets": "budgets", "--arch": "arch", "--markdown": "markdown", "--json": "json" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--strict") out.strict = true;
    else if (a in flags) {
      const v = argv[i + 1];
      if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${USAGE}`);
      out[flags[a]] = v;
      i++;
    } else throw new Error(`unknown argument ${JSON.stringify(a)}\n${USAGE}`);
  }
  return out;
}

function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  let args;
  let res;
  try {
    args = parseArgs(process.argv.slice(2), root);
    const budgets = loadBudgets(path.resolve(args.budgets));
    const measured = measureBundle(path.resolve(args.bundleDir), args.arch ? { arch: args.arch } : {});
    res = checkBudgets(measured, budgets, { strict: args.strict });
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  console.log(renderTable(res));
  if (args.markdown) fs.writeFileSync(args.markdown, renderMarkdown(res));
  if (args.json) fs.writeFileSync(args.json, JSON.stringify(res, null, 2) + "\n");
  process.exit(res.ok ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
