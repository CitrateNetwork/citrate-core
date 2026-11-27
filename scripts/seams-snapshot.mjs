// L0-WP02 (planset v060): the characterization snapshots of the five hotspots (Store methods,
// AppState and its save/load, the bridge, the routes, the Rust command registration and ACL).
//
// A seam refactor (L0-WP03 to WP08) moves code and must leave every snapshot byte-identical.
// The tests compare against fixtures recorded at a826e441 (the product code at the branch base is
// identical: `git diff --stat a826e441 HEAD -- src src-tauri` is empty before this WP).
//
// Fixtures change only in a reviewed L0 PR that says why. To record them again:
//   node scripts/seams-snapshot.mjs                  runs the suites with SEAMS_SNAPSHOT_WRITE=1
//   node scripts/seams-snapshot.mjs decode <log>     writes the fixtures a CI log printed
// A suite whose fixture is missing fails and prints the value it computed, one line per fixture:
//   SEAMS_SNAPSHOT_MISSING <repo-relative path> <hex of the exact fixture bytes>
// so the recording is the code's own output on the CI runner, never a hand-written file.
import { existsSync, lstatSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import { Buffer } from "node:buffer";

export const SEAMS_BASE = "a826e441";
export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const WRITE_ENV = "SEAMS_SNAPSHOT_WRITE";
export const MISSING_MARK = "SEAMS_SNAPSHOT_MISSING";
export const VOLATILE = "<volatile>";
/** The only directories `decode` may write into (a CI log is input, not a path authority). */
export const FIXTURE_DIRS = ["fixtures/seams/", "src-tauri/tests/fixtures/seams/"];

const isPlain = (v) => v !== null && typeof v === "object" && !Array.isArray(v);

/** A JSON-safe, key-sorted copy that keeps what JSON would lose (undefined, functions, Map, Set). */
export function canonical(v, seen = new WeakSet()) {
  if (v === undefined) return "<undefined>";
  if (typeof v === "function") return `<function/${v.length}>`;
  if (typeof v === "bigint") return `<bigint ${v}>`;
  if (typeof v === "symbol") return "<symbol>";
  if (typeof v === "number" && !Number.isFinite(v)) return `<number ${v}>`;
  if (v === null || typeof v !== "object") return v;
  if (seen.has(v)) return "<cycle>";
  seen.add(v);
  let out;
  if (Array.isArray(v)) out = v.map((x) => canonical(x, seen));
  else if (v instanceof Date) out = `<date ${Number.isNaN(v.getTime()) ? "invalid" : v.toISOString()}>`;
  else if (v instanceof Map) out = { "<map>": [...v.entries()].map((e) => canonical(e, seen)) };
  else if (v instanceof Set) out = { "<set>": [...v.values()].map((x) => canonical(x, seen)) };
  else {
    out = {};
    for (const k of Object.keys(v).sort()) out[k] = canonical(v[k], seen);
  }
  seen.delete(v);
  return out;
}

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/** `a` with every value that differs from run `b` (time, randomness) replaced by VOLATILE.
 *  Both are canonical. A subtree whose shape differs between the runs is volatile as a whole. */
export function maskVolatile(a, b) {
  if (isPlain(a) && isPlain(b)) {
    const ka = Object.keys(a);
    if (!same(ka, Object.keys(b))) return VOLATILE;
    const out = {};
    for (const k of ka) out[k] = maskVolatile(a[k], b[k]);
    return out;
  }
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return VOLATILE;
    return a.map((x, i) => maskVolatile(x, b[i]));
  }
  return same(a, b) ? a : VOLATILE;
}

/** Every difference between two canonical values: "- path" removed, "+ path" added, "~ path" changed. */
export function diffSnapshot(want, got, path = "") {
  const at = (k) => (path ? `${path}.${k}` : String(k));
  if (isPlain(want) && isPlain(got)) {
    const out = [];
    for (const k of Object.keys(want)) {
      if (!(k in got)) out.push(`- ${at(k)}`);
      else out.push(...diffSnapshot(want[k], got[k], at(k)));
    }
    for (const k of Object.keys(got)) if (!(k in want)) out.push(`+ ${at(k)}`);
    return out;
  }
  if (Array.isArray(want) && Array.isArray(got)) {
    const out = [];
    const n = Math.max(want.length, got.length);
    for (let i = 0; i < n; i++) {
      if (i >= got.length) out.push(`- ${at(i)}`);
      else if (i >= want.length) out.push(`+ ${at(i)}`);
      else out.push(...diffSnapshot(want[i], got[i], at(i)));
    }
    return out;
  }
  return same(want, got) ? [] : [`~ ${path || "(root)"}: ${JSON.stringify(want)} -> ${JSON.stringify(got)}`];
}

/** Write mode records instead of comparing, so a CI job must never run in it: under CI it fails. */
function writeMode(env) {
  if (env[WRITE_ENV] !== "1") return false;
  if (env.CI) throw new Error(`${WRITE_ENV}=1 is refused under CI: fixtures are recorded locally and reviewed`);
  return true;
}

function missing(rel, body, log) {
  log(`${MISSING_MARK} ${rel} ${Buffer.from(body, "utf8").toString("hex")}`);
  return new Error(`characterization fixture ${rel} is missing; record it with node scripts/seams-snapshot.mjs (an L0-reviewed update)`);
}

/** Write `body` to the repo-relative `rel` under `root`, refusing to follow a symlink: every
 *  existing path component below `root` (each parent directory and the file itself) is checked
 *  with lstat, and a missing directory is created one level at a time. */
export function writeFixture(root, rel, body) {
  const parts = rel.split("/");
  if (isAbsolute(rel) || parts.some((p) => p === "" || p === "." || p === "..")) throw new Error(`refusing fixture path ${rel}`);
  let at = root;
  for (const [i, part] of parts.entries()) {
    at = join(at, part);
    const last = i === parts.length - 1;
    let st = null;
    try {
      st = lstatSync(at);
    } catch (e) {
      if (e.code !== "ENOENT") throw e;
    }
    if (st?.isSymbolicLink()) throw new Error(`refusing to write ${rel}: ${relative(root, at)} is a symlink`);
    if (last) {
      if (st && !st.isFile()) throw new Error(`refusing to write ${rel}: not a regular file`);
    } else if (!st) {
      mkdirSync(at);
    } else if (!st.isDirectory()) {
      throw new Error(`refusing to write ${rel}: ${relative(root, at)} is not a directory`);
    }
  }
  writeFileSync(at, body);
}

/** Compare `actual` with the recorded fixture `rel` (repo-relative). Returns the differences;
 *  [] means byte-identical. In write mode it records the fixture instead. */
export function checkSnapshot(rel, actual, { root = ROOT, env = process.env, log = console.log } = {}) {
  const file = join(root, rel);
  const body = JSON.stringify(canonical(actual), null, 2) + "\n";
  if (writeMode(env)) {
    writeFixture(root, rel, body);
    return [];
  }
  if (!existsSync(file)) throw missing(rel, body, log);
  const raw = readFileSync(file, "utf8");
  const diff = diffSnapshot(JSON.parse(raw), JSON.parse(body));
  if (diff.length === 0 && raw !== body) return ["~ (bytes): the same value, written differently"];
  return diff;
}

/** The differences between the recorded fixture `rel` and `actual`. Never writes, so the suites'
 *  mutation cases (a dropped or changed member must be named) are safe in write mode too. */
export function diffFixture(rel, actual, { root = ROOT } = {}) {
  return diffSnapshot(JSON.parse(readFileSync(join(root, rel), "utf8")), canonical(actual));
}

/** An input fixture (a saved state): recorded from `make()` in write mode, read otherwise.
 *  When it is missing, the value `make()` returns is printed and used, so the snapshots computed
 *  from it print in the same run; `recorded: false` then fails the caller's own assertion. */
export function inputFixture(rel, make, { root = ROOT, env = process.env, log = console.log } = {}) {
  const file = join(root, rel);
  if (writeMode(env)) {
    const value = make();
    writeFixture(root, rel, JSON.stringify(value, null, 2) + "\n");
    return { value, recorded: true };
  }
  if (!existsSync(file)) {
    const value = make();
    missing(rel, JSON.stringify(value, null, 2) + "\n", log);
    return { value, recorded: false };
  }
  return { value: JSON.parse(readFileSync(file, "utf8")), recorded: true };
}

/** The fixtures a log printed with MISSING_MARK, as [repo-relative path, bytes]. Refuses any path
 *  outside FIXTURE_DIRS, and a path printed twice with different bytes. */
export function decodeLog(text) {
  const found = new Map();
  const re = new RegExp(`${MISSING_MARK} (\\S+) ([0-9a-f]+)`, "g");
  for (const m of text.matchAll(re)) {
    const rel = m[1];
    const norm = relative(ROOT, resolve(ROOT, rel)).split(sep).join("/");
    if (norm !== rel || !FIXTURE_DIRS.some((d) => rel.startsWith(d))) throw new Error(`refusing fixture path ${rel}`);
    const body = Buffer.from(m[2], "hex").toString("utf8");
    if (found.has(rel) && found.get(rel) !== body) throw new Error(`${rel} printed twice with different bytes`);
    found.set(rel, body);
  }
  return [...found.entries()];
}

function main(argv) {
  if (argv[0] === "decode") {
    if (!argv[1]) throw new Error("usage: node scripts/seams-snapshot.mjs decode <ci-log>");
    for (const [rel, body] of decodeLog(readFileSync(argv[1], "utf8"))) {
      writeFixture(ROOT, rel, body);
      console.log(`wrote ${rel}`);
    }
    return 0;
  }
  const env = { ...process.env, [WRITE_ENV]: "1" };
  const steps = [
    ["npx", ["vitest", "run", "src/shell/seams", "src/bridge/seams"]],
    ["cargo", ["test", "-p", "citrate-core", "--lib", "seams_tests::"]],
  ];
  for (const [cmd, args] of steps) {
    const r = spawnSync(cmd, args, { cwd: ROOT, env, stdio: "inherit" });
    if (r.status !== 0) return r.status ?? 1;
  }
  console.log("recorded; review `git diff fixtures/seams src-tauri/tests/fixtures/seams` and say why in the PR");
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exit(main(process.argv.slice(2)));
}
