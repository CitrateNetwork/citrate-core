#!/usr/bin/env node
// =====================================================================
// citrate-core: prune the staged llama.cpp runtime to what llama-server loads (HUP-S11.0, g5-size)
//
//   node scripts/prune-llama-runtime.mjs [--dir src-tauri/llama] [--dry-run]
//
// The staged macOS llama runtime (`src-tauri/llama`, from the `llama-runtime-arm64.tar.gz` asset)
// carries every llama.cpp dylib three times (`libggml.dylib`, `libggml.0.dylib`,
// `libggml.0.23.0.dylib`, ...) as regular files, because it was packed from a CMake install tree
// whose version symlinks were dereferenced, plus the llama.cpp tool libraries (bench, cli,
// quantize, ...) that llama-server never loads. Tauri copies `llama/*` into the app as regular
// files, so about 34 MB of the 59.6 MB `resources/llama` component was never loaded.
//
// llama-server and its dylibs name every dependency by install name (`@rpath/libggml.0.dylib`,
// ...), resolved through the `@loader_path` rpath to this directory. This script walks that graph
// from `llama-server` with `otool -L` and keeps exactly the closure:
//
//   - every `@rpath/` or `@loader_path/` dependency of a kept Mach-O must exist in the directory
//     as a regular file (else refused, nothing removed): a pruned runtime can never miss a library;
//   - Mach-O files outside the closure are removed; every other file (the llama.cpp licence, the
//     checksum list) is kept. Symlinks and subdirectories are refused (the runtime is flat files).
//
// The ggml backends in this build are linked, not loaded as plugins (no `ggml_backend_init`
// export), so ggml's own search for `libggml-<backend>.dylib` beside the executable finds nothing
// to load either way. dyld checks the same graph again when llama-server starts on the member's
// machine. Exit 0 pruned (or nothing to prune), 1 refused, 2 usage. macOS only (`otool`).
// No dependencies beyond Node's standard library.
// =====================================================================
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ENTRY = "llama-server";
const USAGE = "usage: node scripts/prune-llama-runtime.mjs [--dir <staged llama runtime dir>] [--dry-run]";

/**
 * The dependencies `otool -L` lists that resolve inside the runtime directory: `@rpath/<name>` and
 * `@loader_path/<name>`, as bare file names. A dylib's own install name is listed too; a kept file
 * satisfies its own id, so that changes nothing. System libraries (absolute paths) are ignored.
 */
export function parseLocalDeps(otoolText) {
  const deps = [];
  for (const line of otoolText.split(/\r?\n/).slice(1)) {
    const m = /^\s+@(?:rpath|loader_path)\/([^\s]+) \(/.exec(line);
    if (!m) continue;
    const name = m[1];
    if (name.includes("/")) throw new Error(`dependency ${name} points into a subdirectory`);
    deps.push(name);
  }
  return deps;
}

/**
 * Walk the dependency graph from `entry`. `depsOf(name)` returns the local dependency names of a
 * file; `exists(name)` says whether the directory holds it as a regular file.
 * Returns { closure: sorted names, missing: [{ file, dep }] }.
 */
export function dependencyClosure(entry, depsOf, exists) {
  const closure = new Set([entry]);
  const missing = [];
  const queue = [entry];
  while (queue.length) {
    const file = queue.shift();
    for (const dep of depsOf(file)) {
      if (closure.has(dep)) continue;
      if (!exists(dep)) {
        missing.push({ file, dep });
        continue;
      }
      closure.add(dep);
      queue.push(dep);
    }
  }
  return { closure: [...closure].sort(), missing };
}

/** Mach-O magic numbers (thin 32/64-bit in either byte order, and universal). */
const MACHO_MAGIC = [0xfeedfacf, 0xcffaedfe, 0xfeedface, 0xcefaedfe, 0xcafebabe];

export function isMachO(p) {
  const fd = fs.openSync(p, "r");
  try {
    const b = Buffer.alloc(4);
    if (fs.readSync(fd, b, 0, 4, 0) < 4) return false;
    return MACHO_MAGIC.includes(b.readUInt32BE(0));
  } finally {
    fs.closeSync(fd);
  }
}

/** Run the prune on `dir`. Returns { code, lines, removed, removedBytes, kept }. */
export function pruneLlamaRuntime(dir, { dryRun = false, otool = "otool" } = {}) {
  const lines = [];
  const refuse = (msg) => ({ code: 1, lines: [...lines, `REFUSED: ${msg}`], removed: [], removedBytes: 0, kept: [] });
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const odd = entries.filter((e) => !e.isFile()).map((e) => e.name);
  if (odd.length) return refuse(`not a regular file: ${odd.sort().join(", ")} (the runtime is flat files)`);
  const files = new Set(entries.map((e) => e.name));
  if (!files.has(ENTRY)) return refuse(`${path.join(dir, ENTRY)} not found (is this a staged llama runtime?)`);
  const machO = new Set([...files].filter((n) => isMachO(path.join(dir, n))));
  if (!machO.has(ENTRY)) return refuse(`${ENTRY} is not a Mach-O executable (this prune is for the macOS runtime)`);

  let walk;
  try {
    walk = dependencyClosure(
      ENTRY,
      (name) => {
        if (!machO.has(name)) throw new Error(`${name} is a dependency but not a Mach-O file`);
        let out;
        try {
          out = execFileSync(otool, ["-L", path.join(dir, name)], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
        } catch (e) {
          throw new Error(`could not read the load commands of ${name} with ${otool}: ${e.message}`);
        }
        return parseLocalDeps(out);
      },
      (name) => files.has(name),
    );
  } catch (e) {
    return refuse(e.message);
  }
  if (walk.missing.length) {
    return refuse(`missing libraries: ${walk.missing.map((m) => `${m.file} -> ${m.dep}`).join(", ")}`);
  }
  const closure = new Set(walk.closure);
  const removed = [...machO].filter((n) => !closure.has(n)).sort();
  const kept = [...files].filter((n) => !removed.includes(n)).sort();
  let removedBytes = 0;
  for (const name of removed) {
    const p = path.join(dir, name);
    removedBytes += fs.statSync(p).size;
    if (!dryRun) fs.rmSync(p);
  }
  lines.push(`dependency closure of ${ENTRY}: ${walk.closure.length} Mach-O files (${walk.closure.join(", ")})`);
  for (const name of removed) lines.push(`${dryRun ? "would remove" : "removed"} ${name}`);
  lines.push(
    `${dryRun ? "dry run: " : ""}kept ${kept.length} files; ${dryRun ? "would remove" : "removed"} ` +
      `${removed.length} Mach-O files outside the closure, ${removedBytes} bytes`,
  );
  return { code: 0, lines, removed, removedBytes, kept };
}

function main(argv) {
  let dir = "src-tauri/llama";
  let dryRun = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--dir" && i + 1 < argv.length) dir = argv[++i];
    else if (a === "--dry-run") dryRun = true;
    else if (a === "-h" || a === "--help") {
      console.log(USAGE);
      return 0;
    } else {
      console.error(`unknown argument: ${a}\n${USAGE}`);
      return 2;
    }
  }
  if (!fs.existsSync(dir) || !fs.statSync(dir).isDirectory()) {
    console.error(`not a directory: ${dir}\n${USAGE}`);
    return 2;
  }
  const r = pruneLlamaRuntime(dir, { dryRun });
  for (const l of r.lines) (r.code === 0 ? console.log : console.error)(l);
  return r.code;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
