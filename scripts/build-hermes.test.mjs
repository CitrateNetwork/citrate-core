// @vitest-environment node
//
// HUP-S11.0 (gate g5-size): the Hermes sidecar ships with its local symbols stripped (`strip -x`,
// about 4 MB on aarch64-apple-darwin; build.strip in release/components.json), and since the
// release pinning work scripts/build-hermes.sh builds it at the pinned rev through
// scripts/build-component.sh, which strips BEFORE the digest check, so the pin is of the stripped
// bytes. The real script runs against a fake citrate-agent-runtime git checkout whose `cargo`
// (first on PATH) "builds" a small C program, so the test needs only a C compiler and `strip`, and
// is skipped without them.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, "build-hermes.sh");

const has = (cmd) => spawnSync("sh", ["-c", `command -v ${cmd}`]).status === 0;
const HOST =
  process.platform === "darwin"
    ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-apple-darwin`
    : process.platform === "linux"
      ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-unknown-linux-gnu`
      : null;
const CAN_RUN = Boolean(HOST) && has("cc") && has("strip") && has("nm") && has("bash") && has("git");
const sha256 = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");

const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

const git = (cwd, ...args) => {
  const r = spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", ...args], { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
  return r.stdout.trim();
};

/** A fake runtime checkout (one commit), a fake cargo that copies a compiled program into the
 *  target dir, and a scratch manifest pinning hermes-sidecar at that commit to `pin`. */
function fixture(pin = "stripped") {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "n7-hermes-"));
  tmpDirs.push(root);
  const rt = path.join(root, "citrate-agent-runtime");
  fs.mkdirSync(rt);
  fs.writeFileSync(path.join(rt, "README"), "fixture\n");
  git(rt, "init", "-q");
  git(rt, "add", "README");
  git(rt, "commit", "-q", "-m", "fixture");
  const rev = git(rt, "rev-parse", "HEAD");
  const src = path.join(root, "sidecar.c");
  // Static functions become local symbols, which `strip -x` removes.
  const statics = Array.from({ length: 40 }, (_, i) => `static int local_helper_${i}(int x) { return x * ${i + 3} + 1; }`).join("\n");
  const calls = Array.from({ length: 40 }, (_, i) => `local_helper_${i}(argc)`).join(" + ");
  fs.writeFileSync(src, `#include <stdio.h>\n${statics}\nint main(int argc, char **argv) { (void)argv; printf("hermes-fixture %d\\n", ${calls}); return 0; }\n`);
  const prebuilt = path.join(root, "prebuilt");
  const cc = spawnSync("cc", ["-O0", "-o", prebuilt, src], { encoding: "utf8" });
  if (cc.status !== 0) throw new Error(`cc failed: ${cc.stderr}`);
  // Stripped under the name the build script stages it as: on macOS `strip` re-signs the binary
  // ad hoc, and that signature names the file.
  fs.mkdirSync(path.join(root, "ref"));
  const strippedRef = path.join(root, "ref", `hermes-${HOST}`);
  fs.copyFileSync(prebuilt, strippedRef);
  spawnSync("strip", ["-x", strippedRef]);
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin);
  fs.writeFileSync(
    path.join(bin, "cargo"),
    [
      "#!/bin/sh",
      "# fake cargo: find --target and install the prebuilt program where cargo would put it",
      'T=""; while [ $# -gt 0 ]; do [ "$1" = "--target" ] && T="$2"; shift; done',
      `D="\${CARGO_TARGET_DIR:-${rt}/target}/$T/release"`,
      'mkdir -p "$D"',
      `cp "${prebuilt}" "$D/citrate-agent-sidecar"`,
      "",
    ].join("\n"),
  );
  fs.chmodSync(path.join(bin, "cargo"), 0o755);
  const m = JSON.parse(fs.readFileSync(path.join(here, "..", "release", "components.json"), "utf8"));
  const h = m.components.find((c) => c.id === "hermes-sidecar");
  h.source.rev = rev;
  h.artifacts = { [HOST]: { sha256: sha256(pin === "stripped" ? strippedRef : prebuilt), evidence: "test" } };
  fs.mkdirSync(path.join(root, "release"));
  const manifest = path.join(root, "release", "components.json");
  fs.writeFileSync(manifest, JSON.stringify(m, null, 2));
  const out = path.join(root, "out");
  const buildRoot = path.join(root, "cb");
  return { root, rt, rev, bin, out, prebuilt, manifest, buildRoot };
}

function run(f, extra = [], env = {}) {
  return spawnSync("bash", [SCRIPT, "--agent-runtime", f.rt, "--out-dir", f.out, ...(extra.includes("--target") ? [] : ["--target", HOST]), ...extra], {
    encoding: "utf8",
    env: { ...process.env, PATH: `${f.bin}:${process.env.PATH}`, CARGO_TARGET_DIR: "", CITRATE_COMPONENTS_MANIFEST: f.manifest, CITRATE_COMPONENT_BUILD_ROOT: f.buildRoot, ...env },
  });
}

const localSymbols = (file) =>
  spawnSync("nm", [file], { encoding: "utf8" })
    .stdout.split("\n")
    .filter((l) => /local_helper_\d+/.test(l)).length;

// Each case runs the real build script (and a C fixture build) synchronously; shared CI
// runners routinely exceed vitest's 5 s default, so give the block a realistic bound.
// Each case runs the real build script (and a C fixture build) synchronously; shared CI
// runners routinely exceed vitest's 5 s default, so give the block a realistic bound.
describe.skipIf(!CAN_RUN)("build-hermes.sh strip step (pinned)", { timeout: 60_000 }, () => {
  it("installs a stripped copy that matches the pin and still runs", () => {
    const f = fixture();
    const r = run(f);
    expect(r.status, r.stderr + r.stdout).toBe(0);
    const installed = path.join(f.out, `hermes-${HOST}`);
    expect(localSymbols(f.prebuilt)).toBeGreaterThan(0);
    expect(localSymbols(installed)).toBe(0);
    expect(fs.statSync(installed).size).toBeLessThan(fs.statSync(f.prebuilt).size);
    expect(r.stdout).toMatch(/strip -x/);
    expect(spawnSync(installed, [], { encoding: "utf8" }).stdout).toMatch(/^hermes-fixture \d+/);
    expect(fs.readFileSync(path.join(f.out, `hermes-${HOST}.provenance`), "utf8")).toContain(`@${f.rev}`);
    // The cargo output itself is left as built.
    const built = path.join(f.buildRoot, `hermes-sidecar-${f.rev.slice(0, 12)}`, "target", HOST, "release", "citrate-agent-sidecar");
    expect(localSymbols(built)).toBeGreaterThan(0);
  });

  it("strips before the digest check: a pin of the unstripped bytes is refused", () => {
    const f = fixture("unstripped");
    const r = run(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/digest mismatch for hermes-sidecar/);
    expect(fs.existsSync(path.join(f.out, `hermes-${HOST}`))).toBe(false);
  });

  it("refuses to build the stripped pin for another OS, rather than installing it unstripped", () => {
    const f = fixture();
    const other = HOST.endsWith("apple-darwin") ? "aarch64-unknown-linux-gnu" : "aarch64-apple-darwin";
    const r = run(f, ["--target", other, "--print-digest"]);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/can only be stripped on a (Linux|Darwin) host/);
    expect(fs.existsSync(path.join(f.out, `hermes-${other}`))).toBe(false);
  });

  it("no longer accepts --no-strip (an unstripped build is never the pinned bytes)", () => {
    const f = fixture();
    const r = run(f, ["--no-strip"]);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/unknown arg: --no-strip/);
  });

  it("builds in the fixed build root even when the caller sets CARGO_TARGET_DIR", () => {
    const f = fixture();
    const shared = path.join(f.root, "shared-target");
    const r = run(f, [], { CARGO_TARGET_DIR: shared });
    expect(r.status, r.stderr + r.stdout).toBe(0);
    expect(fs.existsSync(shared)).toBe(false);
    expect(fs.existsSync(path.join(f.out, `hermes-${HOST}`))).toBe(true);
  });
});
