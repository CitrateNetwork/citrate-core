// @vitest-environment node
//
// HUP-S11.0 (gate g5-size): scripts/prune-llama-runtime.mjs keeps exactly the dependency closure
// of llama-server in the staged llama runtime and removes the duplicate version-named copies and
// the unused tool libraries. These tests use fixture directories of fake Mach-O files and a fake
// `otool` that prints load commands from a JSON map, so they run on any OS; the real otool output
// format is pinned by the parser tests.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { dependencyClosure, isMachO, parseLocalDeps, pruneLlamaRuntime } from "./prune-llama-runtime.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, "prune-llama-runtime.mjs");

const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});
const tmp = (prefix) => {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tmpDirs.push(d);
  return d;
};

/** A file that starts with the 64-bit Mach-O magic (little endian, as on arm64), `size` bytes long. */
function machO(p, size) {
  const b = Buffer.alloc(size);
  b.writeUInt32LE(0xfeedfacf, 0);
  fs.writeFileSync(p, b);
}

/** otool -L output for `file` with these @rpath deps (its own id first for a dylib), plus system libs. */
function otoolText(file, deps, { dylib = true } = {}) {
  const lines = [`${file}:`];
  if (dylib) lines.push(`\t@rpath/${path.basename(file)} (compatibility version 0.0.0, current version 0.23.0)`);
  for (const d of deps) lines.push(`\t@rpath/${d} (compatibility version 0.0.0, current version 0.0.0)`);
  lines.push("\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 2100.43.0)");
  lines.push("\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)");
  return lines.join("\n") + "\n";
}

/** A fake otool (a POSIX shell script): prints the load commands in `depsMap` for the file it is given. */
function fakeOtool(dir, depsMap) {
  const cases = Object.entries(depsMap).map(([name, deps]) => {
    const lines = [];
    if (name !== "llama-server") lines.push(`\t@rpath/${name} (compatibility version 0.0.0, current version 0.23.0)`);
    for (const d of deps) lines.push(`\t@rpath/${d} (compatibility version 0.0.0, current version 0.0.0)`);
    lines.push("\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)");
    return `  ${name}) printf '%s:\\n' "$2"; printf '${lines.join("\\n")}\\n' ;;`;
  });
  const tool = path.join(dir, "fake-otool");
  fs.writeFileSync(
    tool,
    ["#!/bin/sh", 'case "$(basename "$2")" in', ...cases, '  *) echo "fake otool: no entry for $2" >&2; exit 1 ;;', "esac", ""].join("\n"),
  );
  fs.chmodSync(tool, 0o755);
  return tool;
}

// The real layout of the staged runtime (2026-10-04, llama.cpp build 10909), at small sizes:
// each library three times, the tool libraries, and the two text files.
const LIBS = {
  "libggml-base": ["0.dylib", "0.23.0.dylib", "dylib"],
  "libggml-cpu": ["0.dylib", "0.23.0.dylib", "dylib"],
  "libggml-metal": ["0.dylib", "0.23.0.dylib", "dylib"],
  libggml: ["0.dylib", "0.23.0.dylib", "dylib"],
  libllama: ["0.dylib", "0.4.0.dylib", "dylib"],
  "libllama-common": ["0.dylib", "0.4.0.dylib", "dylib"],
};
const DEPS = {
  "llama-server": ["libllama-server-impl.dylib", "libllama-common.0.dylib", "libllama.0.dylib", "libggml.0.dylib", "libggml-cpu.0.dylib", "libggml-base.0.dylib"],
  "libllama-server-impl.dylib": ["libllama-common.0.dylib", "libllama.0.dylib", "libggml-base.0.dylib"],
  "libllama-common.0.dylib": ["libllama.0.dylib", "libggml.0.dylib"],
  "libllama.0.dylib": ["libggml.0.dylib", "libggml-base.0.dylib"],
  "libggml.0.dylib": ["libggml-cpu.0.dylib", "libggml-metal.0.dylib", "libggml-base.0.dylib"],
  "libggml-cpu.0.dylib": ["libggml-base.0.dylib"],
  "libggml-metal.0.dylib": ["libggml-base.0.dylib"],
  "libggml-base.0.dylib": [],
};
const CLOSURE = [
  "libggml-base.0.dylib",
  "libggml-cpu.0.dylib",
  "libggml-metal.0.dylib",
  "libggml.0.dylib",
  "libllama-common.0.dylib",
  "libllama-server-impl.dylib",
  "libllama.0.dylib",
  "llama-server",
];

function fixtureRuntime() {
  const root = tmp("n7-llama-");
  const dir = path.join(root, "llama");
  fs.mkdirSync(dir);
  for (const [lib, suffixes] of Object.entries(LIBS)) for (const s of suffixes) machO(path.join(dir, `${lib}.${s}`), 1000);
  machO(path.join(dir, "libllama-server-impl.dylib"), 1000);
  machO(path.join(dir, "libllama-bench-impl.dylib"), 700);
  machO(path.join(dir, "libllama-quantize-impl.dylib"), 300);
  machO(path.join(dir, "llama-server"), 500);
  fs.writeFileSync(path.join(dir, "LICENSE-llama.cpp.txt"), "MIT License\n");
  fs.writeFileSync(path.join(dir, "llama-server.sha256"), `${"0".repeat(64)}  llama-server\n`);
  const otool = fakeOtool(root, DEPS);
  return { root, dir, otool };
}

describe("parseLocalDeps", () => {
  it("reads @rpath and @loader_path names and ignores system libraries", () => {
    const text = otoolText("/x/libllama.0.dylib", ["libggml.0.dylib", "libggml-base.0.dylib"]).replace(
      "\t/usr/lib/libc++",
      "\t@loader_path/libextra.dylib (compatibility version 1.0.0, current version 1.0.0)\n\t/usr/lib/libc++",
    );
    expect(parseLocalDeps(text)).toEqual(["libllama.0.dylib", "libggml.0.dylib", "libggml-base.0.dylib", "libextra.dylib"]);
  });

  it("parses the real otool -L output of llama-server (build 10909)", () => {
    const real = [
      "llama/llama-server:",
      "\t@rpath/libllama-server-impl.dylib (compatibility version 0.0.0, current version 0.0.0)",
      "\t@rpath/libllama-common.0.dylib (compatibility version 0.0.0, current version 0.0.0)",
      "\t@rpath/libmtmd.0.dylib (compatibility version 0.0.0, current version 0.0.0)",
      "\t@rpath/libggml-rpc.0.dylib (compatibility version 0.0.0, current version 0.23.0)",
      "\t/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 5026.5.4)",
      "\t/usr/lib/librdma.dylib (compatibility version 1.0.0, current version 1.0.0, weak)",
    ].join("\n");
    expect(parseLocalDeps(real)).toEqual([
      "libllama-server-impl.dylib",
      "libllama-common.0.dylib",
      "libmtmd.0.dylib",
      "libggml-rpc.0.dylib",
    ]);
  });

  it("refuses a dependency in a subdirectory", () => {
    expect(() => parseLocalDeps("x:\n\t@rpath/sub/libx.dylib (compatibility version 0.0.0, current version 0.0.0)\n")).toThrow(
      /subdirectory/,
    );
  });
});

describe("dependencyClosure", () => {
  it("walks the graph from the entry and reports missing libraries", () => {
    const deps = { a: ["b", "c"], b: ["c", "d"], c: [], d: ["a"] };
    const have = new Set(["a", "b", "c"]);
    const r = dependencyClosure("a", (n) => deps[n], (n) => have.has(n));
    expect(r.closure).toEqual(["a", "b", "c"]);
    expect(r.missing).toEqual([{ file: "b", dep: "d" }]);
  });

  it("terminates on cycles", () => {
    const deps = { a: ["b"], b: ["a"] };
    expect(dependencyClosure("a", (n) => deps[n], () => true).closure).toEqual(["a", "b"]);
  });
});

describe("isMachO", () => {
  it("recognises Mach-O magic and rejects text", () => {
    const d = tmp("n7-macho-");
    machO(path.join(d, "bin"), 16);
    fs.writeFileSync(path.join(d, "txt"), "MIT License\n");
    fs.writeFileSync(path.join(d, "short"), "ab");
    expect(isMachO(path.join(d, "bin"))).toBe(true);
    expect(isMachO(path.join(d, "txt"))).toBe(false);
    expect(isMachO(path.join(d, "short"))).toBe(false);
  });
});

describe("pruneLlamaRuntime", () => {
  it("keeps the closure and the text files and removes copies and tool libraries", () => {
    const { dir, otool } = fixtureRuntime();
    const r = pruneLlamaRuntime(dir, { otool });
    expect(r.code).toBe(0);
    expect(fs.readdirSync(dir).sort()).toEqual([...CLOSURE, "LICENSE-llama.cpp.txt", "llama-server.sha256"].sort());
    // 6 libraries x 2 extra names x 1000 bytes, plus the two tool libraries.
    expect(r.removedBytes).toBe(6 * 2 * 1000 + 700 + 300);
    expect(r.removed).toContain("libggml.dylib");
    expect(r.removed).toContain("libllama.0.4.0.dylib");
    expect(r.removed).toContain("libllama-bench-impl.dylib");
  });

  it("is idempotent", () => {
    const { dir, otool } = fixtureRuntime();
    expect(pruneLlamaRuntime(dir, { otool }).code).toBe(0);
    const again = pruneLlamaRuntime(dir, { otool });
    expect(again.code).toBe(0);
    expect(again.removed).toEqual([]);
    expect(again.removedBytes).toBe(0);
  });

  it("removes nothing on a dry run", () => {
    const { dir, otool } = fixtureRuntime();
    const before = fs.readdirSync(dir).sort();
    const r = pruneLlamaRuntime(dir, { otool, dryRun: true });
    expect(r.code).toBe(0);
    expect(r.removed.length).toBe(14);
    expect(fs.readdirSync(dir).sort()).toEqual(before);
  });

  it("refuses, removing nothing, when a needed library is missing", () => {
    const { dir, otool } = fixtureRuntime();
    fs.rmSync(path.join(dir, "libggml-metal.0.dylib"));
    const before = fs.readdirSync(dir).sort();
    const r = pruneLlamaRuntime(dir, { otool });
    expect(r.code).toBe(1);
    expect(r.lines.join("\n")).toMatch(/libggml\.0\.dylib -> libggml-metal\.0\.dylib/);
    expect(fs.readdirSync(dir).sort()).toEqual(before);
  });

  it("refuses a symlink or a subdirectory", () => {
    const a = fixtureRuntime();
    fs.symlinkSync("libggml.0.dylib", path.join(a.dir, "libggml.1.dylib"));
    expect(pruneLlamaRuntime(a.dir, { otool: a.otool }).lines.join("\n")).toMatch(/not a regular file: libggml\.1\.dylib/);
    const b = fixtureRuntime();
    fs.mkdirSync(path.join(b.dir, "extra"));
    expect(pruneLlamaRuntime(b.dir, { otool: b.otool }).code).toBe(1);
  });

  it("refuses a directory without a Mach-O llama-server", () => {
    const a = fixtureRuntime();
    fs.rmSync(path.join(a.dir, "llama-server"));
    expect(pruneLlamaRuntime(a.dir, { otool: a.otool }).lines.join("\n")).toMatch(/llama-server not found/);
    const b = fixtureRuntime();
    fs.writeFileSync(path.join(b.dir, "llama-server"), "#!/bin/sh\n");
    expect(pruneLlamaRuntime(b.dir, { otool: b.otool }).lines.join("\n")).toMatch(/not a Mach-O/);
  });

  it("refuses when a dependency is not a Mach-O file", () => {
    const { dir, otool } = fixtureRuntime();
    fs.writeFileSync(path.join(dir, "libggml-base.0.dylib"), "not a library");
    const r = pruneLlamaRuntime(dir, { otool });
    expect(r.code).toBe(1);
    expect(r.lines.join("\n")).toMatch(/libggml-base\.0\.dylib is a dependency but not a Mach-O file/);
  });

  it("refuses when otool cannot read a file", () => {
    const { dir, otool } = fixtureRuntime();
    const r = pruneLlamaRuntime(dir, { otool: path.join(path.dirname(otool), "no-such-otool") });
    expect(r.code).toBe(1);
    expect(r.lines.join("\n")).toMatch(/could not read the load commands of llama-server/);
  });
});

describe("CLI", () => {
  it("exits 2 on a bad argument or a missing directory", () => {
    expect(spawnSync(process.execPath, [SCRIPT, "--bogus"], { encoding: "utf8" }).status).toBe(2);
    expect(spawnSync(process.execPath, [SCRIPT, "--dir", "/nonexistent/llama"], { encoding: "utf8" }).status).toBe(2);
  });

  it("exits 1 on a directory without llama-server", () => {
    const d = tmp("n7-llama-cli-");
    const r = spawnSync(process.execPath, [SCRIPT, "--dir", d, "--dry-run"], { encoding: "utf8" });
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/REFUSED/);
  });
});
