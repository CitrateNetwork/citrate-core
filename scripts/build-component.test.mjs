// @vitest-environment node
//
// scripts/build-component.sh (and the per-component wrappers that delegate to it) build a bundled
// component at the rev pinned in release/components.json and install it only when its sha256
// matches the pin. These cases run the real scripts against a scratch git checkout with two
// commits and a fake `cargo` (first on PATH) whose output depends on the checked-out source, plus
// a local Kubo-shaped archive for the download path, and a scratch manifest
// (CITRATE_COMPONENTS_MANIFEST) and build root (CITRATE_COMPONENT_BUILD_ROOT).
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(here, "..");
const has = (cmd) => spawnSync("sh", ["-c", `command -v ${cmd}`]).status === 0;
const HOST =
  process.platform === "darwin"
    ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-apple-darwin`
    : process.platform === "linux"
      ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-unknown-linux-gnu`
      : null;
const CAN_RUN = Boolean(HOST) && has("git") && has("bash") && has("tar") && has("shasum") && has("curl");

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");
const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

const git = (cwd, ...args) => {
  const r = spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", ...args], { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
  return r.stdout.trim();
};

/** What the fake cargo produces from a source tree whose payload.txt holds `p`. */
const builtBytes = (p) => `#!/bin/sh\necho ${p}\n`;

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "build-component-"));
  tmpDirs.push(root);
  // A checkout with two commits: A (to be pinned) and B (HEAD).
  const co = path.join(root, "citrate-cluster");
  fs.mkdirSync(co);
  git(co, "init", "-q");
  fs.writeFileSync(path.join(co, "payload.txt"), "A\n");
  git(co, "add", "payload.txt");
  git(co, "commit", "-q", "-m", "A");
  const revA = git(co, "rev-parse", "HEAD");
  fs.writeFileSync(path.join(co, "payload.txt"), "B\n");
  git(co, "commit", "-q", "-am", "B");
  const revB = git(co, "rev-parse", "HEAD");
  // Fake cargo: writes <target-dir>/<triple>/release/<bin> from the source tree it runs in, and
  // leaves a marker so a test can tell whether a build ran at all.
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin);
  fs.writeFileSync(
    path.join(bin, "cargo"),
    [
      "#!/bin/sh",
      'T=""; B=""; P=""',
      'while [ $# -gt 0 ]; do case "$1" in --target) T="$2"; shift ;; --bin) B="$2"; shift ;; -p) P="$2"; shift ;; esac; shift; done',
      '[ -n "$B" ] || B="$P"',
      'D="${CARGO_TARGET_DIR:-$PWD/target}/$T/release"',
      'mkdir -p "$D"',
      `touch "${root}/cargo-ran"`,
      'printf "#!/bin/sh\\necho %s\\n" "$(cat payload.txt)" > "$D/$B"',
      'chmod +x "$D/$B"',
      "",
    ].join("\n"),
  );
  fs.chmodSync(path.join(bin, "cargo"), 0o755);
  // A Kubo-shaped archive served from a file:// URL.
  const kdir = path.join(root, "kubo-src", "kubo");
  fs.mkdirSync(kdir, { recursive: true });
  fs.writeFileSync(path.join(kdir, "ipfs"), "#!/bin/sh\necho kubo\n");
  const archive = path.join(root, "kubo.tar.gz");
  spawnSync("tar", ["-czf", archive, "-C", path.join(root, "kubo-src"), "kubo"]);
  // The scratch manifest: the real one, with cluster-daemon and kubo pointed at the fixtures.
  const m = JSON.parse(fs.readFileSync(path.join(REPO, "release", "components.json"), "utf8"));
  const cluster = m.components.find((c) => c.id === "cluster-daemon");
  cluster.source.rev = revA;
  cluster.artifacts = { [HOST]: { sha256: sha256(builtBytes("A")), evidence: "test" } };
  const kubo = m.components.find((c) => c.id === "kubo");
  kubo.artifacts = {
    [HOST]: {
      url: `file://${archive}`,
      archive_sha256: sha256(fs.readFileSync(archive)),
      member: "kubo/ipfs",
      sha256: sha256(fs.readFileSync(path.join(kdir, "ipfs"))),
      evidence: "test",
    },
  };
  fs.mkdirSync(path.join(root, "release"));
  const manifestPath = path.join(root, "release", "components.json");
  const save = () => fs.writeFileSync(manifestPath, JSON.stringify(m, null, 2));
  save();
  const out = path.join(root, "out");
  return { root, co, revA, revB, bin, out, m, cluster, kubo, archive, save, manifestPath };
}

function runScript(f, script, args) {
  return spawnSync("bash", [path.join(here, script), ...args], {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${f.bin}:${process.env.PATH}`,
      CARGO_TARGET_DIR: "",
      CITRATE_COMPONENTS_MANIFEST: f.manifestPath,
      CITRATE_COMPONENT_BUILD_ROOT: path.join(f.root, "cb"),
    },
  });
}
const cluster = (f, ...extra) => runScript(f, "build-cluster-daemon.sh", ["--cluster", f.co, "--target", HOST, "--out-dir", f.out, ...extra]);
const kubo = (f, ...extra) => runScript(f, "build-component.sh", ["kubo", "--target", HOST, "--out-dir", f.out, ...extra]);
const installed = (f, name) => path.join(f.out, `${name}-${HOST}`);

describe.skipIf(!CAN_RUN)("build-component.sh: build at the pin, install only a matching digest", { timeout: 60_000 }, () => {
  it("builds the pinned rev, not the checkout's HEAD, and records the full rev and digest", () => {
    const f = fixture();
    const r = cluster(f);
    expect(r.status, r.stderr).toBe(0);
    expect(fs.readFileSync(installed(f, "cluster-daemon"), "utf8")).toBe(builtBytes("A"));
    const prov = fs.readFileSync(`${installed(f, "cluster-daemon")}.provenance`, "utf8");
    expect(prov).toContain(`@${f.revA}`);
    expect(prov).toContain(`sha256=${sha256(builtBytes("A"))}`);
    // The checkout itself is untouched: still on B.
    expect(git(f.co, "rev-parse", "HEAD")).toBe(f.revB);
  });

  it("refuses bytes whose digest differs from the pin, and installs nothing", () => {
    const f = fixture();
    f.cluster.artifacts[HOST].sha256 = sha256(builtBytes("B"));
    f.save();
    const r = cluster(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/digest mismatch for cluster-daemon/);
    expect(fs.existsSync(installed(f, "cluster-daemon"))).toBe(false);
  });

  it("keeps the previously installed binary when a rebuild does not match", () => {
    const f = fixture();
    fs.mkdirSync(f.out, { recursive: true });
    fs.writeFileSync(installed(f, "cluster-daemon"), "previous");
    f.cluster.artifacts[HOST].sha256 = "0".repeat(64);
    f.save();
    const r = cluster(f);
    expect(r.status).not.toBe(0);
    expect(fs.readFileSync(installed(f, "cluster-daemon"), "utf8")).toBe("previous");
  });

  it("refuses a target with no pin before building anything", () => {
    const f = fixture();
    f.cluster.artifacts = { "riscv64gc-unknown-linux-gnu": { sha256: "0".repeat(64), evidence: "test" } };
    f.save();
    const r = cluster(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/no pin for cluster-daemon on/);
    expect(fs.existsSync(path.join(f.root, "cargo-ran"))).toBe(false);
    expect(fs.existsSync(installed(f, "cluster-daemon"))).toBe(false);
  });

  it("refuses a pinned rev the checkout does not have", () => {
    const f = fixture();
    f.cluster.source.rev = "1".repeat(40);
    f.save();
    const r = cluster(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/does not have the pinned commit 1{40}/);
    expect(fs.existsSync(installed(f, "cluster-daemon"))).toBe(false);
  });

  it("refuses a build tree with local changes", () => {
    const f = fixture();
    expect(cluster(f, "--print-digest").status).toBe(0);
    const src = path.join(f.root, "cb", `cluster-daemon-${f.revA.slice(0, 12)}`, "src");
    fs.writeFileSync(path.join(src, "payload.txt"), "tampered\n");
    const r = cluster(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/has local changes/);
    expect(fs.existsSync(installed(f, "cluster-daemon"))).toBe(false);
  });

  it("--print-digest prints the digest of the pinned build and installs nothing", () => {
    const f = fixture();
    f.cluster.artifacts = {};
    f.save();
    const r = cluster(f, "--print-digest");
    expect(r.status, r.stderr).toBe(0);
    const last = r.stdout.trim().split("\n").pop().split(" ");
    expect(last.slice(0, 3)).toEqual(["cluster-daemon", HOST, sha256(builtBytes("A"))]);
    expect(fs.existsSync(installed(f, "cluster-daemon"))).toBe(false);
  });

  it("fetches a pinned download, checks archive and binary, and installs it", () => {
    const f = fixture();
    const r = kubo(f);
    expect(r.status, r.stderr).toBe(0);
    expect(fs.existsSync(installed(f, "ipfs"))).toBe(true);
    expect(fs.readFileSync(installed(f, "ipfs"), "utf8")).toBe("#!/bin/sh\necho kubo\n");
  });

  it("refuses a download whose archive differs from the pin", () => {
    const f = fixture();
    fs.appendFileSync(f.archive, "x");
    const r = kubo(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/archive digest mismatch/);
    expect(fs.existsSync(installed(f, "ipfs"))).toBe(false);
  });

  it("refuses a download whose binary differs from the pin", () => {
    const f = fixture();
    f.kubo.artifacts[HOST].sha256 = "0".repeat(64);
    f.save();
    const r = kubo(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/digest mismatch for kubo/);
    expect(fs.existsSync(installed(f, "ipfs"))).toBe(false);
  });
});
