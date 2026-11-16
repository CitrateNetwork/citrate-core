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
import { checkStaged, checkLayout, codeIdentity, expandGlob, expectedFromStaged, effectiveBundle, elfEnd } from "./check-bundle.mjs";

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "check-bundle.mjs");
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sha = (b) => createHash("sha256").update(b).digest("hex");
const TRIPLE = "aarch64-apple-darwin";
// ---- minimal real-format executables (test fixtures; never run) ------------------------------
const ARM64 = 0x0100000c;
const X86_64 = 0x01000007;
/** A thin 64-bit Mach-O: LC_UUID (unless uuid is null) and a __TEXT,__text section. */
function machO({ cpu = ARM64, uuid = "11".repeat(16), text = "code" } = {}) {
  const textBuf = Buffer.from(text);
  const cmds = [];
  if (uuid !== null) {
    const c = Buffer.alloc(24);
    c.writeUInt32LE(0x1b, 0);
    c.writeUInt32LE(24, 4);
    Buffer.from(uuid, "hex").copy(c, 8);
    cmds.push(c);
  }
  const seg = Buffer.alloc(72 + 80);
  seg.writeUInt32LE(0x19, 0);
  seg.writeUInt32LE(152, 4);
  seg.write("__TEXT", 8);
  seg.writeUInt32LE(1, 64); // nsects
  seg.write("__text", 72);
  seg.write("__TEXT", 88);
  cmds.push(seg);
  const sizeofcmds = cmds.reduce((n, c) => n + c.length, 0);
  const textOff = 32 + sizeofcmds;
  seg.writeBigUInt64LE(BigInt(textBuf.length), 72 + 40); // size
  seg.writeUInt32LE(textOff, 72 + 48); // offset
  const h = Buffer.alloc(32);
  h.writeUInt32LE(0xfeedfacf, 0);
  h.writeUInt32LE(cpu, 4);
  h.writeUInt32LE(2, 12); // MH_EXECUTE
  h.writeUInt32LE(cmds.length, 16);
  h.writeUInt32LE(sizeofcmds, 20);
  return Buffer.concat([h, ...cmds, textBuf]);
}
/** A fat (universal) Mach-O of `slices`. */
function fat(slices) {
  const head = Buffer.alloc(8 + 20 * slices.length);
  head.writeUInt32BE(0xcafebabe, 0);
  head.writeUInt32BE(slices.length, 4);
  let off = head.length;
  slices.forEach((s, i) => {
    head.writeUInt32BE(s.readUInt32LE(4), 8 + 20 * i);
    head.writeUInt32BE(off, 8 + 20 * i + 8);
    head.writeUInt32BE(s.length, 8 + 20 * i + 12);
    off += s.length;
  });
  return Buffer.concat([head, ...slices]);
}
/** A 64-bit little-endian ELF with .shstrtab, an optional .note.gnu.build-id, and .text. */
function elf({ machine = 0x3e, buildId = "ab".repeat(20), text = "elf code" } = {}) {
  const names = Buffer.from("\0.shstrtab\0.note.gnu.build-id\0.text\0");
  const idx = (n) => names.indexOf(Buffer.from(n + "\0"));
  const parts = [];
  let off = 64;
  const place = (b) => {
    const o = off;
    parts.push(b);
    off += b.length;
    return o;
  };
  const strOff = place(names);
  let note = null;
  if (buildId !== null) {
    const id = Buffer.from(buildId, "hex");
    note = Buffer.alloc(16 + id.length);
    note.writeUInt32LE(4, 0);
    note.writeUInt32LE(id.length, 4);
    note.writeUInt32LE(3, 8);
    note.write("GNU\0", 12, "latin1");
    id.copy(note, 16);
  }
  const noteOff = note ? place(note) : 0;
  const textBuf = Buffer.from(text);
  const textOff = place(textBuf);
  const sh = (name, type, o, size) => {
    const s = Buffer.alloc(64);
    s.writeUInt32LE(name, 0);
    s.writeUInt32LE(type, 4);
    s.writeBigUInt64LE(BigInt(o), 24);
    s.writeBigUInt64LE(BigInt(size), 32);
    return s;
  };
  const shdrs = [Buffer.alloc(64), sh(idx(".shstrtab"), 3, strOff, names.length)];
  if (note) shdrs.push(sh(idx(".note.gnu.build-id"), 7, noteOff, note.length));
  shdrs.push(sh(idx(".text"), 1, textOff, textBuf.length));
  const shoff = off;
  const h = Buffer.alloc(64);
  h.write("\x7fELF", 0, "latin1");
  h[4] = 2;
  h[5] = 1;
  h[6] = 1;
  h.writeUInt16LE(2, 16);
  h.writeUInt16LE(machine, 18);
  h.writeBigUInt64LE(BigInt(shoff), 0x28);
  h.writeUInt16LE(64, 0x3a);
  h.writeUInt16LE(shdrs.length, 0x3c);
  h.writeUInt16LE(1, 0x3e); // e_shstrndx
  return Buffer.concat([h, ...parts, ...shdrs]);
}
const MACHO = machO();

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
    const ELF = elf();
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
  }, 30_000); // ar + tar over a staged tree; slow runners took over the 5 s default

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

describe("code identity, read from the file (#707 review P2)", () => {
  it("Mach-O: per-slice CPU type and LC_UUID, kept by code signing", () => {
    const a = machO();
    expect(codeIdentity(a)).toEqual({ kind: "macho", slices: [`cpu ${ARM64} uuid ${"11".repeat(16)}`] });
    expect(codeIdentity(Buffer.concat([a, Buffer.from("code signature blob")]))).toEqual(codeIdentity(a));
    expect(codeIdentity(machO({ uuid: "22".repeat(16) }))).not.toEqual(codeIdentity(a));
    const u = fat([machO({ cpu: X86_64, uuid: "33".repeat(16) }), machO()]);
    expect(codeIdentity(u).slices).toEqual([`cpu ${X86_64} uuid ${"33".repeat(16)}`, `cpu ${ARM64} uuid ${"11".repeat(16)}`]);
    // No LC_UUID: the __TEXT,__text bytes stand in.
    expect(codeIdentity(machO({ uuid: null })).slices[0]).toMatch(new RegExp(`^cpu ${ARM64} text [0-9a-f]{64}$`));
  });

  it("ELF: e_machine and the GNU build-id, kept by RPATH patching; else the .text bytes", () => {
    const e = elf();
    expect(codeIdentity(e)).toEqual({ kind: "elf", slices: [`machine 62 build-id ${"ab".repeat(20)}`] });
    expect(codeIdentity(elf({ buildId: "cd".repeat(20) }))).not.toEqual(codeIdentity(e));
    const noId = codeIdentity(elf({ buildId: null }));
    expect(noId.slices[0]).toMatch(/^machine 62 text [0-9a-f]{64}$/);
    expect(codeIdentity(elf({ buildId: null, text: "other code" }))).not.toEqual(noId);
  });

  it("a file that only starts like executable code has no identity (an error, not a pass)", () => {
    expect(() => codeIdentity(Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 1, 2]))).toThrow(/cannot read/);
    expect(() => codeIdentity(Buffer.from("\x7fELF\x02\x01", "latin1"))).toThrow(/cannot read/);
  });
});

describe("a wrong sidecar never passes the post-build check (#707 review P2)", () => {
  const app = () => path.join(root, "out/Citrate Core.app/Contents");
  it(".app: another build of a sidecar (other UUID) or another CPU type is refused; a signed copy passes", () => {
    stageComplete();
    bundleLike(path.join(app(), "MacOS"), path.join(app(), "Resources"));
    fs.writeFileSync(path.join(app(), "MacOS/hermes"), Buffer.concat([MACHO, Buffer.from("signature")]));
    expect(checkLayout(opts(), path.join(app(), "MacOS"), path.join(app(), "Resources")).problems).toEqual([]);
    fs.writeFileSync(path.join(app(), "MacOS/hermes"), machO({ uuid: "99".repeat(16) }));
    expect(checkLayout(opts(), path.join(app(), "MacOS"), path.join(app(), "Resources")).problems.join("\n")).toMatch(/MacOS\/hermes is not the staged build/);
    fs.writeFileSync(path.join(app(), "MacOS/hermes"), machO({ cpu: X86_64 }));
    expect(checkLayout(opts(), path.join(app(), "MacOS"), path.join(app(), "Resources")).problems.join("\n")).toMatch(/MacOS\/hermes is not the staged build/);
  });

  it("a non-code sidecar must be byte-identical", () => {
    stageComplete();
    bundleLike(path.join(app(), "MacOS"), path.join(app(), "Resources"));
    fs.writeFileSync(path.join(app(), "MacOS/llama-server"), "another llama");
    expect(checkLayout(opts(), path.join(app(), "MacOS"), path.join(app(), "Resources")).problems.join("\n")).toMatch(/MacOS\/llama-server differs from the staged file/);
  });

  it("AppImage: an ELF library with the same build-id passes after patching; another build is refused", () => {
    stageComplete();
    w("src-tauri/llama/libllama.dylib", elf());
    const r = path.join(root, "appdir");
    bundleLike(path.join(r, "usr/bin"), path.join(r, "usr/lib/Citrate Core"), { change: { "llama/libllama.dylib": Buffer.concat([elf(), Buffer.from("rpath")]) } });
    expect(checkLayout(opts(), path.join(r, "usr/bin"), path.join(r, "usr/lib/Citrate Core")).problems).toEqual([]);
    fs.writeFileSync(path.join(r, "usr/lib/Citrate Core/llama/libllama.dylib"), elf({ buildId: "ee".repeat(20) }));
    expect(checkLayout(opts(), path.join(r, "usr/bin"), path.join(r, "usr/lib/Citrate Core")).problems.join("\n")).toMatch(/llama\/libllama\.dylib is not the staged build/);
  });

  it(".deb: no exception, every file is compared byte for byte, sidecars included", () => {
    stageComplete();
    const r = path.join(root, "debroot2");
    bundleLike(path.join(r, "usr/bin"), path.join(r, "usr/lib/Citrate Core"));
    expect(checkLayout(opts(), path.join(r, "usr/bin"), path.join(r, "usr/lib/Citrate Core"), { exact: true }).problems).toEqual([]);
    fs.writeFileSync(path.join(r, "usr/bin/hermes"), Buffer.concat([MACHO, Buffer.from("x")]));
    fs.writeFileSync(path.join(r, "usr/lib/Citrate Core/llama/libllama.dylib"), Buffer.concat([MACHO, Buffer.from("x")]));
    const p = checkLayout(opts(), path.join(r, "usr/bin"), path.join(r, "usr/lib/Citrate Core"), { exact: true }).problems.join("\n");
    expect(p).toMatch(/bin\/hermes differs from the staged file/);
    expect(p).toMatch(/llama\/libllama\.dylib differs from the staged file/);
  });
});

describe("the pins file is required (#707 review P2)", () => {
  it("a missing runtime-deps.sha256 is refused, never treated as no pins", () => {
    stageComplete();
    fs.rmSync(opts().pins);
    expect(checkStaged(opts()).problems.join("\n")).toMatch(/runtime-deps\.sha256 is missing/);
    const r = spawnSync("node", [SCRIPT, "staged", "--src-tauri", path.join(root, "src-tauri"), "--overlay", opts().overlay, "--target", TRIPLE, "--skills-lock", opts().skillsLock], { encoding: "utf8" });
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/runtime-deps\.sha256 is missing/);
  });
});
