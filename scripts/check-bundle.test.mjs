// @vitest-environment node
//
// scripts/check-bundle.mjs: every daemon, sidecar, resource and prompt an overlay names ships, both
// staged before the build and inside the built bundle (.app, .deb, AppImage), checked from the
// filesystem only. Nothing here ever runs a bundled binary.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkStaged, checkLayout, expandGlob, expectedFromStaged, effectiveBundle, elfEnd } from "./check-bundle.mjs";

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "check-bundle.mjs");
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sha = (b) => createHash("sha256").update(b).digest("hex");
const TRIPLE = "aarch64-apple-darwin";
const MACHO = Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 1, 2, 3, 4]);

let root; // a stand-in repo: <root>/src-tauri, <root>/templates
const tmp = [];
beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), "check-bundle-"));
  tmp.push(root);
});
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});
const w = (rel, data, mode) => {
  const p = path.join(root, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, data);
  if (mode) fs.chmodSync(p, mode);
  return p;
};

const SKILL_MD = "# demo skill\n";
const lockFor = (md = SKILL_MD) =>
  `version = 1\n\n[[skill]]\nname = "demo"\nsource = "src"\npath = "skills/demo"\nverdict = "include-as-is"\nskill_md_sha256 = "${sha(md)}"\nrefs = []\n`;

/** A fully staged tree for a small overlay that has every kind of input. */
function stageComplete() {
  w("src-tauri/tauri.conf.json", JSON.stringify({ productName: "Citrate Core", bundle: { resources: ["skills/**/*"] } }));
  w(
    "src-tauri/tauri.bundle-test.conf.json",
    JSON.stringify({
      bundle: {
        externalBin: ["binaries/hermes", "binaries/llama-server"],
        resources: ["llama/*", "skills/**/*", "skills-bundle/**/*", "models/bge-gguf/*", "licenses/*", "../templates/*.json"],
      },
    }),
  );
  w(`src-tauri/binaries/hermes-${TRIPLE}`, MACHO, 0o755);
  w(`src-tauri/binaries/llama-server-${TRIPLE}`, "llama-bytes", 0o755);
  w("src-tauri/binaries/README.md", "not a binary");
  w("src-tauri/llama/libllama.dylib", MACHO);
  w("src-tauri/skills/citrate/SKILL.md", "# citrate\n");
  w("src-tauri/skills-bundle/README.md", "readme");
  w("src-tauri/skills-bundle/skills.lock", lockFor());
  w("src-tauri/skills-bundle/src/skills/demo/SKILL.md", SKILL_MD);
  w("src-tauri/models/bge-gguf/bge.gguf", "gguf-bytes");
  w("src-tauri/licenses/kubo.LICENSE", "MIT");
  w("templates/erc20.json", "{}");
  w("skills.lock", lockFor());
  w(
    "src-tauri/runtime-deps.sha256",
    `# pins\n${sha("gguf-bytes")}  bge.gguf\n${sha("llama-bytes")}  llama-server-${TRIPLE}-b10909-nowebui\n${"0".repeat(64)}  llama-server-${TRIPLE}-b00001-old\n`,
  );
}
const opts = (over = {}) => ({
  srcTauri: path.join(root, "src-tauri"),
  overlay: path.join(root, "src-tauri/tauri.bundle-test.conf.json"),
  target: TRIPLE,
  pins: path.join(root, "src-tauri/runtime-deps.sha256"),
  skillsLock: path.join(root, "skills.lock"),
  ...over,
});

describe("the overlay's effective lists", () => {
  it("an overlay's arrays replace the base config's (tauri merge-patch)", () => {
    stageComplete();
    const b = effectiveBundle(opts());
    expect(b.externalBin).toEqual(["binaries/hermes", "binaries/llama-server"]);
    expect(b.resources).toContain("skills-bundle/**/*");
    expect(b.productName).toBe("Citrate Core");
  });

  it("globs: * stays in one folder, **/* goes deep, ../ is outside src-tauri", () => {
    stageComplete();
    w("src-tauri/llama/sub/deep.dylib", "x");
    const st = path.join(root, "src-tauri");
    expect(expandGlob(st, "llama/*")).toEqual(["llama/libllama.dylib"]);
    expect(expandGlob(st, "skills-bundle/**/*").sort()).toEqual(["skills-bundle/README.md", "skills-bundle/skills.lock", "skills-bundle/src/skills/demo/SKILL.md"]);
    expect(expandGlob(st, "../templates/*.json")).toEqual(["../templates/erc20.json"]);
  });
});

describe("staged inputs (before the build)", () => {
  it("a complete staging passes, with every pinned file checked", () => {
    stageComplete();
    const r = checkStaged(opts());
    expect(r.problems).toEqual([]);
    expect(r.pinned.sort()).toEqual([`binaries/llama-server-${TRIPLE}`, "models/bge-gguf/bge.gguf"]);
  });

  it("an unstaged skills-bundle (README only) is refused, by glob", () => {
    stageComplete();
    for (const f of ["skills.lock", "src"]) fs.rmSync(path.join(root, "src-tauri/skills-bundle", f), { recursive: true });
    const r = checkStaged(opts());
    expect(r.problems.join("\n")).toMatch(/skills-bundle\/\*\*\/\* matches no file but a README/);
  });

  it("a stale skills-bundle (not this repo's skills.lock) is refused, with no skill count anywhere", () => {
    stageComplete();
    // The repo re-pinned (a skill removed or changed); the staged tree is the old one.
    w("skills.lock", lockFor("# demo skill, v2\n"));
    const r = checkStaged(opts());
    expect(r.problems.join("\n")).toMatch(/skills-bundle does not match skills\.lock/);
    // Re-staged from the re-pinned tarball: passes again.
    w("src-tauri/skills-bundle/skills.lock", lockFor("# demo skill, v2\n"));
    w("src-tauri/skills-bundle/src/skills/demo/SKILL.md", "# demo skill, v2\n");
    expect(checkStaged(opts()).problems).toEqual([]);
  });

  it("a missing or non-executable sidecar is refused", () => {
    stageComplete();
    fs.rmSync(path.join(root, `src-tauri/binaries/hermes-${TRIPLE}`));
    fs.chmodSync(path.join(root, `src-tauri/binaries/llama-server-${TRIPLE}`), 0o644);
    const p = checkStaged(opts()).problems.join("\n");
    expect(p).toMatch(new RegExp(`binaries/hermes-${TRIPLE} is missing`));
    expect(p).toMatch(new RegExp(`binaries/llama-server-${TRIPLE} is not executable`));
  });

  it("a glob that matches nothing, or an empty file, is refused", () => {
    stageComplete();
    fs.rmSync(path.join(root, "src-tauri/models"), { recursive: true });
    w("src-tauri/licenses/kubo.LICENSE", "");
    const p = checkStaged(opts()).problems.join("\n");
    expect(p).toMatch(/models\/bge-gguf\/\* matches no file/);
    expect(p).toMatch(/licenses\/kubo\.LICENSE is empty/);
  });

  it("a pin of another asset never applies (a provenance file, another triple, a bare name)", () => {
    stageComplete();
    w("src-tauri/llama/llama-server", "linux llama", 0o755);
    w(
      "src-tauri/runtime-deps.sha256",
      fs.readFileSync(opts().pins, "utf8") +
        `${"1".repeat(64)}  hermes-${TRIPLE}-0123456789ab.PROVENANCE.txt\n${"2".repeat(64)}  llama-server-x86_64-unknown-linux-gnu-b9438\n`,
    );
    const r = checkStaged(opts());
    expect(r.problems).toEqual([]);
    expect(r.pinned).not.toContain(`binaries/hermes-${TRIPLE}`);
    expect(r.pinned).not.toContain("llama/llama-server");
  });

  it("a staged file whose name is pinned must match one of its pins", () => {
    stageComplete();
    w("src-tauri/models/bge-gguf/bge.gguf", "tampered");
    w(`src-tauri/binaries/llama-server-${TRIPLE}`, "other build", 0o755);
    const p = checkStaged(opts()).problems.join("\n");
    expect(p).toMatch(/models\/bge-gguf\/bge\.gguf does not match its pin in runtime-deps\.sha256/);
    expect(p).toMatch(new RegExp(`binaries/llama-server-${TRIPLE} does not match`));
  });

  it("the CLI fails closed (exit 1) and passes (exit 0)", () => {
    stageComplete();
    const args = ["staged", "--src-tauri", path.join(root, "src-tauri"), "--overlay", opts().overlay, "--target", TRIPLE, "--pins", opts().pins, "--skills-lock", opts().skillsLock];
    expect(spawnSync("node", [SCRIPT, ...args]).status).toBe(0);
    fs.rmSync(path.join(root, "src-tauri/llama"), { recursive: true });
    const r = spawnSync("node", [SCRIPT, ...args], { encoding: "utf8" });
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/llama\/\* matches no file/);
  });
});

/** Lay the staged tree out as tauri bundles it: binaries by bare name, resources by path, ../ → _up_/. */
function bundleLike(binDir, resDir, { skip = [], change = {} } = {}) {
  const exp = expectedFromStaged(opts());
  for (const b of exp.binaries) {
    if (skip.includes(b.name)) continue;
    const p = path.join(binDir, b.name);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.copyFileSync(b.staged, p);
    fs.chmodSync(p, 0o755);
  }
  for (const r of exp.resources) {
    if (skip.includes(r.bundled)) continue;
    const p = path.join(resDir, r.bundled);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, change[r.bundled] ?? fs.readFileSync(r.staged));
  }
}

describe("the built bundle (filesystem only)", () => {
  it("a .app with everything staged passes; signing may change a Mach-O, not a resource", () => {
    stageComplete();
    const app = path.join(root, "out/Citrate Core.app/Contents");
    bundleLike(path.join(app, "MacOS"), path.join(app, "Resources"), { change: { "llama/libllama.dylib": Buffer.concat([MACHO, Buffer.from("signed")]) } });
    expect(checkLayout(opts(), path.join(app, "MacOS"), path.join(app, "Resources")).problems).toEqual([]);
  });

  it("a .app missing a sidecar, a skill or a template is refused, by name", () => {
    stageComplete();
    const app = path.join(root, "out/Citrate Core.app/Contents");
    bundleLike(path.join(app, "MacOS"), path.join(app, "Resources"), { skip: ["hermes", "skills-bundle/src/skills/demo/SKILL.md", "_up_/templates/erc20.json"] });
    const p = checkLayout(opts(), path.join(app, "MacOS"), path.join(app, "Resources")).problems.join("\n");
    expect(p).toMatch(/MacOS\/hermes is missing/);
    expect(p).toMatch(/skills-bundle\/src\/skills\/demo\/SKILL\.md is missing/);
    expect(p).toMatch(/_up_\/templates\/erc20\.json is missing/);
  });

  it("an ELF library may be rewritten by the AppImage (RPATH), but must still be there", () => {
    stageComplete();
    const ELF = Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1, 0]);
    w("src-tauri/llama/libllama.dylib", ELF);
    const root2 = path.join(root, "appdir");
    bundleLike(path.join(root2, "usr/bin"), path.join(root2, "usr/lib/Citrate Core"), { change: { "llama/libllama.dylib": Buffer.concat([ELF, Buffer.from("patched")]) } });
    expect(checkLayout(opts(), path.join(root2, "usr/bin"), path.join(root2, "usr/lib/Citrate Core")).problems).toEqual([]);
    fs.writeFileSync(path.join(root2, "usr/lib/Citrate Core/llama/libllama.dylib"), "");
    expect(checkLayout(opts(), path.join(root2, "usr/bin"), path.join(root2, "usr/lib/Citrate Core")).problems.join("\n")).toMatch(/libllama\.dylib is empty/);
  });

  it("a resource that differs from what was staged (a stale build) is refused", () => {
    stageComplete();
    const app = path.join(root, "out/Citrate Core.app/Contents");
    bundleLike(path.join(app, "MacOS"), path.join(app, "Resources"), { change: { "skills-bundle/skills.lock": "old lock" } });
    expect(checkLayout(opts(), path.join(app, "MacOS"), path.join(app, "Resources")).problems.join("\n")).toMatch(/skills-bundle\/skills\.lock differs from the staged file/);
  });

  it("a .deb is unpacked with ar and tar (never run) and checked the same way", () => {
    stageComplete();
    const stage = path.join(root, "debroot");
    bundleLike(path.join(stage, "usr/bin"), path.join(stage, "usr/lib/Citrate Core"));
    const deb = path.join(root, "out.deb");
    const work = path.join(root, "debwork");
    fs.mkdirSync(work);
    fs.writeFileSync(path.join(work, "debian-binary"), "2.0\n");
    execFileSync("tar", ["-czf", path.join(work, "control.tar.gz"), "-C", stage, "usr/bin"]);
    execFileSync("tar", ["-czf", path.join(work, "data.tar.gz"), "-C", stage, "usr"]);
    execFileSync("ar", ["rc", deb, "debian-binary", "control.tar.gz", "data.tar.gz"], { cwd: work });
    const ok = spawnSync("node", [SCRIPT, "deb", "--deb", deb, "--src-tauri", path.join(root, "src-tauri"), "--overlay", opts().overlay, "--target", TRIPLE, "--skills-lock", opts().skillsLock], { encoding: "utf8" });
    expect(ok.status, ok.stderr).toBe(0);
  });

  it("the AppImage's squashfs starts where the ELF ends (read from the header, never run)", () => {
    const h = Buffer.alloc(64);
    h.write("\x7fELF", 0, "latin1");
    h[4] = 2; // 64-bit
    h[5] = 1; // little endian
    h.writeBigUInt64LE(4096n, 0x28); // e_shoff
    h.writeUInt16LE(64, 0x3a); // e_shentsize
    h.writeUInt16LE(30, 0x3c); // e_shnum
    expect(elfEnd(h)).toBe(4096 + 64 * 30);
    expect(() => elfEnd(Buffer.from("not an elf"))).toThrow(/not an ELF/);
  });
});

describe("the repo's real overlays", () => {
  it("bundle-lite and bundle-linux name the inputs this gate requires", () => {
    for (const o of ["tauri.bundle-lite.conf.json", "tauri.bundle-linux.conf.json"]) {
      const b = effectiveBundle({ srcTauri: path.join(REPO, "src-tauri"), overlay: path.join(REPO, "src-tauri", o) });
      for (const g of ["skills-bundle/**/*", "llama/*", "models/bge-base-en-v1.5/*", "models/bge-base-en-v1.5-gguf/*", "docs-corpus/**/*", "licenses/*", "knowledge-corpus/**/*"]) {
        expect(b.resources, `${o} ${g}`).toContain(g);
      }
      expect(b.externalBin, o).toContain("binaries/hermes");
    }
  });
});

describe("release paths run the gate", () => {
  const rl = fs.readFileSync(path.join(REPO, "scripts/release-local.sh"), "utf8");
  const at = (s) => {
    const i = rl.indexOf(s);
    expect(i, s).toBeGreaterThan(0);
    return i;
  };
  it("release-local.sh stages skills-bundle from the pinned tarball, then checks inputs, builds, checks the .app, notarizes", () => {
    const dl = at("gh release download runtime-deps -p skills-bundle.tar.gz");
    const pin = at('scripts/ci/verify-runtime-deps.sh src-tauri/runtime-deps.sha256 "$WORK/runtime-deps" skills-bundle.tar.gz');
    const stage = at('node scripts/stage-skills-bundle.mjs from-tarball "$WORK/runtime-deps/skills-bundle.tar.gz" --out src-tauri/skills-bundle');
    const staged = at("node scripts/check-bundle.mjs staged --overlay src-tauri/tauri.bundle-lite.conf.json --target aarch64-apple-darwin");
    const build = at("npx --no-install tauri build");
    const app = at('node scripts/check-bundle.mjs app --app "$APP" --overlay src-tauri/tauri.bundle-lite.conf.json --target aarch64-apple-darwin');
    const notarize = at("xcrun notarytool submit");
    expect(dl < pin && pin < stage && stage < staged && staged < build && build < app && app < notarize).toBe(true);
  });
  it("the Linux build doc runs the same gate before and after the build", () => {
    const doc = fs.readFileSync(path.join(REPO, "docs/RELEASE_LINUX.md"), "utf8");
    expect(doc).toMatch(/node scripts\/check-bundle\.mjs staged --overlay src-tauri\/tauri\.bundle-linux\.conf\.json --target "\$TRIPLE"/);
    expect(doc).toMatch(/node scripts\/check-bundle\.mjs deb +--deb /);
    expect(doc).toMatch(/node scripts\/check-bundle\.mjs appimage +--appimage /);
  });
  it("the gate never runs a bundled binary", () => {
    const src = fs.readFileSync(SCRIPT, "utf8");
    // Only ar, tar and unsquashfs are ever executed, and never with a path from the bundle as the program.
    const execs = [...src.matchAll(/execFileSync\(\s*"([^"]+)"/g)].map((m) => m[1]);
    expect(new Set(execs)).toEqual(new Set(["ar", "tar", "unsquashfs"]));
    expect(src).not.toMatch(/--appimage-extract|\b(spawn|spawnSync|exec|execSync)\(/);
  });
});
