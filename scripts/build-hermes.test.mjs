// @vitest-environment node
//
// HUP-S11.0 (gate g5-size): scripts/build-hermes.sh installs the Hermes sidecar with its local
// symbols stripped (`strip -x`, about 4 MB on aarch64-apple-darwin), unless --no-strip. The real
// script runs against a fake citrate-agent-runtime whose `cargo` (first on PATH) "builds" a small
// C program, so the test needs only a C compiler and `strip`, and is skipped without them.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
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
const CAN_RUN = Boolean(HOST) && has("cc") && has("strip") && has("nm") && has("bash");

const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

/** A fake runtime checkout plus a fake cargo that copies a compiled program into the target dir. */
function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "n7-hermes-"));
  tmpDirs.push(root);
  const rt = path.join(root, "citrate-agent-runtime");
  fs.mkdirSync(rt);
  const src = path.join(root, "sidecar.c");
  // Static functions become local symbols, which `strip -x` removes.
  const statics = Array.from({ length: 40 }, (_, i) => `static int local_helper_${i}(int x) { return x * ${i + 3} + 1; }`).join("\n");
  const calls = Array.from({ length: 40 }, (_, i) => `local_helper_${i}(argc)`).join(" + ");
  fs.writeFileSync(src, `#include <stdio.h>\n${statics}\nint main(int argc, char **argv) { (void)argv; printf("hermes-fixture %d\\n", ${calls}); return 0; }\n`);
  const prebuilt = path.join(root, "prebuilt");
  const cc = spawnSync("cc", ["-O0", "-o", prebuilt, src], { encoding: "utf8" });
  if (cc.status !== 0) throw new Error(`cc failed: ${cc.stderr}`);
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
  const out = path.join(root, "out");
  return { root, rt, bin, out, prebuilt };
}

function run(f, ...extra) {
  return spawnSync("bash", [SCRIPT, "--agent-runtime", f.rt, "--target", HOST, "--out-dir", f.out, ...extra], {
    encoding: "utf8",
    env: { ...process.env, PATH: `${f.bin}:${process.env.PATH}`, CARGO_TARGET_DIR: "" },
  });
}

const localSymbols = (file) =>
  spawnSync("nm", [file], { encoding: "utf8" })
    .stdout.split("\n")
    .filter((l) => /local_helper_\d+/.test(l)).length;

describe.skipIf(!CAN_RUN)("build-hermes.sh strip step", () => {
  it("installs a stripped copy that still runs, and records it in the provenance", () => {
    const f = fixture();
    const r = run(f);
    expect(r.status, r.stderr + r.stdout).toBe(0);
    const installed = path.join(f.out, `hermes-${HOST}`);
    expect(localSymbols(f.prebuilt)).toBeGreaterThan(0);
    expect(localSymbols(installed)).toBe(0);
    expect(fs.statSync(installed).size).toBeLessThan(fs.statSync(f.prebuilt).size);
    expect(r.stdout).toMatch(/strip -x: \d+ -> \d+ bytes/);
    expect(spawnSync(installed, [], { encoding: "utf8" }).stdout).toMatch(/^hermes-fixture \d+/);
    expect(fs.readFileSync(path.join(f.out, `hermes-${HOST}.provenance`), "utf8")).toMatch(/stripped=yes/);
    // The cargo output itself is left as built.
    expect(localSymbols(path.join(f.rt, "target", HOST, "release", "citrate-agent-sidecar"))).toBeGreaterThan(0);
  });

  it("--no-strip installs the binary byte for byte", () => {
    const f = fixture();
    const r = run(f, "--no-strip");
    expect(r.status, r.stderr + r.stdout).toBe(0);
    const installed = path.join(f.out, `hermes-${HOST}`);
    expect(fs.readFileSync(installed).equals(fs.readFileSync(f.prebuilt))).toBe(true);
    expect(fs.readFileSync(path.join(f.out, `hermes-${HOST}.provenance`), "utf8")).toMatch(/stripped=no/);
  });

  it("does not strip a binary for another OS, and says so", () => {
    const f = fixture();
    const other = HOST.endsWith("apple-darwin") ? "aarch64-unknown-linux-gnu" : "aarch64-apple-darwin";
    const r = spawnSync("bash", [SCRIPT, "--agent-runtime", f.rt, "--target", other, "--out-dir", f.out], {
      encoding: "utf8",
      env: { ...process.env, PATH: `${f.bin}:${process.env.PATH}`, CARGO_TARGET_DIR: "" },
    });
    expect(r.status, r.stderr + r.stdout).toBe(0);
    expect(r.stdout).toMatch(/is not the host OS .*installed unstripped/);
    expect(fs.readFileSync(path.join(f.out, `hermes-${other}`)).equals(fs.readFileSync(f.prebuilt))).toBe(true);
  });

  it("honours CARGO_TARGET_DIR when locating the build output", () => {
    const f = fixture();
    const shared = path.join(f.root, "shared-target");
    const r = spawnSync("bash", [SCRIPT, "--agent-runtime", f.rt, "--target", HOST, "--out-dir", f.out], {
      encoding: "utf8",
      env: { ...process.env, PATH: `${f.bin}:${process.env.PATH}`, CARGO_TARGET_DIR: shared },
    });
    expect(r.status, r.stderr + r.stdout).toBe(0);
    expect(fs.existsSync(path.join(shared, HOST, "release", "citrate-agent-sidecar"))).toBe(true);
    expect(fs.existsSync(path.join(f.out, `hermes-${HOST}`))).toBe(true);
  });
});
