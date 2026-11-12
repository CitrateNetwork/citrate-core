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
    expect(r.out).toContain(`tag v${conf.version}, latest release`);
  });

  it("a rehearsal takes a throwaway version and an app, and is a draft prerelease", () => {
    const r = run(["--rehearsal", "--version", "0.0.53", "--app", path.join(dir, "Citrate Core.app"), ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toMatch(/tag rehearsal-0\.0\.53, draft prerelease/);
  });

  it("--linux-arches x86_64 ships Linux x86_64 only, and the feed does not list linux-aarch64 (owner 2026-11-12)", () => {
    for (const f of LINUX.filter((f) => f.includes("aarch64"))) fs.rmSync(path.join(dir, "linux", f));
    const r = run(["--version", conf.version, "--linux-arches", "x86_64", ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toContain("Linux x86_64 only; the feed does not list linux-aarch64 or linux-aarch64-deb");
  });

  it("without --linux-arches every Linux arch is required (the default is unchanged)", () => {
    const r = run(["--version", conf.version, ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toContain("Linux x86_64 aarch64");
  });

  const refusals = [
    ["a version that is not x.y.z", () => ["--version", "0.5.3-rc.1", ...linux()], /x\.y\.z/],
    ["a real release whose version is not the app's", () => ["--version", "9.9.9", ...linux()], /not tauri\.conf\.json/],
    ["a real release with --app", () => ["--version", conf.version, "--app", path.join(dir, "Citrate Core.app"), ...linux()], /rehearsal/],
    ["--cleanup on a real release", () => ["--version", conf.version, "--cleanup", ...linux()], /rehearsal draft only/],
    ["a rehearsal without an app", () => ["--rehearsal", "--version", "0.0.53", ...linux()], /--app/],
    ["a rehearsal on the app's own version", () => ["--rehearsal", "--version", conf.version, "--app", path.join(dir, "Citrate Core.app"), ...linux()], /throwaway version/],
    ["a missing Linux update file", () => { fs.rmSync(path.join(dir, "linux", LINUX[3])); return ["--version", conf.version, ...linux()]; }, /no .*linux-aarch64\.deb/],
    ["an unknown Linux arch", () => ["--version", conf.version, "--linux-arches", "x86_64,riscv64", ...linux()], /--linux-arches.*riscv64/],
    ["no Linux arch", () => ["--version", conf.version, "--linux-arches", "", ...linux()], /--linux-arches/],
    ["--linux-arches x86_64 with an x86_64 file missing", () => { fs.rmSync(path.join(dir, "linux", LINUX[1])); return ["--version", conf.version, "--linux-arches", "x86_64", ...linux()]; }, /no .*linux-x86_64\.deb/],
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
  // The commands only: comment lines (the header describes the steps too) are left out.
  const src = fs.readFileSync(script, "utf8").split("\n").filter((l) => !l.trimStart().startsWith("#")).join("\n");
  const at = (s) => {
    const i = src.indexOf(s);
    expect(i, s).toBeGreaterThan(-1);
    return i;
  };

  it("a rehearsal is created only as a draft prerelease that cannot be latest", () => {
    expect(src).toMatch(/gh release create "\$TAG" "\$\{ASSETS\[@\]\}" --repo "\$REPO_SLUG" --draft --prerelease --latest=false/);
    expect(src.match(/gh release create/g)).toHaveLength(2);
  });

  it("the real release becomes latest only by the post-verify flip, and is checked anonymously through the fallback feed 0.5.2 reads", () => {
    expect(src).not.toMatch(/gh release create[^\n]*--latest \\\n/);
    expect(src).toContain('gh release edit "$TAG" --repo "$REPO_SLUG" --draft=false --latest');
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
    expect(src).toContain("['plugins']['updater']['pubkey']");
    expect(src).toContain('base64.b64decode(sys.argv[1])');
    expect(src).toContain('"$V052_UPDATER_PUBKEY")"');
    expect(src).toContain('BASE_URL="https://github.com/$REPO_SLUG/releases/download/$TAG"');
    // The platform keys are the 0.5.2 feed's: darwin-aarch64, then linux-<arch> and linux-<arch>-deb per shipped arch.
    expect(src).toContain("PLATFORMS=(darwin-aarch64)");
    expect(src).toContain('PLATFORMS+=("linux-$a" "linux-$a-deb")');
    expect(src).toContain('LINUX_FILES+=("Citrate-Core-linux-$a.AppImage" "Citrate-Core-linux-$a.deb")');
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

  it("refuses, before signing, a target commit GitHub does not have", () => {
    expect(at("repos/$REPO_SLUG/commits/$TARGET_SHA")).toBeLessThan(at("notarytool submit"));
  });
});

// #633 review (BerryManifold P1): the DMG is the one named for this version, never "the first".
describe("release-local.sh --select-dmg (the DMG the build made for this version)", () => {
  const sel = (bundle, version) => spawnSync("bash", [script, "--select-dmg", bundle, "--version", version], { encoding: "utf8" });
  const dmgDir = () => {
    const d = path.join(dir, "bundle", "dmg");
    fs.mkdirSync(d, { recursive: true });
    return d;
  };

  it("takes exactly <productName>_<version>_aarch64.dmg", () => {
    const d = dmgDir();
    fs.writeFileSync(path.join(d, `${conf.productName}_9.9.9_aarch64.dmg`), "new");
    const r = sel(path.join(dir, "bundle"), "9.9.9");
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout.trim()).toBe(path.join(d, `${conf.productName}_9.9.9_aarch64.dmg`));
  });

  it("refuses when an older DMG is still there (it sorts first and would ship)", () => {
    const d = dmgDir();
    fs.writeFileSync(path.join(d, `${conf.productName}_0.5.2_aarch64.dmg`), "stale");
    fs.writeFileSync(path.join(d, `${conf.productName}_9.9.9_aarch64.dmg`), "new");
    const r = sel(path.join(dir, "bundle"), "9.9.9");
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/other DMG.*0\.5\.2/);
  });

  it("refuses when the DMG for this version is missing", () => {
    const d = dmgDir();
    fs.writeFileSync(path.join(d, `${conf.productName}_0.5.2_aarch64.dmg`), "stale");
    const r = sel(path.join(dir, "bundle"), "9.9.9");
    expect(r.status).not.toBe(0);
    expect(r.stderr).toMatch(/no .*_9\.9\.9_aarch64\.dmg/);
  });
});

// #633 review: the money-path guards.
describe("release-local.sh release guards (source)", () => {
  const raw = fs.readFileSync(script, "utf8");
  const src = raw.split("\n").filter((l) => !l.trimStart().startsWith("#")).join("\n");
  const at = (s) => {
    const i = src.indexOf(s);
    expect(i, s).toBeGreaterThan(-1);
    return i;
  };

  it("checks latest.json names exactly the platforms built (not a fixed five)", () => {
    expect(src).not.toContain('want = {"darwin-aarch64", "linux-x86_64", "linux-x86_64-deb", "linux-aarch64", "linux-aarch64-deb"}');
    expect(src).toContain('want = set(sys.argv[4:])');
    expect(src).toMatch(/"\$OUT\/latest.json" "\$VERSION" "\$BASE_URL" "\$\{PLATFORMS\[@\]\}"/);
  });

  it("checks the version in the app and in the app inside the DMG before notarizing", () => {
    expect(src).toContain("plutil -extract CFBundleShortVersionString raw");
    expect(src).toContain("hdiutil attach -nobrowse -readonly");
    expect(at("check_app_version \"$APP\"")).toBeLessThan(at("notarytool submit"));
    expect(at("check_dmg_version")).toBeLessThan(at("notarytool submit"));
  });

  it("a rehearsal checks against the version its app was built with, not the throwaway tag version", () => {
    // A rehearsal of an app built from this tree (0.5.3) uses a throwaway --version, so comparing the
    // app with --version would always refuse it. The check stays; its expected value differs.
    expect(src).toContain('APP_VERSION="$VERSION"');
    const read = at('APP_VERSION="$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist")"');
    expect(read).toBeGreaterThan(at("rehearsal: codesigning the given app"));
    expect(read).toBeLessThan(at('cp -R "$APP" "$WORK/$APP_NAME"'));
    expect(src).toContain('[ "$v" = "$APP_VERSION" ]');
    expect(src).not.toContain('[ "$v" = "$VERSION" ]');
    expect(src).toContain('is not version $APP_VERSION"');
  });

  it("a rehearsal cleanup deletes the draft without a tag cleanup (a draft has no tag) and checks it is gone", () => {
    expect(src).toContain('gh release delete "$TAG" --repo "$REPO_SLUG" --yes\n');
    expect(src).not.toContain("--cleanup-tag");
    expect(at('gh release delete "$TAG"')).toBeLessThan(at('rehearsal draft $TAG deleted'));
    expect(src).toMatch(/gh release view "\$TAG" --repo "\$REPO_SLUG" >\/dev\/null 2>&1; then die "the rehearsal draft \$TAG is still there"/);
  });

  it("gives the updater key only to the signing step, after the build", () => {
    expect(src).not.toMatch(/^export TAURI_SIGNING_PRIVATE_KEY/m);
    expect(at("TAURI_SIGNING_PRIVATE_KEY=")).toBeGreaterThan(at("tauri build"));
    expect(src).toMatch(/createUpdaterArtifacts":false/);
  });

  it("pins the updater public key to the one 0.5.2 installs trust", () => {
    const v052 = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhFQzM4MkREMTA3RjA4QzUKUldURkNIOFEzWUxEanZsN096Vk1WWm9WMllUUE5GNXVUYVFYeGRQNFluRnVlVG5vWVZnV0F6eXAK";
    expect(src).toContain(`V052_UPDATER_PUBKEY="${v052}"`);
    expect(conf.plugins.updater.pubkey).toBe(v052);
    expect(at("V052_UPDATER_PUBKEY\" ]")).toBeLessThan(at("notarytool submit"));
  });

  it("verifies the read-back by sha256 digest against SHA256SUMS", () => {
    expect(src).toMatch(/\.digest/);
    expect(src).toMatch(/sha256:\$/);
  });

  it("creates the real release as a draft, verifies it, and only then makes it latest", () => {
    const create = at("--draft --latest=false \\\n    --target \"$TARGET_SHA\" --title \"Citrate Core $VERSION\"");
    const verify = at("the release holds every asset");
    const flip = at("gh release edit \"$TAG\" --repo \"$REPO_SLUG\" --draft=false --latest");
    const anon = at("releases/latest/download/latest.json");
    expect(create).toBeLessThan(verify);
    expect(verify).toBeLessThan(flip);
    expect(flip).toBeLessThan(anon);
    expect(src.match(/--draft=false/g)).toHaveLength(1);
  });
});


// Owner 2026-11-12: 0.5.3 is private. No GitHub release; the updater files go to Spaces
// citrate-cdn under downloads/updater/ (public-read on exactly those objects), the installers and
// SHA256SUMS to downloads/<file> as PRIVATE objects (served by the invite gate's presigned links).
describe("release-local.sh --publish-target spaces", () => {
  const raw = fs.readFileSync(script, "utf8");
  const src = raw.split("\n").filter((l) => !l.trimStart().startsWith("#")).join("\n");
  const at = (s) => {
    const i = src.indexOf(s);
    expect(i, s).toBeGreaterThan(-1);
    return i;
  };

  it("takes --publish-target spaces and says where each kind of file goes", () => {
    const r = run(["--version", conf.version, "--publish-target", "spaces", ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toContain("publish target spaces: updater files public-read under s3://citrate-cdn/downloads/updater/, installers and SHA256SUMS private under s3://citrate-cdn/downloads/; no GitHub release");
  });

  it("a spaces rehearsal stays under its own prefix", () => {
    const r = run(["--rehearsal", "--version", "0.0.56", "--app", path.join(dir, "Citrate Core.app"), "--publish-target", "spaces", ...linux()]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toContain("s3://citrate-cdn/downloads/rehearsal-0.0.56/updater/");
  });

  it("refuses an unknown publish target", () => {
    const r = run(["--version", conf.version, "--publish-target", "s3-public", ...linux()]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/--publish-target.*github.*spaces/);
  });

  it("the GitHub path is still the default", () => {
    const r = run(["--version", conf.version, ...linux()]);
    expect(r.out).toContain("publish target github");
  });

  it("points latest.json at the CDN objects", () => {
    expect(src).toContain('BASE_URL="$CDN_PUBLIC/$UPD_PREFIX/v$VERSION"');
    expect(src).toContain('CDN_PUBLIC="https://$CDN_BUCKET.$CDN_REGION.cdn.digitaloceanspaces.com"');
  });

  it("uploads updater files public-read, installers and SHA256SUMS private, and latest.json last", () => {
    const upd = at('put_object "$UPD_PREFIX/v$VERSION/$n" "$f" public-read');
    const inst = at('put_object "$INST_PREFIX/$(gate_name "$n")" "$f" private');
    const sums = at('put_object "$INST_PREFIX/SHA256SUMS" "$OUT/SHA256SUMS" private');
    const verify = at("every updater object is public-read and every installer is private");
    const feed = at('put_object "$UPD_PREFIX/latest.json" "$OUT/latest.json" public-read');
    expect(upd).toBeLessThan(verify);
    expect(inst).toBeLessThan(verify);
    expect(sums).toBeLessThan(verify);
    expect(verify).toBeLessThan(feed);
    expect(feed).toBeLessThan(at("the feed and every URL in it answer 200 anonymously at their sha256"));
  });

  it("names the DMG as the invite gate does (citrate-landing src/lib/download.ts)", () => {
    expect(src).toContain('Citrate-Core_"$VERSION"_aarch64.dmg) echo "Citrate-Core-macos-arm64.dmg"');
  });

  it("checks anonymously: updater objects 200 at their sha256, installers refused", () => {
    expect(src).toMatch(/anon_sha "\$u"/);
    expect(src).toMatch(/anon_code "\$CDN_ORIGIN\/\$INST_PREFIX\/\$\(gate_name "\$n"\)"/);
    expect(src).toContain('[ "$c" = "403" ] || die');
  });

  it("never creates a GitHub release on the spaces path, and keeps the secret off argv and output", () => {
    const spaces = src.slice(at('if [ "$PUBLISH_TARGET" = "spaces" ]; then'), at("# ---- 7b. publish to GitHub"));
    expect(spaces).not.toContain("gh release");
    expect(src).toContain('AWS_SECRET_ACCESS_KEY="$(security find-generic-password -s "$SPACES_KEY_ITEM" -w)"');
    expect(src).not.toMatch(/echo[^\n]*AWS_SECRET/);
    expect(src).not.toMatch(/--secret|aws configure set/);
  });

  it("a spaces rehearsal cleanup removes only its own prefix", () => {
    expect(src).toContain('s3 s3 rm "s3://$CDN_BUCKET/downloads/rehearsal-$VERSION/" --recursive');
    expect(src).not.toMatch(/s3 rm "s3:\/\/\$CDN_BUCKET\/downloads\/"/);
  });
});
