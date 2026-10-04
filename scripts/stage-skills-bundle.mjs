#!/usr/bin/env node
// HUP-S3.2: stage the reviewed third-party skills as an app resource (src-tauri/skills-bundle/).
//
// The Hermes sidecar loads this tree as a locked skill source (CITRATE_HERMES_SKILLS_LOCK +
// CITRATE_HERMES_SKILLS_THIRD_PARTY): `<out>/<source label>/<skill path>/SKILL.md` plus the refs
// skills.lock pins, and `<out>/skills.lock` itself. The sidecar checks every file against the
// lock again at load and on every read; this script makes sure a build never ships anything
// else.
//
//   node scripts/stage-skills-bundle.mjs build --sources-base <dir> [--out src-tauri/skills-bundle]
//       copy what skills.lock admits from the source checkouts (the same base as
//       scripts/skills-lock.mjs), each file checked against its pinned sha256, the recorded
//       intake rewrite applied and its shipped hash checked. Scripts are never copied.
//   node scripts/stage-skills-bundle.mjs verify <dir>
//       check a staged tree against this repo's skills.lock: every admitted skill present, every
//       file pinned and matching, no extra file, no symlink, the same skills.lock.
//   node scripts/stage-skills-bundle.mjs from-tarball <skills-bundle.tar.gz> [--out <dir>]
//       the release step: extract the runtime-deps asset to a scratch dir, verify it, copy it in.
//
// Exit 0 staged/verified, 1 refused, 2 usage. Zero dependencies (node:fs, node:path, node:crypto,
// node:child_process for `tar`).

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { INTAKE_REWRITE, flattenFrontmatter } from "./skill-intake-rewrite.mjs";

const SHIPS = new Set(["include-as-is", "include-with-scripts-stripped", "convert-script-to-capsule"]);
const KEEP = new Set(["README.md", ".gitignore"]);
const sha = (b) => createHash("sha256").update(b).digest("hex");

class Refused extends Error {}

/** Read the lock `scripts/skills-lock.mjs` renders. Strict: an unknown line is an error. */
export function parseLockToml(text) {
  const lock = { version: null, sources: [], skills: [] };
  let cur = null;
  let inRefs = false;
  const str = (raw, n) => {
    try {
      const v = JSON.parse(raw);
      if (typeof v !== "string") throw new Error();
      return v;
    } catch {
      throw new Refused(`skills.lock line ${n}: expected a string, got ${raw.slice(0, 40)}`);
    }
  };
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const n = i + 1;
    const l = lines[i];
    if (inRefs) {
      if (l === "]") {
        inRefs = false;
        continue;
      }
      const m = /^ {2}\{ path = ("(?:[^"\\]|\\.)*"), sha256 = ("[0-9a-f]{64}") \},$/.exec(l);
      if (!m) throw new Refused(`skills.lock line ${n}: not a ref entry`);
      cur.refs.push({ path: str(m[1], n), sha256: str(m[2], n) });
      continue;
    }
    if (l === "" || l.startsWith("#")) continue;
    if (l === "[[source]]") {
      cur = {};
      lock.sources.push(cur);
      continue;
    }
    if (l === "[[skill]]") {
      cur = { refs: [], stripped: [] };
      lock.skills.push(cur);
      continue;
    }
    if (l === "refs = [") {
      if (!cur) throw new Refused(`skills.lock line ${n}: refs outside a skill`);
      inRefs = true;
      continue;
    }
    if (l === "refs = []") continue;
    const kv = /^([a-z_0-9]+) = (.*)$/.exec(l);
    if (!kv) throw new Refused(`skills.lock line ${n}: cannot read ${JSON.stringify(l.slice(0, 40))}`);
    const [, k, v] = kv;
    if (k === "version") {
      lock.version = Number(v);
      continue;
    }
    if (!cur) throw new Refused(`skills.lock line ${n}: ${k} outside a table`);
    if (k === "stripped") {
      const arr = JSON.parse(v);
      if (!Array.isArray(arr) || !arr.every((x) => typeof x === "string")) {
        throw new Refused(`skills.lock line ${n}: stripped must be a list of strings`);
      }
      cur.stripped = arr;
      continue;
    }
    cur[k] = str(v, n);
  }
  if (lock.version !== 1) throw new Refused(`unsupported skills.lock version ${lock.version}`);
  for (const s of lock.skills) {
    if (s.verdict !== "exclude" && !SHIPS.has(s.verdict)) {
      throw new Refused(`skill ${s.name}: unknown verdict ${s.verdict}`);
    }
  }
  return lock;
}

const plainRel = (rel) =>
  typeof rel === "string" &&
  rel.length > 0 &&
  !rel.startsWith("/") &&
  !rel.includes("\\") &&
  rel.split("/").every((seg) => seg && seg !== "." && seg !== "..");

/** The files one admitted skill ships, relative to the bundle root, with the bytes' sha256. */
function shippedFiles(s) {
  if (!plainRel(s.source) || !plainRel(s.path)) throw new Refused(`skill ${s.name}: lock path is not plain`);
  const dir = `${s.source}/${s.path}`;
  const md = s.intake_rewrite ? s.shipped_skill_md_sha256 : s.skill_md_sha256;
  if (s.intake_rewrite && s.intake_rewrite !== INTAKE_REWRITE) {
    throw new Refused(`skill ${s.name}: unknown intake_rewrite ${s.intake_rewrite}`);
  }
  if (!/^[0-9a-f]{64}$/.test(md ?? "")) throw new Refused(`skill ${s.name}: no SKILL.md hash to ship`);
  const files = [{ rel: `${dir}/SKILL.md`, sha256: md }];
  for (const r of s.refs) {
    if (!plainRel(r.path) || r.path.startsWith("scripts/")) throw new Refused(`skill ${s.name}: ref ${r.path} not allowed`);
    files.push({ rel: `${dir}/${r.path}`, sha256: r.sha256 });
  }
  return files;
}

/** Read a regular file (never through a symlink). */
function readRegular(p) {
  const st = fs.lstatSync(p, { throwIfNoEntry: false });
  if (!st) throw new Refused(`${p} is missing`);
  if (st.isSymbolicLink()) throw new Refused(`${p} is a symlink`);
  if (!st.isFile()) throw new Refused(`${p} is not a regular file`);
  return fs.readFileSync(p);
}

/** Empty `out` except the committed README.md / .gitignore. */
function clearOut(out) {
  fs.mkdirSync(out, { recursive: true });
  for (const e of fs.readdirSync(out)) {
    if (!KEEP.has(e)) fs.rmSync(path.join(out, e), { recursive: true, force: true });
  }
}

function writeFile(out, rel, bytes) {
  const p = path.join(out, ...rel.split("/"));
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, bytes);
}

/**
 * Stage from the source checkouts. `sources` are the intake sources (`label` and `local`, the
 * checkout's path under `sourcesBase`). Returns { skills, files }. Throws Refused on any mismatch.
 */
export function stageFromSources(lockText, sourcesBase, out, sources) {
  const lock = parseLockToml(lockText);
  const local = new Map(sources.map((s) => [s.label, path.join(sourcesBase, s.local)]));
  const staged = [];
  for (const s of lock.skills.filter((x) => SHIPS.has(x.verdict))) {
    if (!local.has(s.source)) throw new Refused(`skill ${s.name}: source ${s.source} is not in the intake`);
    const root = path.join(local.get(s.source), ...s.path.split("/"));
    const files = shippedFiles(s);
    const upstreamMd = readRegular(path.join(root, "SKILL.md"));
    if (sha(upstreamMd) !== s.skill_md_sha256) {
      throw new Refused(`${s.source}/${s.path}/SKILL.md does not match skills.lock`);
    }
    let mdBytes = upstreamMd;
    if (s.intake_rewrite) {
      mdBytes = Buffer.from(flattenFrontmatter(upstreamMd.toString("utf8")), "utf8");
    }
    if (sha(mdBytes) !== files[0].sha256) {
      throw new Refused(`${s.source}/${s.path}/SKILL.md: the shipped bytes do not match skills.lock`);
    }
    staged.push({ rel: files[0].rel, bytes: mdBytes });
    for (const [i, r] of s.refs.entries()) {
      const bytes = readRegular(path.join(root, ...r.path.split("/")));
      if (sha(bytes) !== r.sha256) throw new Refused(`${s.source}/${s.path}/${r.path} does not match skills.lock`);
      staged.push({ rel: files[i + 1].rel, bytes });
    }
  }
  clearOut(out);
  for (const f of staged) writeFile(out, f.rel, f.bytes);
  fs.writeFileSync(path.join(out, "skills.lock"), lockText);
  const v = verifyStaged(lockText, out);
  if (!v.ok) throw new Refused(`staged tree does not verify:\n  ${v.problems.join("\n  ")}`);
  return { skills: lock.skills.filter((x) => SHIPS.has(x.verdict)).length, files: staged.length };
}

/** Check a staged tree against the lock. Never throws for a bad tree; lists every problem. */
export function verifyStaged(lockText, dir) {
  const problems = [];
  let lock;
  try {
    lock = parseLockToml(lockText);
  } catch (e) {
    return { ok: false, problems: [String(e.message)] };
  }
  const want = new Map();
  for (const s of lock.skills.filter((x) => SHIPS.has(x.verdict))) {
    try {
      for (const f of shippedFiles(s)) want.set(f.rel, f.sha256);
    } catch (e) {
      problems.push(String(e.message));
    }
  }
  const seen = new Set();
  const walk = (d, relBase) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const rel = relBase ? `${relBase}/${e.name}` : e.name;
      const p = path.join(d, e.name);
      if (e.isSymbolicLink()) {
        problems.push(`${rel} is a symlink`);
      } else if (e.isDirectory()) {
        walk(p, rel);
      } else if (!relBase && (KEEP.has(e.name) || e.name === "skills.lock")) {
        // The bundle's own files.
      } else if (!want.has(rel)) {
        problems.push(`${rel} is not pinned by skills.lock`);
      } else {
        seen.add(rel);
        if (sha(fs.readFileSync(p)) !== want.get(rel)) problems.push(`${rel} does not match skills.lock`);
      }
    }
  };
  if (!fs.existsSync(dir)) return { ok: false, problems: [`${dir} does not exist`] };
  walk(dir, "");
  for (const rel of want.keys()) if (!seen.has(rel)) problems.push(`${rel} is missing`);
  const stagedLock = path.join(dir, "skills.lock");
  if (!fs.existsSync(stagedLock) || fs.readFileSync(stagedLock, "utf8") !== lockText) {
    problems.push("skills.lock in the bundle is missing or differs from this repo's skills.lock");
  }
  return { ok: problems.length === 0, problems };
}

function arg(args, name) {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
}

function main() {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const lockText = fs.readFileSync(path.join(repoRoot, "skills.lock"), "utf8");
  const [cmd, ...rest] = process.argv.slice(2);
  const out = path.resolve(arg(rest, "--out") ?? path.join(repoRoot, "src-tauri/skills-bundle"));
  try {
    if (cmd === "build") {
      const base = arg(rest, "--sources-base");
      if (!base) throw Object.assign(new Error("build needs --sources-base <dir>"), { usage: true });
      const intake = JSON.parse(fs.readFileSync(path.join(repoRoot, ".agentile/skill-intake/intake.json"), "utf8"));
      const r = stageFromSources(lockText, path.resolve(base), out, intake.sources);
      console.log(`staged ${r.skills} reviewed skills (${r.files} files) into ${out}`);
    } else if (cmd === "verify") {
      const dir = rest[0];
      if (!dir) throw Object.assign(new Error("verify needs <dir>"), { usage: true });
      const v = verifyStaged(lockText, path.resolve(dir));
      if (!v.ok) throw new Refused(`refused:\n  ${v.problems.join("\n  ")}`);
      console.log(`${dir} matches skills.lock`);
    } else if (cmd === "from-tarball") {
      const tarball = rest[0];
      if (!tarball) throw Object.assign(new Error("from-tarball needs <file>"), { usage: true });
      const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "skills-bundle-"));
      try {
        execFileSync("tar", ["-xzf", path.resolve(tarball), "-C", scratch], { stdio: "inherit" });
        const v = verifyStaged(lockText, scratch);
        if (!v.ok) throw new Refused(`the asset does not match skills.lock:\n  ${v.problems.join("\n  ")}`);
        clearOut(out);
        fs.cpSync(scratch, out, { recursive: true, verbatimSymlinks: true });
        const again = verifyStaged(lockText, out);
        if (!again.ok) throw new Refused(`staged copy does not verify:\n  ${again.problems.join("\n  ")}`);
        console.log(`staged the reviewed skills from ${tarball} into ${out}`);
      } finally {
        fs.rmSync(scratch, { recursive: true, force: true });
      }
    } else {
      throw Object.assign(new Error("usage: stage-skills-bundle.mjs build|verify|from-tarball ..."), { usage: true });
    }
  } catch (e) {
    console.error(String(e?.message ?? e));
    process.exit(e?.usage ? 2 : 1);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
