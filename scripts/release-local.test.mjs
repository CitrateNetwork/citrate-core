// created: 2026-11-11 | branch: feat/053-release-local
// author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
// status: active
//
// scripts/release-local.sh: the release Mac's local release (owner decision 2026-11-11). These
// tests check its argument rules (--check-args, which stops before any signing, notarizing or
// publishing) and, in its source, the order and guards that matter: a rehearsal is only ever a
// draft prerelease, the update tarball is made from the stapled app, every signature is verified
// against the pinned key, and the built app is never run.
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..");
const script = path.join(here, "release-local.sh");
const conf = JSON.parse(fs.readFileSync(path.join(repo, "src-tauri/tauri.conf.json"), "utf8"));
const LINUX = ["Citrate-Core-linux-x86_64.AppImage", "Citrate-Core-linux-x86_64.deb", "Citrate-Core-linux-aarch64.AppImage", "Citrate-Core-linux-aarch64.deb"];

let dir;
beforeEach(() => {
  dir = fs.mkdtempSync("/tmp/core053-release-local-");
  fs.mkdirSync(path.join(dir, "linux"));
  for (const f of LINUX) fs.writeFileSync(path.join(dir, "linux", f), "stand-in");
  fs.mkdirSync(path.join(dir, "Citrate Core.app"));
});
afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

const run = (args) => {
  const r = spawnSync("bash", [script, "--check-args", ...args], { encoding: "utf8" });
  return { code: r.status, out: r.stdout, err: r.stderr };
};
const linux = () => ["--linux-dir", path.join(dir, "linux"), "--out", path.join(dir, "out")];

describe("release-local.sh arguments (--check-args)", () => {
  it("a real release takes the app's own version and is the latest release", () => {
    const r = run(["--version", conf.version, ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toMatch(new RegExp(`tag v${conf.version.replace(/\./g, "\\.")}, latest release`));
  });

  it("a rehearsal takes a throwaway version and an app, and is a draft prerelease", () => {
    const r = run(["--rehearsal", "--version", "0.0.53", "--app", path.join(dir, "Citrate Core.app"), ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toMatch(/tag rehearsal-0\.0\.53, draft prerelease/);
  });

  const refusals = [
    ["a version that is not x.y.z", () => ["--version", "0.5.3-rc.1", ...linux()], /x\.y\.z/],
    ["a real release whose version is not the app's", () => ["--version", "9.9.9", ...linux()], /not tauri\.conf\.json/],
    ["a real release with --app", () => ["--version", conf.version, "--app", path.join(dir, "Citrate Core.app"), ...linux()], /rehearsal/],
    ["--cleanup on a real release", () => ["--version", conf.version, "--cleanup", ...linux()], /rehearsal draft only/],
    ["a rehearsal without an app", () => ["--rehearsal", "--version", "0.0.53", ...linux()], /--app/],
    ["a rehearsal on the app's own version", () => ["--rehearsal", "--version", conf.version, "--app", path.join(dir, "Citrate Core.app"), ...linux()], /throwaway version/],
    ["a missing Linux update file", () => { fs.rmSync(path.join(dir, "linux", LINUX[3])); return ["--version", conf.version, ...linux()]; }, /no .*linux-aarch64\.deb/],
    ["no --out", () => ["--version", conf.version, "--linux-dir", path.join(dir, "linux")], /--out/],
  ];
  for (const [what, args, msg] of refusals) {
    it(`refuses ${what}`, () => {
      const r = run(args());
      expect(r.code).not.toBe(0);
      expect(r.err).toMatch(msg);
    });
  }
});

describe("release-local.sh source", () => {
  const src = fs.readFileSync(script, "utf8");
  const at = (s) => {
    const i = src.indexOf(s);
    expect(i, s).toBeGreaterThan(-1);
    return i;
  };

  it("a rehearsal is created only as a draft prerelease that cannot be latest", () => {
    expect(src).toMatch(/gh release create "\$TAG" "\$\{ASSETS\[@\]\}" --repo "\$REPO_SLUG" --draft --prerelease --latest=false/);
    expect(src.match(/gh release create/g)).toHaveLength(2);
  });

  it("the real release is --latest and is checked anonymously through the fallback feed 0.5.2 reads", () => {
    expect(src).toMatch(/--repo "\$REPO_SLUG" --latest \\\n/);
    expect(src).toContain("releases/latest/download/latest.json");
  });

  it("notarizes, then staples, then makes the update tarball from the stapled app, then signs", () => {
    expect(at("notarytool submit")).toBeLessThan(at('xcrun stapler staple "$APP"'));
    expect(at('xcrun stapler staple "$APP"')).toBeLessThan(at("-czf \"$OUT/Citrate-Core.app.tar.gz\""));
    expect(at("-czf \"$OUT/Citrate-Core.app.tar.gz\"")).toBeLessThan(at("tauri signer sign"));
    expect(src).toContain('[ "$STATUS" = "Accepted" ]');
    expect(src).toContain("source=Notarized Developer ID");
  });

  it("verifies every signature against the pinned updater key, and points latest.json at the release", () => {
    expect(src).toMatch(/minisign -V -q -P "\$PUBKEY"/);
    expect(src).toContain('["plugins"]["updater"]["pubkey"]');
    expect(src).toContain('BASE_URL="https://github.com/$REPO_SLUG/releases/download/$TAG"');
    for (const p of ["darwin-aarch64", "linux-x86_64", "linux-x86_64-deb", "linux-aarch64", "linux-aarch64-deb"]) expect(src).toContain(p);
  });

  it("never runs the built app and keeps the key off argv", () => {
    expect(src).not.toMatch(/Contents\/MacOS\/|\bopen\s+"?\$APP|--version"?\s*$/m);
    expect(src).not.toMatch(/signer sign[^\n]*(-p|--password|-k|--private-key)\b/);
    expect(src).toContain("export TAURI_SIGNING_PRIVATE_KEY_PASSWORD");
  });

  it("refuses to start a real release below the free-space floor", () => {
    expect(src).toMatch(/REAL_FLOOR_GIB=25/);
    expect(src).toContain('[ "$FREE_GIB" -ge "$FLOOR" ]');
  });
});
