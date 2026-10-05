// @vitest-environment node
//
// HUP-S11.0: installer size budget check (scripts/size-budget.mjs).
//
// Fixture bundle dirs (macOS .app + DMG + updater tar.gz; Linux AppImage + AppDir + deb) are
// written to a temp dir with exact byte sizes, so every measured number is known in advance.
// The CLI is run as a child process to pin the exit-code contract: 0 within budget, 1 over
// budget (or unbudgeted under --strict), 2 for usage errors or an empty bundle dir.
import { describe, expect, it, beforeAll, afterAll } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  measureBundle,
  measurePath,
  loadBudgets,
  validateBudgets,
  checkBudgets,
  renderTable,
  renderMarkdown,
  formatBytes,
  V042_SHIPPED,
} from "./size-budget.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const SCRIPT = path.join(here, "size-budget.mjs");

let tmp;

function put(rel, bytes, fill = 0x61) {
  const p = path.join(tmp, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, Buffer.alloc(bytes, fill));
  return p;
}

function mkMac(root) {
  const b = path.join(tmp, root);
  const app = `${root}/macos/Citrate Core.app/Contents`;
  put(`${root}/dmg/Citrate Core_0.4.2_aarch64.dmg`, 5000);
  put(`${root}/dmg/bundle_dmg.sh`, 77); // build helper, not an artifact
  put(`${root}/macos/Citrate Core.app.tar.gz`, 4000);
  put(`${root}/macos/Citrate Core.app.tar.gz.sig`, 412); // signature, not an artifact
  put(`${app}/Info.plist`, 10);
  put(`${app}/MacOS/citrate-core`, 1000);
  put(`${app}/MacOS/hermes`, 300);
  put(`${app}/MacOS/llama-server`, 50);
  put(`${app}/Resources/llama/libggml.0.23.0.dylib`, 200, 0x01);
  put(`${app}/Resources/llama/libggml.0.dylib`, 200, 0x01); // byte-identical copy
  put(`${app}/Resources/llama/libggml.dylib`, 200, 0x01); // byte-identical copy
  put(`${app}/Resources/llama/libllama.dylib`, 120, 0x02);
  fs.symlinkSync("libllama.dylib", path.join(b, `macos/Citrate Core.app/Contents/Resources/llama/libllama.0.dylib`));
  put(`${app}/Resources/models/bge/model.onnx`, 900);
  put(`${app}/Resources/docs-corpus/a.md`, 30);
  put(`${app}/Resources/capsules/hello/capsule.wasm`, 40);
  put(`${app}/Resources/icon.icns`, 20);
  return b;
}

function mkLinux(root) {
  const b = path.join(tmp, root);
  put(`${root}/appimage/citrate-core_0.4.2_amd64.AppImage`, 6000);
  put(`${root}/appimage/Citrate Core.AppDir/usr/bin/citrate-core`, 1100);
  put(`${root}/appimage/Citrate Core.AppDir/usr/bin/hermes`, 310);
  put(`${root}/appimage/Citrate Core.AppDir/usr/lib/Citrate Core/llama/libllama.so`, 130);
  put(`${root}/appimage/Citrate Core.AppDir/usr/lib/Citrate Core/models/m.bin`, 800);
  put(`${root}/appimage/Citrate Core.AppDir/usr/lib/x86_64-linux-gnu/libfoo.so`, 70);
  put(`${root}/appimage/Citrate Core.AppDir/usr/lib/aaa-helper/helper.so`, 5); // sorts before the product dir
  put(`${root}/deb/citrate-core_0.4.2_amd64.deb`, 5500);
  return b;
}

const byId = (rows) => Object.fromEntries(rows.map((r) => [r.id, r]));

function runCli(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
}

function writeBudgets(name, obj) {
  const p = path.join(tmp, name);
  fs.writeFileSync(p, JSON.stringify(obj, null, 2));
  return p;
}

beforeAll(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "size-budget-"));
  mkMac("mac");
  mkLinux("linux");
  fs.mkdirSync(path.join(tmp, "empty"), { recursive: true });
});

afterAll(() => {
  fs.rmSync(tmp, { recursive: true, force: true });
});

describe("measureBundle: macOS", () => {
  it("finds the DMG and the updater tar.gz with exact sizes, ignoring .sig and build helpers", () => {
    const m = measureBundle(path.join(tmp, "mac"));
    expect(m.target).toEqual({ os: "macos", arch: "aarch64" });
    const a = byId(m.artifacts);
    expect(Object.keys(a).sort()).toEqual(["macos-aarch64/app.tar.gz", "macos-aarch64/dmg"]);
    expect(a["macos-aarch64/dmg"].bytes).toBe(5000);
    expect(a["macos-aarch64/app.tar.gz"].bytes).toBe(4000);
  });

  it("measures each sidecar binary and each Resources entry, plus the whole app", () => {
    const c = byId(measureBundle(path.join(tmp, "mac")).components);
    expect(c["macos-aarch64/bin/citrate-core"].bytes).toBe(1000);
    expect(c["macos-aarch64/bin/hermes"].bytes).toBe(300);
    expect(c["macos-aarch64/bin/llama-server"].bytes).toBe(50);
    // symlink counts 0 bytes (it is stored as a link, not a copy)
    expect(c["macos-aarch64/resources/llama"].bytes).toBe(720);
    expect(c["macos-aarch64/resources/llama"].files).toBe(4);
    expect(c["macos-aarch64/resources/models"].bytes).toBe(900);
    expect(c["macos-aarch64/resources/docs-corpus"].bytes).toBe(30);
    expect(c["macos-aarch64/resources/capsules"].bytes).toBe(40);
    expect(c["macos-aarch64/resources/icon.icns"].bytes).toBe(20);
    expect(c["macos-aarch64/app"].bytes).toBe(10 + 1350 + 720 + 900 + 30 + 40 + 20);
  });

  it("reports bytes held in byte-identical duplicate files inside a component", () => {
    const c = byId(measureBundle(path.join(tmp, "mac")).components);
    expect(c["macos-aarch64/resources/llama"].dupBytes).toBe(400);
    expect(c["macos-aarch64/resources/models"].dupBytes).toBe(0);
  });

  it("--arch overrides the arch parsed from artifact names", () => {
    const m = measureBundle(path.join(tmp, "mac"), { arch: "x86_64" });
    expect(m.target.arch).toBe("x86_64");
    expect(m.artifacts.map((a) => a.id)).toContain("macos-x86_64/dmg");
  });
});

describe("measureBundle: Linux", () => {
  it("finds AppImage and deb, and measures the AppDir payload", () => {
    const m = measureBundle(path.join(tmp, "linux"));
    expect(m.target).toEqual({ os: "linux", arch: "x86_64" });
    const a = byId(m.artifacts);
    expect(a["linux-x86_64/appimage"].bytes).toBe(6000);
    expect(a["linux-x86_64/deb"].bytes).toBe(5500);
    const c = byId(m.components);
    expect(c["linux-x86_64/bin/citrate-core"].bytes).toBe(1100);
    expect(c["linux-x86_64/bin/hermes"].bytes).toBe(310);
    // resources come from usr/lib/<product dir named like the AppDir>, not the system lib dir
    expect(c["linux-x86_64/resources/llama"].bytes).toBe(130);
    expect(c["linux-x86_64/resources/models"].bytes).toBe(800);
    expect(c["linux-x86_64/resources/libfoo.so"]).toBeUndefined();
    expect(c["linux-x86_64/resources/helper.so"]).toBeUndefined();
  });
});

describe("measureBundle: errors", () => {
  it("throws on a missing dir and on a dir with no installer artifacts", () => {
    expect(() => measureBundle(path.join(tmp, "nope"))).toThrow(/not a directory/);
    expect(() => measureBundle(path.join(tmp, "empty"))).toThrow(/no installer artifacts/);
  });

  it("refuses to guess when no artifact name carries an arch, instead of passing every row as unbudgeted", () => {
    // Only the updater was built and its name has no arch: without this refusal every row would
    // be "macos-unknown/...", none would be gated, and the check would exit 0.
    put("noarch/macos/Citrate Core.app.tar.gz", 4000);
    expect(() => measureBundle(path.join(tmp, "noarch"))).toThrow(/--arch/);
    expect(measureBundle(path.join(tmp, "noarch"), { arch: "aarch64" }).target.arch).toBe("aarch64");
    const r = runCli(["--bundle-dir", path.join(tmp, "noarch"), "--budgets", path.join(repoRoot, "release", "budgets.json")]);
    expect(r.status).toBe(2);
    expect(r.stderr).toMatch(/--arch/);
  });
});

describe("measureBundle: large files", () => {
  it("measures a component file over 2 GiB (a bundled model) without reading it whole into memory", () => {
    // Sparse file: no disk blocks are written. fs.readFileSync refuses files over 2 GiB.
    const big = path.join(tmp, "big/macos/Citrate Core.app/Contents/Resources/models/m.gguf");
    put("big/dmg/Citrate Core_0.5.0_aarch64.dmg", 10);
    put("big/macos/Citrate Core.app/Contents/MacOS/citrate-core", 10);
    fs.mkdirSync(path.dirname(big), { recursive: true });
    const size = 2 ** 31 + 4096;
    fs.closeSync(fs.openSync(big, "w"));
    fs.truncateSync(big, size);
    const c = byId(measureBundle(path.join(tmp, "big")).components);
    expect(c["macos-aarch64/resources/models"].bytes).toBe(size);
    expect(c["macos-aarch64/resources/models"].dupBytes).toBe(0);
  });

  it("hashes every chunk of same-size files: a difference past the first chunk is not a duplicate", () => {
    const n = 9 * 1024 * 1024; // larger than one 8 MiB read
    const a = Buffer.alloc(n, 0x07);
    const b = Buffer.from(a);
    b[n - 1] = 0x08;
    const d = path.join(tmp, "chunks");
    fs.mkdirSync(d, { recursive: true });
    fs.writeFileSync(path.join(d, "a.bin"), a);
    fs.writeFileSync(path.join(d, "b.bin"), b);
    fs.writeFileSync(path.join(d, "c.bin"), a);
    expect(measurePath(d)).toEqual({ bytes: 3 * n, files: 3, dupBytes: n });
  });
});

describe("validateBudgets", () => {
  it("accepts a well-formed file and rejects bad shapes", () => {
    expect(() => validateBudgets({ version: 1, artifacts: {}, components: {} })).not.toThrow();
    expect(() => validateBudgets({ version: 2, artifacts: {}, components: {} })).toThrow(/version/);
    expect(() => validateBudgets({ version: 1, artifacts: { x: { maxBytes: -1 } }, components: {} })).toThrow(
      /maxBytes/,
    );
    expect(() => validateBudgets({ version: 1, artifacts: { x: { maxBytes: 1.5 } }, components: {} })).toThrow(
      /maxBytes/,
    );
    expect(() => validateBudgets({ version: 1, artifacts: { x: {} }, components: {} })).toThrow(/maxBytes/);
    // null = measured and reported, not gated (no baseline yet)
    expect(() => validateBudgets({ version: 1, artifacts: { x: { maxBytes: null } }, components: {} })).not.toThrow();
  });
});

describe("checkBudgets", () => {
  const budgets = {
    version: 1,
    artifacts: {
      "macos-aarch64/dmg": { maxBytes: 5000 },
      "macos-aarch64/app.tar.gz": { maxBytes: 3999 },
      "linux-x86_64/appimage": { maxBytes: 9000 },
    },
    components: {
      "macos-aarch64/resources/models": { maxBytes: null },
      "macos-aarch64/bin/hermes": { maxBytes: 1000 },
    },
  };

  it("marks ok at exactly the budget, over by one byte, tracked for null, unbudgeted otherwise", () => {
    const r = checkBudgets(measureBundle(path.join(tmp, "mac")), budgets);
    const rows = byId(r.rows);
    expect(rows["macos-aarch64/dmg"].status).toBe("ok");
    expect(rows["macos-aarch64/app.tar.gz"].status).toBe("over");
    expect(rows["macos-aarch64/resources/models"].status).toBe("tracked");
    expect(rows["macos-aarch64/bin/hermes"].status).toBe("ok");
    expect(rows["macos-aarch64/bin/citrate-core"].status).toBe("unbudgeted");
    expect(r.ok).toBe(false);
    expect(r.over.map((x) => x.id)).toEqual(["macos-aarch64/app.tar.gz"]);
  });

  it("lists budgeted ids for this target that were not built, and ignores other targets", () => {
    const r = checkBudgets(measureBundle(path.join(tmp, "mac")), {
      version: 1,
      artifacts: { "macos-aarch64/dmg": { maxBytes: 9999 }, "macos-aarch64/appimage": { maxBytes: 1 } },
      components: {},
    });
    expect(r.notBuilt).toEqual(["macos-aarch64/appimage"]);
    expect(r.ok).toBe(true);
  });

  it("fails on unbudgeted rows only under strict", () => {
    const b = { version: 1, artifacts: { "macos-aarch64/dmg": { maxBytes: 9999 } }, components: {} };
    expect(checkBudgets(measureBundle(path.join(tmp, "mac")), b).ok).toBe(true);
    expect(checkBudgets(measureBundle(path.join(tmp, "mac")), b, { strict: true }).ok).toBe(false);
  });
});

describe("rendering", () => {
  it("formats bytes as decimal MB with the exact byte count", () => {
    expect(formatBytes(394_782_331)).toBe("394.8 MB");
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(12_345)).toBe("12.3 kB");
  });

  it("prints a table with a header, every row, and the verdict", () => {
    const r = checkBudgets(measureBundle(path.join(tmp, "mac")), {
      version: 1,
      artifacts: { "macos-aarch64/dmg": { maxBytes: 4000 } },
      components: {},
    });
    const t = renderTable(r);
    expect(t).toMatch(/id\s+size\s+budget\s+use\s+dup\s+status/);
    expect(t).toContain("macos-aarch64/dmg");
    expect(t).toContain("OVER");
    expect(t).toMatch(/1 over budget/);
    const md = renderMarkdown(r);
    expect(md).toContain("| id | size | budget | use | dup | status |");
    expect(md).toContain("| macos-aarch64/dmg | 5,000 B");
  });
});

describe("CLI exit codes", () => {
  it("exits 0 within budget and prints the table", () => {
    const p = writeBudgets("ok.json", {
      version: 1,
      artifacts: { "macos-aarch64/dmg": { maxBytes: 5000 }, "macos-aarch64/app.tar.gz": { maxBytes: 5000 } },
      components: {},
    });
    const r = runCli(["--bundle-dir", path.join(tmp, "mac"), "--budgets", p]);
    expect(r.status).toBe(0);
    expect(r.stdout).toContain("macos-aarch64/dmg");
  });

  it("exits 1 over budget and writes the markdown + json reports", () => {
    const p = writeBudgets("over.json", {
      version: 1,
      artifacts: { "macos-aarch64/dmg": { maxBytes: 4999 } },
      components: {},
    });
    const md = path.join(tmp, "report.md");
    const js = path.join(tmp, "report.json");
    const r = runCli(["--bundle-dir", path.join(tmp, "mac"), "--budgets", p, "--markdown", md, "--json", js]);
    expect(r.status).toBe(1);
    expect(fs.readFileSync(md, "utf8")).toContain("over budget");
    const j = JSON.parse(fs.readFileSync(js, "utf8"));
    expect(j.ok).toBe(false);
    expect(j.over[0].id).toBe("macos-aarch64/dmg");
  });

  it("exits 1 on unbudgeted rows with --strict", () => {
    const p = writeBudgets("strict.json", {
      version: 1,
      artifacts: { "macos-aarch64/dmg": { maxBytes: 5000 }, "macos-aarch64/app.tar.gz": { maxBytes: 5000 } },
      components: {},
    });
    expect(runCli(["--bundle-dir", path.join(tmp, "mac"), "--budgets", p, "--strict"]).status).toBe(1);
  });

  it("exits 2 on an empty bundle dir, a bad flag, or a broken budgets file", () => {
    const p = writeBudgets("fine.json", { version: 1, artifacts: {}, components: {} });
    expect(runCli(["--bundle-dir", path.join(tmp, "empty"), "--budgets", p]).status).toBe(2);
    expect(runCli(["--nope"]).status).toBe(2);
    const bad = writeBudgets("bad.json", { version: 1, artifacts: { x: { maxBytes: "big" } }, components: {} });
    expect(runCli(["--bundle-dir", path.join(tmp, "mac"), "--budgets", bad]).status).toBe(2);
  });
});

describe("committed release/budgets.json", () => {
  const file = path.join(repoRoot, "release", "budgets.json");

  it("is valid", () => {
    expect(() => loadBudgets(file)).not.toThrow();
  });

  // HUP-S11.0, 2026-10-04: a local bundle-lite build of the 0.5.0 line (core hup/m2-core with the
  // knowledge corpus, the reviewed skills, the M2 Hermes sidecar and the licence texts). The
  // updater row is a `tar -czf` of the .app (the build ran without the updater key), so it is an
  // estimate of Tauri's own tarball. Budgets set from these: pending owner sign-off.
  const MEASURED_2026_10_04 = {
    dmg: 431_684_435,
    appTarGz: 435_081_567,
    app: 852_410_676,
    hermes: 29_442_128,
    citrateCore: 35_851_840,
    licenses: 142_493,
  };

  it("gates the macOS installer and updater at or above both the shipped v0.4.2 and the measured 0.5.0 sizes", () => {
    const b = loadBudgets(file);
    const dmg = b.artifacts["macos-aarch64/dmg"];
    const tgz = b.artifacts["macos-aarch64/app.tar.gz"];
    expect(V042_SHIPPED.dmg).toBe(394_782_331);
    expect(V042_SHIPPED.appTarGz).toBe(366_205_330);
    expect(dmg.maxBytes).toBeGreaterThanOrEqual(V042_SHIPPED.dmg);
    expect(tgz.maxBytes).toBeGreaterThanOrEqual(V042_SHIPPED.appTarGz);
    expect(dmg.maxBytes).toBeGreaterThanOrEqual(MEASURED_2026_10_04.dmg);
    expect(tgz.maxBytes).toBeGreaterThanOrEqual(MEASURED_2026_10_04.appTarGz);
    // headroom stays tight: a budget is a tripwire, not a ceiling nobody reaches
    expect(dmg.maxBytes).toBeLessThanOrEqual(Math.ceil(MEASURED_2026_10_04.dmg * 1.1));
    expect(tgz.maxBytes).toBeLessThanOrEqual(Math.ceil(MEASURED_2026_10_04.appTarGz * 1.1));
    expect(dmg.baselineBytes).toBe(MEASURED_2026_10_04.dmg);
    expect(tgz.baselineBytes).toBe(MEASURED_2026_10_04.appTarGz);
  });

  it("budgets every component the measured 0.5.0 build reported, with tight headroom on the re-measured rows", () => {
    const b = loadBudgets(file);
    const ids = [
      "bin/citrate", "bin/citrate-core", "bin/cluster-daemon", "bin/comms-member-daemon", "bin/hermes",
      "bin/ipfs", "bin/llama-server", "bin/mem-mcp", "bin/node-agent", "resources/capsules",
      "resources/docs-corpus", "resources/icon.icns", "resources/knowledge-corpus", "resources/licenses",
      "resources/llama", "resources/models", "resources/skills", "resources/skills-bundle", "app",
    ];
    for (const id of ids) {
      const row = b.components[`macos-aarch64/${id}`];
      expect(row, id).toBeDefined();
      expect(Number.isInteger(row.maxBytes), id).toBe(true);
    }
    const tight = [
      ["app", MEASURED_2026_10_04.app],
      ["bin/hermes", MEASURED_2026_10_04.hermes],
      ["bin/citrate-core", MEASURED_2026_10_04.citrateCore],
    ];
    for (const [id, bytes] of tight) {
      const row = b.components[`macos-aarch64/${id}`];
      expect(row.baselineBytes, id).toBe(bytes);
      expect(row.maxBytes, id).toBeGreaterThanOrEqual(bytes);
      expect(row.maxBytes, id).toBeLessThanOrEqual(Math.ceil(bytes * 1.1) + 1_000_000);
    }
    expect(b.components["macos-aarch64/resources/licenses"].baselineBytes).toBe(MEASURED_2026_10_04.licenses);
  });

  // L14 release prep, 2026-10-04: the same build line re-measured after the llama runtime prune
  // (scripts/prune-llama-runtime.mjs) and the Hermes strip -x (scripts/build-hermes.sh). Recorded
  // as measuredBytes; the budgets themselves are unchanged (owner sign-off).
  const MEASURED_AFTER_PREP = {
    "artifacts/macos-aarch64/dmg": 420_878_903,
    "artifacts/macos-aarch64/app.tar.gz": 424_350_415,
    "components/macos-aarch64/app": 818_552_274,
    "components/macos-aarch64/bin/hermes": 25_974_928,
    "components/macos-aarch64/bin/citrate-core": 38_834_896,
    "components/macos-aarch64/resources/llama": 25_515_197,
    "components/macos-aarch64/resources/licenses": 798_515,
  };

  it("records the post-prep re-measurement within the unchanged budgets", () => {
    const b = loadBudgets(file);
    for (const [key, bytes] of Object.entries(MEASURED_AFTER_PREP)) {
      const [section, ...rest] = key.split("/");
      const row = b[section][rest.join("/")];
      expect(row.measuredBytes, key).toBe(bytes);
      expect(row.measured, key).toMatch(/pending owner sign-off/);
      expect(row.maxBytes, key).toBeGreaterThanOrEqual(bytes);
    }
    // The budgets were not moved by the re-measurement.
    expect(b.artifacts["macos-aarch64/dmg"].maxBytes).toBe(455_000_000);
    expect(b.artifacts["macos-aarch64/app.tar.gz"].maxBytes).toBe(460_000_000);
    expect(b.components["macos-aarch64/app"].maxBytes).toBe(938_000_000);
    // The savings the prep was for: the llama runtime and the Hermes sidecar.
    const llama = b.components["macos-aarch64/resources/llama"];
    expect(llama.baselineBytes - llama.measuredBytes).toBeGreaterThan(30_000_000);
    expect(MEASURED_2026_10_04.hermes - b.components["macos-aarch64/bin/hermes"].measuredBytes).toBeGreaterThan(3_000_000);
    expect(MEASURED_2026_10_04.app - b.components["macos-aarch64/app"].measuredBytes).toBeGreaterThan(30_000_000);
  });

  it("names a budget row for every OS the release plan ships (macOS, Linux, Windows)", () => {
    const ids = Object.keys(loadBudgets(file).artifacts);
    expect(ids.some((i) => i.startsWith("macos-"))).toBe(true);
    expect(ids.some((i) => i.startsWith("linux-"))).toBe(true);
    expect(ids.some((i) => i.startsWith("windows-"))).toBe(true);
  });
});
