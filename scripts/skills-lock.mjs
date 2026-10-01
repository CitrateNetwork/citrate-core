#!/usr/bin/env node
// HUP-S3.6 — reproducible generator and drift check for skills.lock.
//
// skills.lock pins every third-party skill reviewed for the bundled corpus: where it came from
// (upstream + full commit), where it sits in that source, the sha256 of its SKILL.md and of each
// bundled file that ships with it, and the review verdict. Skill contents are never copied into
// this repo; the lock holds only paths and hashes.
//
// The frontmatter and file-listing rules are a port of the runtime loader
// (citrate-agent-runtime agent-loop/src/skills.rs, HUP-S3.2), so "valid here" means "the loader
// would accept it". Scripts are never executed or hashed as included files: a skill that bundles
// scripts ships with them stripped, or its script becomes a named capsule, or it is excluded.
//
// Usage:
//   node scripts/skills-lock.mjs            write skills.lock
//   node scripts/skills-lock.mjs --check    recompute and fail (exit 1) on any drift
//   node scripts/skills-lock.mjs --table    print the review summary as markdown
//   --sources-base <dir> | SKILLS_SOURCES_BASE   where the source checkouts live
//                                                 (default: the directory above this repo)
//
// Zero dependencies: node:fs, node:path, node:crypto only.

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------------------------
// Loader limits (mirror agent-loop/src/skills.rs)
// ---------------------------------------------------------------------------------------------

export const MAX_SKILL_FILE_BYTES = 64 * 1024;
export const MAX_NAME_LEN = 64;
export const MAX_DESCRIPTION_LEN = 1024;
export const MAX_COMPATIBILITY_LEN = 500;
export const MAX_VALUE_LEN = 1024;
export const MAX_REF_BYTES = 128 * 1024;
export const MAX_SCAN_DEPTH = 6;
export const MAX_SCAN_DIRS = 4096;
export const MAX_FILES_LISTED = 64;
export const MAX_RESOURCE_DEPTH = 4;

const EXTENSION_KEYS = ["argument-hint", "disable-model-invocation", "user-invocable", "model", "type", "version"];

export const VERDICTS = [
  "include-as-is",
  "include-with-scripts-stripped",
  "convert-script-to-capsule",
  "exclude",
];

export class SkillError extends Error {
  constructor(kind, message) {
    super(message);
    this.kind = kind;
  }
}

const yamlErr = (line, msg) => new SkillError("Yaml", `frontmatter line ${line}: ${msg}`);
const chars = (s) => [...s].length;
const trimRust = (s) => s.replace(/^\s+|\s+$/gu, "");

function validKey(k) {
  return k.length > 0 && k.length <= 64 && /^[A-Za-z0-9_-]+$/.test(k);
}

function scalar(raw, line) {
  const s = trimRust(raw);
  if (s.startsWith('"')) {
    const rest = s.slice(1);
    if (!rest.endsWith('"')) throw yamlErr(line, "unterminated double-quoted string");
    const inner = rest.slice(0, -1);
    let out = "";
    const cs = [...inner];
    for (let i = 0; i < cs.length; i++) {
      const c = cs[i];
      if (c === "\\") {
        const n = cs[++i];
        if (n === undefined) throw yamlErr(line, "dangling escape");
        const map = { '"': '"', "\\": "\\", "/": "/", n: "\n", t: "\t" };
        if (!(n in map)) throw yamlErr(line, `unsupported escape \\${n}`);
        out += map[n];
      } else if (c === '"') {
        throw yamlErr(line, "unescaped quote inside a string");
      } else {
        out += c;
      }
    }
    return out;
  }
  if (s.startsWith("'")) {
    const rest = s.slice(1);
    if (!rest.endsWith("'")) throw yamlErr(line, "unterminated single-quoted string");
    const inner = rest.slice(0, -1);
    if (inner.replaceAll("''", "").includes("'")) throw yamlErr(line, "unescaped quote inside a string");
    return inner.replaceAll("''", "'");
  }
  const first = s[0];
  if (first === "&") throw yamlErr(line, "anchors are not supported");
  if (first === "*") throw yamlErr(line, "aliases are not supported");
  if (first === "!") throw yamlErr(line, "tags are not supported");
  if (first === "{") throw yamlErr(line, "flow maps are not supported");
  if (first === "@" || first === "`") throw yamlErr(line, "reserved indicator");
  if (first === "%") throw yamlErr(line, "directives are not supported");
  const i = s.indexOf(" #");
  return i >= 0 ? s.slice(0, i).replace(/\s+$/u, "") : s;
}

function flowList(raw, line) {
  const s = trimRust(raw);
  if (!(s.startsWith("[") && s.endsWith("]") && s.length >= 2)) throw yamlErr(line, "unterminated flow list");
  const inner = s.slice(1, -1);
  if (inner.includes("[") || inner.includes("{")) throw yamlErr(line, "nested collections are not supported");
  return inner
    .split(",")
    .map(trimRust)
    .filter((p) => p.length > 0)
    .map((p) => scalar(p, line));
}

const indentOf = (l) => l.length - l.replace(/^ +/, "").length;
const isBlank = (l) => trimRust(l).length === 0;

function parseFrontmatter(lines) {
  const out = [];
  let i = 0;
  while (i < lines.length) {
    const ln = i + 1;
    const l = lines[i];
    if (l.includes("\t") && l.replace(/^\s+/u, "").length !== l.length) {
      throw yamlErr(ln, "tab indentation is not supported");
    }
    const t = trimRust(l);
    if (t.length === 0 || t.startsWith("#")) {
      i++;
      continue;
    }
    if (indentOf(l) > 0) throw yamlErr(ln, "unexpected indentation");
    if (t === "---" || t === "...") throw yamlErr(ln, "multiple documents are not supported");
    const ci = t.indexOf(":");
    if (ci < 0) throw yamlErr(ln, "expected `key: value`");
    const key = trimRust(t.slice(0, ci));
    let rest = t.slice(ci + 1);
    if (!validKey(key)) throw yamlErr(ln, `invalid key ${JSON.stringify(key)}`);
    if (rest.length > 0 && !rest.startsWith(" ")) throw yamlErr(ln, "expected a space after ':'");
    rest = trimRust(rest);
    i++;
    const start = i;
    while (i < lines.length && (isBlank(lines[i]) || indentOf(lines[i]) > 0)) i++;
    const block = lines.slice(start, i);
    const blockHasContent = block.some((b) => !isBlank(b));
    let value;
    if ([">", ">-", "|", "|-"].includes(rest)) {
      const firstNonBlank = block.find((b) => !isBlank(b));
      const base = firstNonBlank === undefined ? 0 : indentOf(firstNonBlank);
      const parts = [];
      block.forEach((b, j) => {
        if (b.includes("\t")) throw yamlErr(start + j + 1, "tabs are not supported");
        if (isBlank(b)) parts.push("");
        else if (indentOf(b) < base) throw yamlErr(start + j + 1, "block scalar indentation");
        else parts.push(b.slice(base).replace(/\s+$/u, ""));
      });
      while (parts.length && parts[parts.length - 1] === "") parts.pop();
      let joined;
      if (rest.startsWith("|")) joined = parts.join("\n");
      else {
        joined = "";
        for (const p of parts) {
          if (p === "") joined += "\n";
          else {
            if (joined.length && !joined.endsWith("\n")) joined += " ";
            joined += p;
          }
        }
      }
      value = { t: "str", v: joined };
    } else if (rest === "" && blockHasContent) {
      const items = block
        .map((b, j) => [start + j + 1, b])
        .filter(([, b]) => !isBlank(b));
      const base = indentOf(items[0][1]);
      if (items.some(([, b]) => indentOf(b) !== base)) throw yamlErr(items[0][0], "nested collections are not supported");
      const f = items[0][1].replace(/^\s+/u, "");
      if (f.startsWith("- ") || trimRust(items[0][1]) === "-") {
        const list = [];
        for (const [n, b] of items) {
          const tb = trimRust(b);
          if (!tb.startsWith("-")) throw yamlErr(n, "mixed list and map");
          const item = trimRust(tb.slice(1));
          if (item.includes(": ") || item.endsWith(":")) throw yamlErr(n, "nested collections are not supported");
          list.push(scalar(item, n));
        }
        value = { t: "list", v: list };
      } else {
        const map = {};
        for (const [n, b] of items) {
          const tb = trimRust(b);
          const mi = tb.indexOf(":");
          if (mi < 0) throw yamlErr(n, "expected `key: value`");
          const mk = trimRust(tb.slice(0, mi));
          const mv = tb.slice(mi + 1);
          if (!validKey(mk)) throw yamlErr(n, `invalid key ${JSON.stringify(mk)}`);
          if (trimRust(mv).length === 0) throw yamlErr(n, "nested collections are not supported");
          const v = scalar(mv, n);
          if (Object.prototype.hasOwnProperty.call(map, mk)) throw new SkillError("DuplicateKey", `frontmatter key '${key}.${mk}' appears twice`);
          map[mk] = v;
        }
        value = { t: "map", v: map };
      }
    } else if (rest === "") {
      value = { t: "str", v: "" };
    } else if (blockHasContent) {
      throw yamlErr(start + 1, `multi-line plain values are not supported for ${JSON.stringify(key)}`);
    } else if (rest.startsWith("[")) {
      value = { t: "list", v: flowList(rest, ln) };
    } else {
      value = { t: "str", v: scalar(rest, ln) };
    }
    if (out.some(([k]) => k === key)) throw new SkillError("DuplicateKey", `frontmatter key '${key}' appears twice`);
    out.push([key, value, ln]);
  }
  return out;
}

export function validSkillName(n) {
  return (
    n.length > 0 &&
    n.length <= MAX_NAME_LEN &&
    /^[a-z0-9-]+$/.test(n) &&
    !n.startsWith("-") &&
    !n.endsWith("-") &&
    !n.includes("--")
  );
}

function strField(v, field, line) {
  if (v.t !== "str") throw yamlErr(line, `'${field}' must be a string`);
  return v.v;
}

function capped(s, field, max) {
  if (chars(s) > max) throw new SkillError("TooLong", `field '${field}' is longer than ${max} characters`);
  return s;
}

/** Parse and validate a whole SKILL.md the way the runtime loader does. Throws SkillError. */
export function parseSkillMd(input) {
  if (Buffer.byteLength(input, "utf8") > MAX_SKILL_FILE_BYTES) {
    throw new SkillError("TooLarge", `SKILL.md is larger than ${MAX_SKILL_FILE_BYTES} bytes`);
  }
  let text = input.startsWith("﻿") ? input.slice(1) : input;
  if (text.includes("\r")) text = text.replaceAll("\r\n", "\n");
  if (!text.startsWith("---\n")) throw new SkillError("NoFrontmatter", "no YAML frontmatter (expected a leading ---)");
  const after = text.slice(4);
  let offset = 0;
  let close = null;
  for (const line of after.split(/(?<=\n)/)) {
    if (line.replace(/\n$/, "").replace(/\s+$/u, "") === "---") {
      close = [offset, offset + line.length];
      break;
    }
    offset += line.length;
  }
  if (!close) throw new SkillError("Unterminated", "frontmatter has no closing ---");
  const fmText = after.slice(0, close[0]);
  const body = after.slice(close[1]);
  // Rust str::lines: split on \n, drop one trailing empty piece.
  const lines = fmText.length === 0 ? [] : fmText.replace(/\n$/, "").split("\n");
  const fm = { name: "", description: "", license: null, compatibility: null, metadata: {}, allowedTools: [], extensions: {} };
  let haveName = false;
  let haveDesc = false;
  for (const [key, value, line] of parseFrontmatter(lines)) {
    if (key === "name") {
      const n = trimRust(strField(value, "name", line));
      if (!n) continue;
      if (!validSkillName(n)) throw new SkillError("InvalidName", `invalid name ${JSON.stringify(n)}`);
      fm.name = n;
      haveName = true;
    } else if (key === "description") {
      const d = trimRust(strField(value, "description", line));
      if (!d) continue;
      fm.description = capped(d, "description", MAX_DESCRIPTION_LEN);
      haveDesc = true;
    } else if (key === "license") {
      fm.license = capped(trimRust(strField(value, "license", line)), "license", MAX_VALUE_LEN);
    } else if (key === "compatibility") {
      fm.compatibility = capped(trimRust(strField(value, "compatibility", line)), "compatibility", MAX_COMPATIBILITY_LEN);
    } else if (key === "metadata") {
      if (value.t === "map") {
        for (const [k, v] of Object.entries(value.v)) fm.metadata[k] = capped(v, "metadata", MAX_VALUE_LEN);
      } else if (!(value.t === "str" && value.v === "")) {
        throw yamlErr(line, "'metadata' must be a map of strings");
      }
    } else if (key === "allowed-tools") {
      if (value.t === "str") fm.allowedTools = value.v.split(/\s+/u).filter(Boolean);
      else if (value.t === "list") fm.allowedTools = value.v;
      else throw yamlErr(line, "'allowed-tools' must be a string or list");
    } else if (EXTENSION_KEYS.includes(key)) {
      let v;
      if (value.t === "str") v = value.v;
      else if (value.t === "list") v = value.v.join(", ");
      else throw yamlErr(line, `'${key}' must be a string or list`);
      fm.extensions[key] = capped(v, "extension", MAX_VALUE_LEN);
    } else {
      throw new SkillError("UnknownKey", `unknown frontmatter key '${key}'`);
    }
  }
  if (!haveName) throw new SkillError("MissingField", "required field 'name' is missing or empty");
  if (!haveDesc) throw new SkillError("MissingField", "required field 'description' is missing or empty");
  return { fm, body };
}

// ---------------------------------------------------------------------------------------------
// Discovery and file listing (mirror the loader)
// ---------------------------------------------------------------------------------------------

const isHidden = (p) => path.basename(p).startsWith(".");
const byteSort = (a, b) => (Buffer.compare(Buffer.from(a), Buffer.from(b)));

function sortedEntries(dir) {
  try {
    return fs.readdirSync(dir).map((n) => path.join(dir, n)).sort(byteSort);
  } catch {
    return [];
  }
}

function isDirFollow(p) {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

function isFileFollow(p) {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

/** Every directory under root holding a SKILL.md (not descending into a skill). */
export function discoverSkills(root) {
  const found = [];
  const notes = [];
  let visited = 0;
  const stack = [[root, 0]];
  while (stack.length) {
    const [dir, depth] = stack.pop();
    visited++;
    if (visited > MAX_SCAN_DIRS) {
      notes.push(`scan stopped after ${MAX_SCAN_DIRS} directories`);
      break;
    }
    if (isFileFollow(path.join(dir, "SKILL.md"))) {
      found.push(dir);
      continue;
    }
    if (depth >= MAX_SCAN_DEPTH) continue;
    for (const child of sortedEntries(dir).reverse()) {
      if (!isHidden(child) && isDirFollow(child)) stack.push([child, depth + 1]);
    }
  }
  return { found, notes };
}

/** A skill's bundled files: [refs, scripts, truncated]. */
export function listResources(dir) {
  const canonDir = fs.realpathSync(dir);
  const refs = [];
  const scripts = [];
  let truncated = false;
  const stack = [[canonDir, 0]];
  while (stack.length) {
    const [d, depth] = stack.pop();
    for (const entry of sortedEntries(d)) {
      if (refs.length + scripts.length >= MAX_FILES_LISTED) {
        truncated = true;
        break;
      }
      if (isHidden(entry)) continue;
      let meta;
      try {
        meta = fs.lstatSync(entry);
      } catch {
        continue;
      }
      let isFile;
      if (meta.isSymbolicLink()) {
        let target;
        try {
          target = fs.realpathSync(entry);
        } catch {
          continue;
        }
        if ((target === canonDir || target.startsWith(canonDir + path.sep)) && isFileFollow(target)) isFile = true;
        else continue;
      } else if (meta.isDirectory()) {
        if (depth + 1 < MAX_RESOURCE_DEPTH) stack.push([entry, depth + 1]);
        continue;
      } else {
        isFile = meta.isFile();
      }
      if (!isFile) continue;
      const rel = path.relative(canonDir, entry).split(path.sep).join("/");
      if (rel === "SKILL.md") continue;
      (rel.startsWith("scripts/") ? scripts : refs).push(rel);
    }
  }
  refs.sort(byteSort);
  scripts.sort(byteSort);
  return { canonDir, refs, scripts, truncated };
}

// ---------------------------------------------------------------------------------------------
// Content scan (heuristic; every hit needs a recorded human decision)
// ---------------------------------------------------------------------------------------------

/** Patterns that must never pass without a recorded review decision. */
const RED_FLAGS = [
  ["pipe-to-shell", /\b(curl|wget)\b[^\n|]*\|\s*(sudo\s+)?(ba|z)?sh\b|\biex\s*\(\s*(irm|iwr|invoke-)|\b(irm|iwr|invoke-(restmethod|webrequest))\b[^\n|]*\|\s*(iex|invoke-expression)\b/i],
  ["safety-disable", /ignore (all |any )?(previous|prior|above) instructions|disable (the )?(safety|sandbox|approvals?)|--dangerously[-\w]*|bypass[-_ ]?permissions|skip[-_ ]?permissions|--yolo\b/i],
  ["exfil-endpoint", /webhook\.site|requestbin|pipedream\.net|ngrok\.io|pastebin\.com|interactsh|burpcollaborator|oast\.(fun|pro|live)/i],
  ["credential-path", /~\/\.ssh\/|id_(rsa|ed25519)\b|\.aws\/credentials|\.netrc\b|security find-(generic|internet)-password|keychain dump|\.git-credentials|seed phrase|mnemonic phrase/i],
];

/** Patterns that are recorded for the reviewer but do not block on their own. */
const NOTES = [
  ["network-install", /\b(pip3? install|npm (i|install)|npx |pnpm (add|dlx)|uvx |brew install|cargo install|go install|git clone)\b/],
  ["remote-fetch", /\b(curl|wget)\s+[^\n]*https?:\/\//],
  ["shell-blocks", /```(bash|sh|shell|zsh|console|powershell)\b/],
  ["destructive-shell", /\brm -rf\b|\bsudo\b|\bchmod \+x\b|\bgit push --force\b/],
  ["secret-env", /\b[A-Z][A-Z0-9_]*(API_KEY|SECRET|TOKEN|PASSWORD)\b/],
];

/** Network or process primitives inside bundled scripts. */
const SCRIPT_NET = /\b(requests\.(get|post|put|delete|request|Session)|urllib\.request|urllib3|http\.client|httpx|aiohttp|socket\.(socket|create_connection)|fetch\(|axios|XMLHttpRequest|WebSocket\(|curl\b|wget\b|Invoke-WebRequest|net\/http|reqwest)/;
const SCRIPT_PROC = /\b(subprocess\.|os\.system|os\.popen|child_process|execSync|spawn\(|eval\(|exec\()/;

function scan(text, rules) {
  return rules.filter(([, re]) => re.test(text)).map(([k]) => k);
}

const readText = (p) => {
  try {
    const b = fs.readFileSync(p);
    return b.includes(0) ? null : b.toString("utf8");
  } catch {
    return null;
  }
};

/** Executable files that sit outside `scripts/` (the loader lists them as readable refs). The
 * intake treats them exactly like scripts: never shipped, never hashed as included files. */
const EXEC_EXT = /\.(sh|bash|zsh|py|js|mjs|cjs|ts|ps1|rb|pl)$/;
export function isExecutableRef(rel, absPath) {
  if (EXEC_EXT.test(rel) || /(^|\/)Dockerfile$/.test(rel)) return true;
  try {
    const fd = fs.openSync(absPath, "r");
    const b = Buffer.alloc(2);
    fs.readSync(fd, b, 0, 2, 0);
    fs.closeSync(fd);
    return b.toString("latin1") === "#!";
  } catch {
    return false;
  }
}

const sha256File = (p) => createHash("sha256").update(fs.readFileSync(p)).digest("hex");

// ---------------------------------------------------------------------------------------------
// Lock building
// ---------------------------------------------------------------------------------------------

export function loadIntake(file) {
  const intake = JSON.parse(fs.readFileSync(file, "utf8"));
  for (const s of intake.sources) {
    for (const k of ["label", "upstream", "commit", "license", "local", "pin_method"]) {
      if (typeof s[k] !== "string" || !s[k]) throw new Error(`intake source missing '${k}'`);
    }
    if (!/^[0-9a-f]{40}$/.test(s.commit)) throw new Error(`source ${s.label}: commit must be a full sha`);
  }
  intake.decisions ??= {};
  return intake;
}

export function resolveSourcesBase(repoRoot, argv = process.argv) {
  const i = argv.indexOf("--sources-base");
  if (i >= 0 && argv[i + 1]) return path.resolve(argv[i + 1]);
  if (process.env.SKILLS_SOURCES_BASE) return path.resolve(process.env.SKILLS_SOURCES_BASE);
  return path.resolve(repoRoot, "..");
}

/** Review one skill directory. */
export function reviewSkill(src, root, dir) {
  const rel = path.relative(root, dir).split(path.sep).join("/");
  const key = `${src.label}/${rel}`;
  const skillMd = path.join(dir, "SKILL.md");
  const sk = {
    key,
    source: src.label,
    commit: src.commit,
    path: rel,
    dirName: path.basename(dir),
    name: path.basename(dir),
    skillMdSha256: sha256File(skillMd),
    skillMdBytes: fs.statSync(skillMd).size,
    valid: true,
    invalidReason: null,
    license: null,
    refs: [],
    allRefs: [],
    scripts: [],
    execRefs: [],
    truncated: false,
    oversizedRefs: [],
    flags: new Set(),
    notes: new Set(),
    scriptNet: [],
    scriptProc: [],
  };
  const text = readText(skillMd);
  try {
    if (text === null) throw new SkillError("NotUtf8", "SKILL.md is not UTF-8 text");
    const { fm, body } = parseSkillMd(text);
    if (fm.name !== sk.dirName) throw new SkillError("Name", `name '${fm.name}' does not match its directory '${sk.dirName}'`);
    sk.name = fm.name;
    sk.license = fm.license;
    for (const f of scan(body, RED_FLAGS)) sk.flags.add(f);
    for (const f of scan(body, NOTES)) sk.notes.add(f);
  } catch (e) {
    if (!(e instanceof SkillError)) throw e;
    sk.valid = false;
    sk.invalidReason = e.message;
    if (text !== null) for (const f of scan(text, RED_FLAGS)) sk.flags.add(f);
  }
  const res = listResources(dir);
  sk.truncated = res.truncated;
  sk.scripts = res.scripts;
  sk.execRefs = res.refs.filter((r) => isExecutableRef(r, path.join(res.canonDir, r)));
  for (const r of res.refs) {
    const p = path.join(res.canonDir, r);
    const size = fs.statSync(p).size;
    if (size > MAX_REF_BYTES) sk.oversizedRefs.push(r);
    sk.allRefs.push({ path: r, sha256: sha256File(p) });
    const t = readText(p);
    if (t !== null) {
      for (const f of scan(t, RED_FLAGS)) sk.flags.add(`${f} (in ${r})`);
      for (const f of scan(t, NOTES)) sk.notes.add(f);
    }
  }
  for (const s of [...res.scripts, ...sk.execRefs]) {
    const t = readText(path.join(res.canonDir, s));
    if (t === null) continue;
    if (SCRIPT_NET.test(t)) sk.scriptNet.push(s);
    if (SCRIPT_PROC.test(t)) sk.scriptProc.push(s);
    for (const f of scan(t, RED_FLAGS)) sk.flags.add(`${f} (in ${s})`);
  }
  return sk;
}

function defaultVerdict(sk) {
  if (!sk.valid) return { verdict: "exclude", reason: `loader would refuse it: ${sk.invalidReason}` };
  if (sk.scripts.length || sk.execRefs.length) {
    return { verdict: "include-with-scripts-stripped", reason: "ships without its scripts and executable files" };
  }
  return { verdict: "include-as-is", reason: "" };
}

/** Review every source and apply the recorded decisions. Throws on an unreviewed red flag. */
export function buildLock(intake, sourcesBase) {
  const skills = [];
  const problems = [];
  const seen = new Set();
  for (const src of intake.sources) {
    const root = path.join(sourcesBase, src.local);
    if (!isDirFollow(root)) throw new Error(`source '${src.label}': ${root} is not a directory`);
    const canonRoot = fs.realpathSync(root);
    const { found } = discoverSkills(canonRoot);
    for (const dir of found) {
      const sk = reviewSkill(src, canonRoot, dir);
      seen.add(sk.key);
      const d = intake.decisions[sk.key];
      const auto = defaultVerdict(sk);
      if (d) {
        if (!VERDICTS.includes(d.verdict)) problems.push(`${sk.key}: unknown verdict '${d.verdict}'`);
        if (!d.reason) problems.push(`${sk.key}: a recorded decision needs a reason`);
        if (d.verdict === "convert-script-to-capsule" && !/^[a-z0-9-]+$/.test(d.capsule ?? "")) {
          problems.push(`${sk.key}: convert-script-to-capsule needs a capsule name`);
        }
        if (d.verdict !== "exclude" && !sk.valid) problems.push(`${sk.key}: cannot include a skill the loader refuses (${sk.invalidReason})`);
        if (d.verdict === "include-as-is" && (sk.scripts.length || sk.execRefs.length)) {
          problems.push(`${sk.key}: include-as-is but it bundles scripts or executable files`);
        }
        sk.verdict = d.verdict;
        sk.reason = d.reason;
        sk.capsule = d.capsule ?? null;
        sk.reviewed = true;
      } else {
        // A skill the loader refuses is excluded by default, so its flags cannot reach a bundle;
        // they are still reported in the review table.
        if (sk.flags.size && auto.verdict !== "exclude") {
          problems.push(`${sk.key}: unreviewed red flags: ${[...sk.flags].join(", ")}`);
        }
        sk.verdict = auto.verdict;
        sk.reason = auto.reason;
        sk.capsule = null;
        sk.reviewed = false;
      }
      const exec = new Set(sk.execRefs);
      sk.refs = sk.verdict === "exclude" ? [] : sk.allRefs.filter((r) => !exec.has(r.path));
      sk.stripped = sk.verdict === "exclude" ? [] : [...sk.scripts, ...sk.execRefs].sort(byteSort);
      skills.push(sk);
    }
  }
  for (const k of Object.keys(intake.decisions)) {
    if (!seen.has(k)) problems.push(`decision for ${k}: no such skill in the sources (stale review)`);
  }
  if (problems.length) throw new Error(`skills intake is not clean:\n  ${problems.join("\n  ")}`);
  // Cross-source name collisions among shipped skills: the loader keeps the first source.
  const byName = new Map();
  for (const sk of skills) {
    if (sk.verdict === "exclude") continue;
    if (!byName.has(sk.name)) byName.set(sk.name, []);
    byName.get(sk.name).push(sk.key);
  }
  const collisions = [...byName.entries()].filter(([, v]) => v.length > 1);
  return { sources: intake.sources, skills, collisions };
}

const q = (s) => JSON.stringify(s);

/** Deterministic TOML. */
export function renderLock(lock) {
  const out = [];
  out.push("# skills.lock: third-party skills reviewed for the bundled Hermes corpus (HUP-S3.6).");
  out.push("# Generated by scripts/skills-lock.mjs from .agentile/skill-intake/intake.json. Do not edit by hand;");
  out.push("# rerun the script. `node scripts/skills-lock.mjs --check` fails on any drift.");
  out.push("# Paths are relative to the source root. Skill contents are not copied into this repo.");
  out.push("");
  out.push("version = 1");
  out.push("");
  for (const s of lock.sources) {
    out.push("[[source]]");
    out.push(`label = ${q(s.label)}`);
    out.push(`upstream = ${q(s.upstream)}`);
    out.push(`commit = ${q(s.commit)}`);
    out.push(`license = ${q(s.license)}`);
    out.push(`local = ${q(s.local)}`);
    out.push(`pin_method = ${q(s.pin_method)}`);
    out.push("");
  }
  for (const sk of lock.skills) {
    out.push("[[skill]]");
    out.push(`name = ${q(sk.name)}`);
    out.push(`source = ${q(sk.source)}`);
    out.push(`commit = ${q(sk.commit)}`);
    out.push(`path = ${q(sk.path)}`);
    out.push(`verdict = ${q(sk.verdict)}`);
    if (sk.capsule) out.push(`capsule = ${q(sk.capsule)}`);
    if (sk.reason) out.push(`reason = ${q(sk.reason)}`);
    out.push(`skill_md_sha256 = ${q(sk.skillMdSha256)}`);
    if (sk.refs.length) {
      out.push("refs = [");
      for (const r of sk.refs) out.push(`  { path = ${q(r.path)}, sha256 = ${q(r.sha256)} },`);
      out.push("]");
    } else {
      out.push("refs = []");
    }
    out.push(`stripped = [${sk.stripped.map(q).join(", ")}]`);
    out.push("");
  }
  return out.join("\n");
}

/** Recompute and compare. */
export function checkLock(committed, intake, sourcesBase) {
  let fresh;
  try {
    fresh = renderLock(buildLock(intake, sourcesBase));
  } catch (e) {
    return { ok: false, diffs: [String(e.message ?? e)] };
  }
  if (fresh === committed) return { ok: true, diffs: [] };
  const a = committed.split("\n");
  const b = fresh.split("\n");
  const diffs = [];
  const n = Math.max(a.length, b.length);
  let ctx = "";
  for (let i = 0; i < n; i++) {
    if (b[i]?.startsWith("path = ")) ctx = b[i];
    if (a[i] !== b[i]) diffs.push(`line ${i + 1} [${ctx}]: lock has ${q(a[i] ?? "")}, sources give ${q(b[i] ?? "")}`);
  }
  return { ok: false, diffs };
}

// ---------------------------------------------------------------------------------------------
// Review table
// ---------------------------------------------------------------------------------------------

export function renderTable(lock) {
  const rows = [
    "| Source | Skill | Loader | Licence field | SKILL.md bytes | Refs | Exec files (net/proc) | Scan hits | Verdict |",
    "|---|---|---|---|---:|---:|---|---|---|",
  ];
  for (const sk of lock.skills) {
    const nExec = sk.scripts.length + sk.execRefs.length;
    const scr = nExec ? `${nExec} (${sk.scriptNet.length}/${sk.scriptProc.length})` : "0";
    const notes = [...sk.flags, ...sk.notes, ...(sk.truncated ? ["listing-truncated"] : []), ...(sk.oversizedRefs.length ? [`${sk.oversizedRefs.length} ref(s) >128KiB`] : [])];
    const verdict = sk.capsule ? `${sk.verdict} (\`${sk.capsule}\`)` : sk.verdict;
    rows.push(
      `| ${sk.source} | \`${sk.path}\` | ${sk.valid ? "ok" : "refused"} | ${sk.license ? sk.license.slice(0, 40).replaceAll("|", "/") : "-"} | ${sk.skillMdBytes} | ${sk.allRefs.length} | ${scr} | ${notes.join(", ") || "none"} | ${verdict} |`,
    );
  }
  return rows.join("\n");
}

// ---------------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------------

function main() {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const repoRoot = path.resolve(here, "..");
  const intake = loadIntake(path.join(repoRoot, ".agentile/skill-intake/intake.json"));
  const base = resolveSourcesBase(repoRoot);
  const lockFile = path.join(repoRoot, "skills.lock");
  const args = process.argv.slice(2);
  if (args.includes("--check")) {
    const res = checkLock(fs.readFileSync(lockFile, "utf8"), intake, base);
    if (!res.ok) {
      console.error(`skills.lock drift (${res.diffs.length} line(s)):\n${res.diffs.slice(0, 40).join("\n")}`);
      process.exit(1);
    }
    console.log("skills.lock matches the sources");
    return;
  }
  const lock = buildLock(intake, base);
  if (args.includes("--table")) {
    console.log(renderTable(lock));
    return;
  }
  if (args.includes("--json")) {
    console.log(
      JSON.stringify(
        lock.skills.map((s) => ({ ...s, flags: [...s.flags], notes: [...s.notes] })),
        null,
        1,
      ),
    );
    return;
  }
  fs.writeFileSync(lockFile, renderLock(lock));
  const counts = {};
  for (const s of lock.skills) counts[s.verdict] = (counts[s.verdict] ?? 0) + 1;
  console.log(`wrote skills.lock: ${lock.skills.length} skills ${JSON.stringify(counts)}`);
  if (lock.collisions.length) console.log(`name collisions across sources: ${JSON.stringify(lock.collisions)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (e) {
    console.error(String(e?.message ?? e));
    process.exit(1);
  }
}
