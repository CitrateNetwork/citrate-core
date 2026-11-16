#!/usr/bin/env node
// created: 2026-11-15 | branch: release/054-bundle-completeness-gate
// author: Larry Klosowski (0.5.4 Hermes lane; Claude Opus 5.5)
// status: active
//
// 0.5.4 release gate (owner: proof that every daemon, sidecar, resource and prompt ships with the
// frontend). For the overlay being built (bundle-lite on macOS, bundle-linux on Linux), every
// `externalBin` and every `resources` glob must be staged before the build and present in what the
// build made. Fails closed.
//
//   node scripts/check-bundle.mjs staged   --overlay <conf> --target <triple>
//       before `tauri build`: every externalBin is a non-empty executable
//       src-tauri/<bin>-<triple>; every resources glob matches at least one non-empty file that is
//       not a README; a staged file whose name is pinned in src-tauri/runtime-deps.sha256 matches
//       one of its pins; skills-bundle/ matches this repo's skills.lock file for file (so a
//       re-pinned tarball needs no change here: the lock says what ships, no count is hard-coded);
//       the BGE embedder folder holds the three files the memory daemon loads.
//   node scripts/check-bundle.mjs app      --app <Citrate Core.app> --overlay <conf> --target <triple>
//   node scripts/check-bundle.mjs deb      --deb <file.deb>         --overlay <conf> --target <triple>
//   node scripts/check-bundle.mjs appimage --appimage <file>        --overlay <conf> --target <triple>
//       after the build: the staged check again, then every externalBin is an executable in the
//       bundle's binary folder (Contents/MacOS, usr/bin) and every staged resource is in its
//       resource folder (Contents/Resources, usr/lib/<productName>; `../` becomes `_up_/`) with
//       the staged bytes. Executable code (Mach-O, ELF) may differ (code signing, the AppImage's
//       RPATH patching rewrite it) but must be there and non-empty; nothing else may differ.
//       Read from the filesystem only: a .deb is unpacked with `ar` and `tar`, an AppImage's
//       squashfs with `unsquashfs` at the offset its ELF header gives. No bundled program is
//       ever run.
//
// Options: --src-tauri <dir> (default src-tauri), --pins <file> (default
// <src-tauri>/runtime-deps.sha256), --skills-lock <file> (default skills.lock beside src-tauri).
// Exit 0 complete, 1 refused, 2 usage. Zero dependencies.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { verifyStaged } from "./stage-skills-bundle.mjs";

const sha = (b) => createHash("sha256").update(b).digest("hex");
const NOT_REAL = new Set(["README.md", ".gitignore"]);
/** Files the embedder folder must hold (release.yml checks the same three). */
const REQUIRED_IN = { "models/bge-base-en-v1.5/*": ["config.json", "tokenizer.json", "model.safetensors"] };

const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));

/** The overlay's bundle lists: an overlay array replaces the base config's (tauri's merge patch). */
export function effectiveBundle({ srcTauri, overlay }) {
  const base = readJson(path.join(srcTauri, "tauri.conf.json"));
  const ov = overlay ? readJson(overlay) : {};
  const pick = (k) => ov.bundle?.[k] ?? base.bundle?.[k] ?? [];
  const resources = pick("resources");
  if (!Array.isArray(resources)) throw new Error("resources given as a map are not supported by this gate");
  return { externalBin: pick("externalBin"), resources, productName: ov.productName ?? base.productName };
}

function globRegex(rest) {
  let re = "";
  for (let i = 0; i < rest.length; i++) {
    const c = rest[i];
    if (c === "*" && rest[i + 1] === "*") {
      if (rest[i + 2] === "/") {
        re += "(?:.*/)?";
        i += 2;
      } else {
        re += ".*";
        i += 1;
      }
    } else if (c === "*") re += "[^/]*";
    else if (c === "?") re += "[^/]";
    else re += c.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${re}$`);
}

/** The regular files `pattern` (relative to `srcTauri`, may start with ../) matches, as written. */
export function expandGlob(srcTauri, pattern) {
  const segs = pattern.split("/");
  const lit = [];
  while (segs.length > 1 && !/[*?[]/.test(segs[0])) lit.push(segs.shift());
  const base = path.join(srcTauri, ...lit);
  const re = globRegex(segs.join("/"));
  const out = [];
  const walk = (dir, relBase) => {
    let ents;
    try {
      ents = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of ents) {
      const rel = relBase ? `${relBase}/${e.name}` : e.name;
      if (e.isDirectory()) walk(path.join(dir, e.name), rel);
      else if (e.isFile() && re.test(rel)) out.push([...lit, rel].join("/"));
    }
  };
  walk(base, "");
  return out.sort();
}

/** `../a/b` → `_up_/a/b`: where tauri puts a resource from outside src-tauri. */
const bundledPath = (rel) => rel.replace(/^(\.\.\/)+/, (m) => "_up_/".repeat(m.length / 3));

function readPins(file) {
  const pins = [];
  if (!file || !fs.existsSync(file)) return pins;
  for (const l of fs.readFileSync(file, "utf8").split("\n")) {
    const m = l.match(/^([0-9a-f]{64}) {2}(\S+)$/);
    if (m) pins.push({ hex: m[1], name: m[2] });
  }
  return pins;
}

/** Executable code (Mach-O or ELF): bundling may rewrite it (macOS code signing, the AppImage's
 *  RPATH patching), so its bytes are not compared; it must still be there and non-empty. */
const isCode = (b) => b.length >= 4 && ["cffaedfe", "cefaedfe", "cafebabe", "bebafeca", "feedfacf", "feedface", "7f454c46"].includes(b.subarray(0, 4).toString("hex"));

function defaults(o) {
  const srcTauri = path.resolve(o.srcTauri ?? "src-tauri");
  return {
    ...o,
    srcTauri,
    pins: o.pins ?? path.join(srcTauri, "runtime-deps.sha256"),
    skillsLock: o.skillsLock ?? path.join(srcTauri, "..", "skills.lock"),
  };
}

/** What the build must carry, from the staged tree. */
export function expectedFromStaged(o) {
  o = defaults(o);
  const b = effectiveBundle(o);
  const binaries = b.externalBin.map((e) => ({ entry: e, name: path.basename(e), staged: path.join(o.srcTauri, `${e}-${o.target}`) }));
  const resources = [];
  const globs = [];
  for (const g of b.resources) {
    const files = expandGlob(o.srcTauri, g);
    globs.push({ glob: g, files });
    for (const rel of files) resources.push({ rel, bundled: bundledPath(rel), staged: path.join(o.srcTauri, rel) });
  }
  return { binaries, resources, globs, productName: b.productName };
}

/** The staged inputs, before the build. */
export function checkStaged(o) {
  o = defaults(o);
  const exp = expectedFromStaged(o);
  const problems = [];
  const pinned = [];
  const pins = readPins(o.pins);
  const pinCheck = (rel, abs) => {
    const base = path.basename(abs);
    // A pin names this file when it is the same name, or (for a target-suffixed sidecar, staged as
    // <bin>-<triple>) the same name plus a build tag such as -<rev12> or -b10909-nowebui. A name with
    // a further extension (.PROVENANCE.txt, .tar.gz) is a different asset.
    const mine = pins.filter(
      (p) => p.name === base || (base.includes(o.target) && p.name.startsWith(`${base}-`) && !p.name.slice(base.length + 1).includes(".")),
    );
    if (mine.length === 0) return;
    pinned.push(rel);
    if (!mine.some((p) => p.hex === sha(fs.readFileSync(abs)))) {
      problems.push(`${rel} does not match its pin in runtime-deps.sha256 (${mine.map((p) => p.name).join(", ")})`);
    }
  };
  for (const bin of exp.binaries) {
    const rel = `${bin.entry}-${o.target}`;
    const st = fs.lstatSync(bin.staged, { throwIfNoEntry: false });
    if (!st || !st.isFile()) problems.push(`${rel} is missing`);
    else if (st.size === 0) problems.push(`${rel} is empty`);
    else if ((st.mode & 0o111) === 0) problems.push(`${rel} is not executable`);
    else pinCheck(rel, bin.staged);
  }
  for (const { glob, files } of exp.globs) {
    const real = files.filter((f) => !NOT_REAL.has(path.basename(f)));
    if (files.length === 0) problems.push(`${glob} matches no file`);
    else if (real.length === 0) problems.push(`${glob} matches no file but a README`);
    for (const rel of files) {
      const abs = path.join(o.srcTauri, rel);
      if (fs.statSync(abs).size === 0) problems.push(`${rel} is empty`);
      else if (!NOT_REAL.has(path.basename(rel))) pinCheck(rel, abs);
    }
    for (const need of REQUIRED_IN[glob] ?? []) {
      const dir = glob.slice(0, -2);
      if (!files.includes(`${dir}/${need}`)) problems.push(`${dir}/${need} is missing (${glob})`);
    }
    if (glob === "skills-bundle/**/*" && real.length > 0) {
      const v = verifyStaged(fs.readFileSync(o.skillsLock, "utf8"), path.join(o.srcTauri, "skills-bundle"));
      if (!v.ok) problems.push(`skills-bundle does not match skills.lock: ${v.problems.slice(0, 5).join("; ")}${v.problems.length > 5 ? ` (+${v.problems.length - 5} more)` : ""}`);
    }
  }
  return { problems, pinned, expected: exp };
}

/** A built bundle laid out as `binDir` (executables by bare name) and `resDir` (resources). */
export function checkLayout(o, binDir, resDir) {
  o = defaults(o);
  const exp = expectedFromStaged(o);
  const problems = [];
  const binLabel = path.basename(binDir);
  for (const bin of exp.binaries) {
    const p = path.join(binDir, bin.name);
    const st = fs.lstatSync(p, { throwIfNoEntry: false });
    if (!st || !st.isFile()) problems.push(`${binLabel}/${bin.name} is missing`);
    else if (st.size === 0) problems.push(`${binLabel}/${bin.name} is empty`);
    else if ((st.mode & 0o111) === 0) problems.push(`${binLabel}/${bin.name} is not executable`);
  }
  for (const r of exp.resources) {
    const p = path.join(resDir, r.bundled);
    const st = fs.lstatSync(p, { throwIfNoEntry: false });
    if (!st || !st.isFile()) {
      problems.push(`${r.bundled} is missing`);
      continue;
    }
    const staged = fs.readFileSync(r.staged);
    if (isCode(staged)) {
      if (st.size === 0) problems.push(`${r.bundled} is empty`);
      continue;
    }
    if (sha(fs.readFileSync(p)) !== sha(staged)) problems.push(`${r.bundled} differs from the staged file`);
  }
  return { problems, checked: { binaries: exp.binaries.length, resources: exp.resources.length } };
}

/** Where an AppImage's squashfs starts: the end of its ELF section headers. */
export function elfEnd(head) {
  if (head.length < 64 || head.subarray(0, 4).toString("latin1") !== "\x7fELF") throw new Error("not an ELF file");
  const le = head[5] === 1;
  const u16 = (off) => (le ? head.readUInt16LE(off) : head.readUInt16BE(off));
  if (head[4] === 2) {
    const shoff = Number(le ? head.readBigUInt64LE(0x28) : head.readBigUInt64BE(0x28));
    return shoff + u16(0x3a) * u16(0x3c);
  }
  const shoff = le ? head.readUInt32LE(0x20) : head.readUInt32BE(0x20);
  return shoff + u16(0x2e) * u16(0x30);
}

function arg(args, name) {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
}

function report(what, problems, ok) {
  if (problems.length) {
    console.error(`check-bundle: ${what} REFUSED:\n  ${problems.join("\n  ")}`);
    process.exit(1);
  }
  console.log(`check-bundle: ${what}: ${ok}`);
}

function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  const o = {
    srcTauri: arg(rest, "--src-tauri"),
    overlay: arg(rest, "--overlay"),
    target: arg(rest, "--target"),
    pins: arg(rest, "--pins"),
    skillsLock: arg(rest, "--skills-lock"),
  };
  const usage = () => {
    console.error("usage: check-bundle.mjs staged|app|deb|appimage --overlay <conf> --target <triple> [--app|--deb|--appimage <file>] (see the header)");
    process.exit(2);
  };
  if (!o.overlay || !o.target) usage();
  const staged = checkStaged(o);
  const summary = `${staged.expected.binaries.length} sidecars, ${staged.expected.resources.length} resource files from ${staged.expected.globs.length} globs, ${staged.pinned.length} pinned`;
  if (cmd === "staged") return report("staged inputs", staged.problems, summary);
  if (staged.problems.length) return report("staged inputs (the reference for the bundle)", staged.problems, "");
  let scratch = null;
  let result;
  try {
    result = inspect(cmd, rest, o, staged, usage, (d) => (scratch = d));
  } finally {
    if (scratch) fs.rmSync(scratch, { recursive: true, force: true });
  }
  report(result.what, result.problems, `${result.ok} (${summary})`);
}

/** Unpack (when needed) and check one built bundle. */
function inspect(cmd, rest, o, staged, usage, keepScratch) {
  let binDir;
  let resDir;
  if (cmd === "app") {
    const app = arg(rest, "--app");
    if (!app) usage();
    binDir = path.join(app, "Contents", "MacOS");
    resDir = path.join(app, "Contents", "Resources");
  } else if (cmd === "deb" || cmd === "appimage") {
    const file = arg(rest, `--${cmd}`);
    if (!file) usage();
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), `check-bundle-${cmd}-`));
    keepScratch(scratch);
    const root = path.join(scratch, "root");
    if (cmd === "deb") {
      const ar = path.join(scratch, "ar");
      fs.mkdirSync(ar);
      fs.mkdirSync(root);
      execFileSync("ar", ["x", path.resolve(file)], { cwd: ar });
      const data = fs.readdirSync(ar).find((n) => n.startsWith("data.tar"));
      if (!data) return { what: cmd, problems: [`${file} has no data.tar member`], ok: "" };
      execFileSync("tar", ["-xf", path.join(ar, data), "-C", root]);
    } else {
      const fd = fs.openSync(path.resolve(file), "r");
      const head = Buffer.alloc(64);
      fs.readSync(fd, head, 0, 64, 0);
      fs.closeSync(fd);
      execFileSync("unsquashfs", ["-q", "-n", "-o", String(elfEnd(head)), "-d", root, path.resolve(file)], { stdio: ["ignore", "ignore", "inherit"] });
    }
    binDir = path.join(root, "usr", "bin");
    resDir = path.join(root, "usr", "lib", staged.expected.productName);
  } else usage();
  const r = checkLayout(o, binDir, resDir);
  return { what: `${cmd} bundle`, problems: r.problems, ok: `${r.checked.binaries} sidecars and ${r.checked.resources} resource files present` };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
