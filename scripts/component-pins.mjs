#!/usr/bin/env node
// Release component pins (release/components.json): the single record of where every bundled
// sidecar and model comes from (source repo + exact rev, build command, toolchain) and which bytes
// a release may ship for each target (sha256). scripts/build-component.sh reads it to build from
// the pinned rev and refuses to install bytes that do not match; release.yml stages only pinned
// bytes; `check` (run in CI) fails when the manifest, the build scripts, the bundle overlays, the
// runtime-deps pins, release.yml and the app's own model pin disagree.
//
// A digest lives in exactly one place: either the artifact's own `sha256`, or, for bytes uploaded
// to the `runtime-deps` prerelease, the line for its `runtime_deps_asset` in
// src-tauri/runtime-deps.sha256 (which release.yml verifies before staging).
//
// Usage:
//   node scripts/component-pins.mjs field <id> <dotted.path>   print one manifest value
//   node scripts/component-pins.mjs artifact <id> <target>     print the artifact record (JSON)
//   node scripts/component-pins.mjs digest <id> <target>       print the pinned sha256; exit 1 if none
//   node scripts/component-pins.mjs check [--root <dir>]       consistency check (CI)
// CITRATE_COMPONENTS_MANIFEST overrides the manifest path; the repo root is then the manifest's
// grandparent directory (<root>/release/components.json).
//
// Zero dependencies: node:fs, node:path only.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HEX40 = /^[0-9a-f]{40}$/;
const HEX64 = /^[0-9a-f]{64}$/;
export const RUST_KINDS = new Set(["cargo"]);
export const KINDS = new Set(["cargo", "download", "manual"]);
// Scripts that build or fetch a bundled component and must go through the pinned engine.
const ENGINE = "scripts/build-component.sh";
const NON_COMPONENT_SCRIPTS = new Set(["scripts/build-bge-gguf.sh", ENGINE]);

const here = path.dirname(fileURLToPath(import.meta.url));

export function defaultManifestPath() {
  return process.env.CITRATE_COMPONENTS_MANIFEST || path.join(here, "..", "release", "components.json");
}

export function loadManifest(file = defaultManifestPath()) {
  const raw = fs.readFileSync(file, "utf8");
  return JSON.parse(raw);
}

/** Parse `shasum -a 256` lines: Map<asset, digest[]> (a list, so duplicates are visible). */
export function parsePins(text) {
  const pins = new Map();
  for (const line of text.split("\n")) {
    const m = /^([0-9a-f]{64})  (\S+)$/.exec(line);
    if (!m) continue;
    pins.set(m[2], [...(pins.get(m[2]) || []), m[1]]);
  }
  return pins;
}

function pinsFor(root) {
  const f = path.join(root, "src-tauri", "runtime-deps.sha256");
  return fs.existsSync(f) ? parsePins(fs.readFileSync(f, "utf8")) : new Map();
}

export function component(manifest, id) {
  const c = (manifest.components || []).find((x) => x.id === id) || (manifest.models || []).find((x) => x.id === id);
  if (!c) throw new Error(`no component '${id}' in the manifest`);
  return c;
}

export function getField(obj, dotted) {
  let v = obj;
  for (const k of dotted.split(".")) {
    if (v == null || typeof v !== "object" || !(k in v)) throw new Error(`no field '${dotted}'`);
    v = v[k];
  }
  return v;
}

/** The pinned sha256 for one component on one target, or throw (fail closed). */
export function pinnedDigest(manifest, pins, id, target) {
  const c = component(manifest, id);
  const a = (c.artifacts || {})[target];
  if (!a) throw new Error(`no pinned artifact for '${id}' on ${target}`);
  if (a.sha256) {
    if (!HEX64.test(a.sha256)) throw new Error(`'${id}' ${target}: sha256 is not 64 lowercase hex`);
    return a.sha256;
  }
  if (a.runtime_deps_asset) {
    const d = pins.get(a.runtime_deps_asset) || [];
    if (d.length !== 1) throw new Error(`'${id}' ${target}: runtime-deps asset ${a.runtime_deps_asset} is pinned ${d.length} times in src-tauri/runtime-deps.sha256`);
    return d[0];
  }
  throw new Error(`'${id}' ${target}: artifact has neither sha256 nor runtime_deps_asset`);
}

// ---------------------------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------------------------

function readIf(f) {
  return fs.existsSync(f) ? fs.readFileSync(f, "utf8") : null;
}

/** externalBin entries of every src-tauri/tauri*.conf.json overlay. */
export function overlayExternalBins(root) {
  const dir = path.join(root, "src-tauri");
  const bins = new Map(); // bin -> [overlay files]
  for (const f of fs.readdirSync(dir).filter((n) => /^tauri.*\.conf\.json$/.test(n)).sort()) {
    const j = JSON.parse(fs.readFileSync(path.join(dir, f), "utf8"));
    for (const b of (j.bundle && j.bundle.externalBin) || []) bins.set(b, [...(bins.get(b) || []), f]);
  }
  return bins;
}

/** Asset names listed in release.yml's runtime-deps ASSETS arrays (lines that are one bare token). */
export function releaseAssets(text) {
  const out = [];
  let inArr = false;
  for (const line of text.split("\n")) {
    if (/ASSETS(\+)?=\($/.test(line.trim())) {
      inArr = true;
      continue;
    }
    if (inArr && line.trim() === ")") {
      inArr = false;
      continue;
    }
    if (inArr) {
      const t = line.trim();
      if (t && !t.startsWith("#")) out.push(t);
    }
  }
  return out;
}

/** The workflow's steps as text blocks (each starts at a `      - ` line under `steps:`). */
export function workflowSteps(text) {
  const steps = [];
  let cur = null;
  for (const line of text.split("\n")) {
    if (/^ {6}- /.test(line)) {
      if (cur) steps.push(cur.join("\n"));
      cur = [line];
    } else if (cur && (/^ {8}/.test(line) || line.trim() === "")) cur.push(line);
    else if (cur) {
      steps.push(cur.join("\n"));
      cur = null;
    }
  }
  if (cur) steps.push(cur.join("\n"));
  return steps;
}

// The dry-run contract of release.yml: a dry run is only ever a manual dispatch with dry_run, every
// step that signs, notarizes or publishes is skipped in a dry run, and the dry-run-only steps read
// no secret but the job's own GITHUB_TOKEN.
const DRY_RUN_ENV = "DRY_RUN: ${{ github.event_name == 'workflow_dispatch' && inputs.dry_run == true }}";
const PUBLISHES = /tauri-apps\/tauri-action|gh release (create|edit|upload|delete)|secrets\.(TAURI_SIGNING|APPLE_|KEYCHAIN)/;
export function dryRunGuardErrors(text) {
  const errors = [];
  if (!text.includes(DRY_RUN_ENV)) errors.push(`release.yml must set job env ${DRY_RUN_ENV}`);
  for (const step of workflowSteps(text)) {
    const name = (/name: (.*)/.exec(step) || [])[1] || step.split("\n")[0].trim();
    const ifLine = (/^ {8}if: (.*)$/m.exec(step) || [])[1] || "";
    if (PUBLISHES.test(step) && !ifLine.includes("env.DRY_RUN != 'true'")) errors.push(`release.yml step '${name}' signs or publishes but is not guarded by env.DRY_RUN != 'true'`);
    if (ifLine.includes("env.DRY_RUN == 'true'")) {
      for (const s of step.match(/secrets\.[A-Z_]+/g) || []) if (s !== "secrets.GITHUB_TOKEN") errors.push(`release.yml dry-run step '${name}' reads ${s}`);
      if (/gh release (create|edit|upload|delete)/.test(step)) errors.push(`release.yml dry-run step '${name}' writes a release`);
    }
  }
  return errors;
}

export function check(root) {
  const errors = [];
  const err = (m) => errors.push(m);
  const mf = path.join(root, "release", "components.json");
  let m;
  try {
    m = JSON.parse(fs.readFileSync(mf, "utf8"));
  } catch (e) {
    return [`cannot read ${mf}: ${e.message}`];
  }
  if (m.version !== 1) err("manifest version must be 1");
  const tc = m.toolchain && m.toolchain.rust;
  if (!tc || !/^\d+\.\d+\.\d+$/.test(tc)) err("toolchain.rust must be an exact x.y.z version");
  const pins = pinsFor(root);
  const comps = m.components || [];
  const ids = new Set();
  for (const c of [...comps, ...(m.models || [])]) {
    if (ids.has(c.id)) err(`duplicate id '${c.id}'`);
    ids.add(c.id);
  }

  // Federation drift is recorded, never silent.
  for (const c of comps) {
    const fed = c.federation;
    if (!fed) {
      err(`'${c.id}': no federation record (status match | drift | not-pinned)`);
      continue;
    }
    if (!["match", "drift", "not-pinned"].includes(fed.status)) err(`'${c.id}': federation.status must be match, drift or not-pinned`);
    if (fed.status === "match" && fed.rev !== (c.source && c.source.rev)) err(`'${c.id}': federation.status is match but federation.rev ${fed.rev} != source.rev ${c.source && c.source.rev}`);
    if (fed.status !== "match" && !(fed.note && fed.note.trim())) err(`'${c.id}': federation ${fed.status} needs a note`);
  }

  // Every component: kind, source, artifacts.
  for (const c of comps) {
    const b = c.build || {};
    if (!KINDS.has(b.kind)) {
      err(`'${c.id}': build.kind must be one of ${[...KINDS].join(", ")}`);
      continue;
    }
    const src = c.source || {};
    if (!src.repo) err(`'${c.id}': source.repo missing`);
    if (b.kind === "cargo" || b.kind === "manual") {
      if (!HEX40.test(src.rev || "")) err(`'${c.id}': source.rev must be a full 40-hex commit (got '${src.rev}')`);
    }
    if (b.kind === "cargo") {
      if (!src.checkout) err(`'${c.id}': source.checkout (the sibling checkout dir name) missing`);
      if (!b.package || !b.bin) err(`'${c.id}': cargo build needs package and bin`);
    }
    if (b.kind === "download" && !src.version) err(`'${c.id}': download source needs version`);
    if (b.kind === "manual" && !(b.doc && fs.existsSync(path.join(root, b.doc.split("#")[0])))) err(`'${c.id}': manual build needs an existing build.doc`);
    if (b.script && !fs.existsSync(path.join(root, b.script))) err(`'${c.id}': build.script ${b.script} does not exist`);
    const arts = c.artifacts || {};
    if (Object.keys(arts).length === 0) err(`'${c.id}': no artifacts`);
    for (const [t, a] of Object.entries(arts)) {
      const has = ["sha256", "runtime_deps_asset"].filter((k) => a[k]);
      if (has.length !== 1) err(`'${c.id}' ${t}: exactly one of sha256 or runtime_deps_asset (got ${has.join(", ") || "none"})`);
      if (!a.evidence || !String(a.evidence).trim()) err(`'${c.id}' ${t}: evidence missing (where these bytes were built or shipped)`);
      try {
        pinnedDigest(m, pins, c.id, t);
      } catch (e) {
        err(e.message);
      }
      if (b.kind === "download") {
        if (!/^https:\/\//.test(a.url || "")) err(`'${c.id}' ${t}: download url must be https`);
        if (!HEX64.test(a.archive_sha256 || "")) err(`'${c.id}' ${t}: archive_sha256 missing or not 64 hex`);
        if (!a.member) err(`'${c.id}' ${t}: archive member path missing`);
        if (a.url && src.version && !a.url.includes(src.version)) err(`'${c.id}' ${t}: url does not name version ${src.version}`);
      }
    }
  }

  // Overlay coverage: every externalBin of every overlay is a component, and vice versa.
  const bins = overlayExternalBins(root);
  const byBin = new Map(comps.map((c) => [c.external_bin, c]));
  for (const [bin, files] of bins) if (!byBin.has(bin)) err(`externalBin ${bin} (${files.join(", ")}) has no component in release/components.json`);
  for (const c of comps) if (!bins.has(c.external_bin)) err(`'${c.id}': external_bin ${c.external_bin} is in no bundle overlay`);

  // Build scripts: each scripts/build-*.sh that builds a component delegates to the pinned engine
  // and never runs cargo or a download itself (the class of bug: building whatever HEAD a local
  // checkout is on, with no digest check).
  const scriptsDir = path.join(root, "scripts");
  const rel = (n) => `scripts/${n}`;
  const buildScripts = fs.existsSync(scriptsDir) ? fs.readdirSync(scriptsDir).filter((n) => /^build-.*\.sh$/.test(n)).map(rel) : [];
  if (!buildScripts.includes(ENGINE)) err(`${ENGINE} is missing`);
  for (const s of buildScripts) {
    if (NON_COMPONENT_SCRIPTS.has(s)) continue;
    const text = fs.readFileSync(path.join(root, s), "utf8");
    const code = text.split("\n").filter((l) => !/^\s*#/.test(l)).join("\n");
    const m2 = /build-component\.sh"?\s+([a-z0-9.-]+)/.exec(code);
    if (!m2) err(`${s} does not delegate to ${ENGINE} <id>`);
    else if (!ids.has(m2[1])) err(`${s} builds unknown component '${m2[1]}'`);
    else if ((component(m, m2[1]).build || {}).script !== s) err(`${s} builds '${m2[1]}' but the manifest names ${(component(m, m2[1]).build || {}).script} as its script`);
    if (/\bcargo\s+build\b/.test(code)) err(`${s} runs cargo build itself; only ${ENGINE} builds (from the pinned rev)`);
    if (/\b(curl|wget)\b/.test(code)) err(`${s} downloads itself; only ${ENGINE} fetches (and verifies) a component`);
    if (/rev-parse --short HEAD/.test(code)) err(`${s} records a checkout HEAD as provenance; the pinned rev is the provenance`);
  }
  for (const c of comps) {
    if (c.build && c.build.script && !c.build.script.startsWith("scripts/")) err(`'${c.id}': build.script must live in scripts/`);
  }

  // release.yml stages exactly the pinned macOS bytes: every runtime-deps asset it downloads that
  // is a sidecar is the manifest's aarch64-apple-darwin asset, and every component is staged.
  const wf = readIf(path.join(root, ".github", "workflows", "release.yml"));
  if (wf) {
    const assets = new Set(releaseAssets(wf));
    const T = "aarch64-apple-darwin";
    for (const c of comps) {
      const a = (c.artifacts || {})[T];
      if (!a) {
        err(`'${c.id}': no ${T} artifact, but release.yml builds the ${T} bundle`);
        continue;
      }
      if (a.runtime_deps_asset && !assets.has(a.runtime_deps_asset)) err(`release.yml does not stage '${c.id}' (${a.runtime_deps_asset})`);
      if (!a.runtime_deps_asset && !(c.build && c.build.kind === "download" && wf.includes(`build-component.sh ${c.id}`))) err(`release.yml does not stage '${c.id}' from a pinned source`);
    }
    const known = new Set([
      ...comps.flatMap((c) => Object.values(c.artifacts || {}).map((a) => a.runtime_deps_asset).filter(Boolean)),
      ...comps.flatMap((c) => c.runtime_deps_assets || []),
      ...(m.models || []).flatMap((x) => x.runtime_deps_assets || []),
      ...(m.other_runtime_deps || []),
    ]);
    for (const a of assets) if (!known.has(a)) err(`release.yml stages runtime-deps asset ${a}, which release/components.json does not name`);
    for (const a of known) if (!pins.has(a)) err(`runtime-deps asset ${a} (named in release/components.json) has no pin in src-tauri/runtime-deps.sha256`);
  }

  if (wf) for (const e of dryRunGuardErrors(wf)) err(e);

  // The node sidecar rev is the rev the app's own chain crates and build-sidecar.sh's
  // activation-hardening gate pin.
  const node = comps.find((c) => c.id === "citrate-node");
  if (node) {
    const sidecar = readIf(path.join(root, "scripts", "build-sidecar.sh")) || "";
    const min = /MIN_CHAIN_REV="([0-9a-f]{40})"/.exec(sidecar);
    if (!min) err("scripts/build-sidecar.sh has no MIN_CHAIN_REV");
    else if (min[1] !== node.source.rev) err(`citrate-node rev ${node.source.rev} != build-sidecar.sh MIN_CHAIN_REV ${min[1]}`);
    const cargo = readIf(path.join(root, "src-tauri", "Cargo.toml")) || "";
    for (const r of new Set([...cargo.matchAll(/citrate-chain\.git", rev = "([0-9a-f]{40})"/g)].map((x) => x[1]))) {
      if (r !== node.source.rev) err(`src-tauri/Cargo.toml pins citrate-chain ${r}, the bundled node is ${node.source.rev}`);
    }
  }

  // Models: the first-run Gemma download is pinned by the app (src-tauri/src/model.rs); the
  // manifest records the same digest and size and a revision-pinned URL.
  for (const x of m.models || []) {
    if (x.app_pin) {
      const src = readIf(path.join(root, x.app_pin));
      if (!src) err(`'${x.id}': app_pin ${x.app_pin} not found`);
      else {
        const sha = /pub const MODEL_SHA256: &str = "([0-9a-f]{64})"/.exec(src);
        const size = /pub const MODEL_SIZE_BYTES: u64 = ([0-9_]+);/.exec(src);
        if (!sha || sha[1] !== x.sha256) err(`'${x.id}': sha256 ${x.sha256} != ${x.app_pin} MODEL_SHA256 ${sha && sha[1]}`);
        if (!size || Number(size[1].replace(/_/g, "")) !== x.size) err(`'${x.id}': size ${x.size} != ${x.app_pin} MODEL_SIZE_BYTES ${size && size[1]}`);
      }
      if (!/^https:\/\/huggingface\.co\/[^/]+\/[^/]+\/resolve\/[0-9a-f]{40}\//.test(x.url || "")) err(`'${x.id}': url must be a Hugging Face resolve URL at a 40-hex revision (not a branch)`);
    }
    for (const a of x.runtime_deps_assets || []) if (!pins.has(a)) err(`'${x.id}': runtime-deps asset ${a} has no pin`);
  }

  // Licence inventory names every component (gate g3-licence) under the same id.
  const lic = readIf(path.join(root, "release", "licences.json"));
  if (lic) {
    const licIds = new Set((JSON.parse(lic).components || []).map((c) => c.id));
    for (const id of ids) if (!licIds.has(id)) err(`'${id}' has no entry in release/licences.json`);
  }
  return errors;
}

// ---------------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------------

function main(argv) {
  const [cmd, ...rest] = argv;
  const die = (msg) => {
    process.stderr.write(`component-pins: ${msg}\n`);
    process.exit(1);
  };
  try {
    if (cmd === "check") {
      const i = rest.indexOf("--root");
      const root = i >= 0 ? path.resolve(rest[i + 1]) : path.join(here, "..");
      const errors = check(root);
      if (errors.length) {
        for (const e of errors) process.stderr.write(`::error::component pins: ${e}\n`);
        process.exit(1);
      }
      process.stdout.write("component pins: OK\n");
      return;
    }
    const mfPath = defaultManifestPath();
    const m = loadManifest(mfPath);
    const root = path.dirname(path.dirname(path.resolve(mfPath)));
    if (cmd === "field") {
      const [id, dotted] = rest;
      if (!id || !dotted) die("usage: field <id> <dotted.path>");
      const v = id === "-" ? getField(m, dotted) : getField(component(m, id), dotted);
      if (Array.isArray(v)) process.stdout.write(v.join(" ") + "\n");
      else if (v && typeof v === "object") process.stdout.write(Object.entries(v).map(([k, x]) => `${k}=${x}`).join("\n") + (Object.keys(v).length ? "\n" : ""));
      else process.stdout.write(`${v}\n`);
      return;
    }
    if (cmd === "artifact") {
      const [id, target] = rest;
      const a = (component(m, id).artifacts || {})[target];
      if (!a) die(`no pinned artifact for '${id}' on ${target}`);
      process.stdout.write(JSON.stringify(a) + "\n");
      return;
    }
    if (cmd === "digest") {
      const [id, target] = rest;
      if (!id || !target) die("usage: digest <id> <target>");
      process.stdout.write(pinnedDigest(m, pinsFor(root), id, target) + "\n");
      return;
    }
    die("usage: component-pins.mjs field|artifact|digest|check ...");
  } catch (e) {
    die(e.message);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
