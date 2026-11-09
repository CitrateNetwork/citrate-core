// @vitest-environment node
//
// scripts/updater-feed.mjs: the updater feed (latest.json) entries a release job writes for the
// files it built, and the merge of those entries into the one feed both endpoints serve
// (docs/release/053-UPDATER.md section 3, step 4). The platform keys and file names are the ones
// the 0.5.2 feed carries, which every installed app reads.
import { afterEach, describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { PLATFORM_FILES, fragment, merge } from "./updater-feed.mjs";

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "updater-feed.mjs");
const BASE = "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/v0.5.3";
const tmp = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

// A .sig as `tauri signer sign` writes it: base64 of the minisign signature text.
const sigFor = (file) =>
  Buffer.from(
    `untrusted comment: signature from tauri secret key\nRUTFCH8Q3YLDjrFN\ntrusted comment: timestamp:1793325985\tfile:${file}\nwQEN7jM6QqUX\n`,
  ).toString("base64");

function dirWith(names, { sig = true } = {}) {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "updater-feed-"));
  tmp.push(d);
  for (const n of names) {
    fs.writeFileSync(path.join(d, n), `bytes of ${n}`);
    if (sig) fs.writeFileSync(path.join(d, `${n}.sig`), sigFor(n));
  }
  return d;
}

const LINUX_ARM = ["Citrate-Core-linux-aarch64.AppImage", "Citrate-Core-linux-aarch64.deb"];

describe("updater feed platforms", () => {
  it("names the platforms and files of the 0.5.2 feed", () => {
    expect(PLATFORM_FILES).toEqual({
      "darwin-aarch64": "Citrate-Core.app.tar.gz",
      "linux-x86_64": "Citrate-Core-linux-x86_64.AppImage",
      "linux-aarch64": "Citrate-Core-linux-aarch64.AppImage",
      "linux-x86_64-deb": "Citrate-Core-linux-x86_64.deb",
      "linux-aarch64-deb": "Citrate-Core-linux-aarch64.deb",
    });
  });
});

describe("fragment", () => {
  it("writes the linux-aarch64 and linux-aarch64-deb entries with each file's signature", () => {
    const d = dirWith(LINUX_ARM);
    const f = fragment({ version: "0.5.3", baseUrl: BASE, dir: d, platforms: ["linux-aarch64", "linux-aarch64-deb"] });
    expect(f).toEqual({
      version: "0.5.3",
      platforms: {
        "linux-aarch64": { signature: sigFor(LINUX_ARM[0]), url: `${BASE}/Citrate-Core-linux-aarch64.AppImage` },
        "linux-aarch64-deb": { signature: sigFor(LINUX_ARM[1]), url: `${BASE}/Citrate-Core-linux-aarch64.deb` },
      },
    });
  });

  it("refuses a platform the installed apps do not read", () => {
    const d = dirWith(LINUX_ARM);
    expect(() => fragment({ version: "0.5.3", baseUrl: BASE, dir: d, platforms: ["linux-aarch64-appimage"] })).toThrow(/unknown platform 'linux-aarch64-appimage'/);
  });

  it("refuses a file with no signature", () => {
    const d = dirWith(LINUX_ARM, { sig: false });
    expect(() => fragment({ version: "0.5.3", baseUrl: BASE, dir: d, platforms: ["linux-aarch64"] })).toThrow(/Citrate-Core-linux-aarch64.AppImage.sig/);
  });

  it("refuses a signature that is not a minisign signature", () => {
    const d = dirWith(LINUX_ARM);
    fs.writeFileSync(path.join(d, "Citrate-Core-linux-aarch64.AppImage.sig"), Buffer.from("not a signature").toString("base64"));
    expect(() => fragment({ version: "0.5.3", baseUrl: BASE, dir: d, platforms: ["linux-aarch64"] })).toThrow(/not a minisign signature/);
  });

  it("refuses a missing file even when its signature is present", () => {
    const d = dirWith(LINUX_ARM);
    fs.rmSync(path.join(d, "Citrate-Core-linux-aarch64.deb"));
    expect(() => fragment({ version: "0.5.3", baseUrl: BASE, dir: d, platforms: ["linux-aarch64-deb"] })).toThrow(/Citrate-Core-linux-aarch64.deb/);
  });

  it("refuses a base URL that is not https, and a version that is not x.y.z", () => {
    const d = dirWith(LINUX_ARM);
    expect(() => fragment({ version: "0.5.3", baseUrl: "http://example.com/u", dir: d, platforms: ["linux-aarch64"] })).toThrow(/https/);
    expect(() => fragment({ version: "v0.5.3", baseUrl: BASE, dir: d, platforms: ["linux-aarch64"] })).toThrow(/version/);
  });
});

describe("merge", () => {
  const mac = { version: "0.5.3", platforms: { "darwin-aarch64": { signature: sigFor("Citrate Core.app.tar.gz"), url: `${BASE}/Citrate-Core.app.tar.gz` } } };
  const arm = () => fragment({ version: "0.5.3", baseUrl: BASE, dir: dirWith(LINUX_ARM), platforms: ["linux-aarch64", "linux-aarch64-deb"] });

  it("joins the fragments into the feed shape the apps read (version, notes, pub_date, platforms)", () => {
    const feed = merge({ fragments: [mac, arm()], notes: "Citrate Core 0.5.3", pubDate: "2026-11-20T00:00:00Z" });
    expect(Object.keys(feed)).toEqual(["version", "notes", "pub_date", "platforms"]);
    expect(feed.version).toBe("0.5.3");
    expect(Object.keys(feed.platforms).sort()).toEqual(["darwin-aarch64", "linux-aarch64", "linux-aarch64-deb"]);
  });

  it("refuses fragments of different versions (a feed names one release)", () => {
    expect(() => merge({ fragments: [mac, { ...arm(), version: "0.5.2" }], notes: "n", pubDate: "2026-11-20T00:00:00Z" })).toThrow(/version/);
  });

  it("refuses the same platform twice", () => {
    expect(() => merge({ fragments: [arm(), arm()], notes: "n", pubDate: "2026-11-20T00:00:00Z" })).toThrow(/linux-aarch64 appears twice/);
  });

  it("refuses a pub_date that is not RFC 3339 UTC", () => {
    expect(() => merge({ fragments: [mac], notes: "n", pubDate: "Nov 20" })).toThrow(/pub_date/);
  });
});

describe("CLI", () => {
  it("prints a fragment for the files in a directory", () => {
    const d = dirWith(LINUX_ARM);
    const out = execFileSync(process.execPath, [SCRIPT, "fragment", "--version", "0.5.3", "--base-url", BASE, "--dir", d, "linux-aarch64", "linux-aarch64-deb"], { encoding: "utf8" });
    expect(Object.keys(JSON.parse(out).platforms)).toEqual(["linux-aarch64", "linux-aarch64-deb"]);
  });

  it("exits 1 and names the problem when a signature is missing", () => {
    const d = dirWith(LINUX_ARM, { sig: false });
    let code = 0;
    let err = "";
    try {
      execFileSync(process.execPath, [SCRIPT, "fragment", "--version", "0.5.3", "--base-url", BASE, "--dir", d, "linux-aarch64"], { encoding: "utf8", stdio: "pipe" });
    } catch (e) {
      code = e.status;
      err = String(e.stderr);
    }
    expect(code).toBe(1);
    expect(err).toMatch(/Citrate-Core-linux-aarch64.AppImage.sig/);
  });
});
