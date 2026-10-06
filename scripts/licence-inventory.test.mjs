// @vitest-environment node
//
// HUP gate g3-licence: release/licences.json must name a licence for everything the app ships
// (installer sidecars and resources), downloads on first run (toolchain, Solidity libraries) or
// carries as text (skills bundle, knowledge corpus), and every licence text it names must exist.
// These tests run the real check against the committed inventory and against small fixture
// repos built in a temp dir, one defect per test.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkInventory,
  corpusSourceIds,
  requiredKeys,
  resourceKey,
  skillsLockLabels,
} from "./licence-inventory.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const SCRIPT = path.join(here, "licence-inventory.mjs");
const committed = JSON.parse(fs.readFileSync(path.join(repoRoot, "release", "licences.json"), "utf8"));

const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function write(root, rel, body) {
  const p = path.join(root, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, typeof body === "string" ? body : JSON.stringify(body, null, 2));
}

/** A minimal repo: one bundle config, one tool, one library, one skills source, one licence text. */
function fixtureRepo() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "n6-licence-"));
  tmpDirs.push(root);
  write(root, "src-tauri/tauri.bundle-lite.conf.json", {
    bundle: { externalBin: ["binaries/ipfs"], resources: ["llama/*", "licenses/*", "knowledge-corpus/**/*"] },
  });
  write(root, "components/toolchain-bundle.json", { tools: [{ name: "slither" }] });
  write(root, "templates/deps.lock.json", { deps: { solady: { license: "MIT" } } });
  write(
    root,
    "skills.lock",
    '# lock\nversion = 1\n\n[[source]]\nlabel = "trailofbits"\nlicense = "CC-BY-SA-4.0"\n\n[[skill]]\nsource = "trailofbits"\n',
  );
  write(root, "src-tauri/licenses/kubo.LICENSE", "MIT text\n");
  write(root, "src-tauri/licenses/llama.cpp.LICENSE", "MIT text\n");
  write(root, "src-tauri/licenses/tob.LICENSE", "CC BY-SA text\n");
  write(root, "src-tauri/licenses/README.md", "index\n");
  return root;
}

function fixtureInventory() {
  const base = { first_party: false, copyleft: "none", licence_texts: [], source_offer: null, review: "ok" };
  return {
    version: 1,
    sign_off: { status: "pending owner sign-off", by: null, date: null },
    components: [
      { ...base, id: "kubo", spdx: "MIT OR Apache-2.0", ships_as: "installer", covers: ["externalBin:binaries/ipfs"], licence_texts: ["src-tauri/licenses/kubo.LICENSE"] },
      { ...base, id: "llama", spdx: "MIT", ships_as: "installer", covers: ["resource:llama"], licence_texts: ["src-tauri/licenses/llama.cpp.LICENSE"] },
      { ...base, id: "texts", first_party: true, spdx: "LicenseRef-various", ships_as: "installer", covers: ["resource:licenses"] },
      { ...base, id: "corpus", spdx: "LicenseRef-per-source", ships_as: "installer", covers: ["resource:knowledge-corpus"] },
      {
        ...base,
        id: "slither",
        spdx: "AGPL-3.0-only",
        ships_as: "first-run-download",
        copyleft: "network",
        covers: ["toolchain:slither"],
        source_offer: "upstream at the pinned release",
        review: "owner",
        gap: "mirror or upstream: owner call",
      },
      { ...base, id: "solady", spdx: "MIT", ships_as: "first-run-download", covers: ["library:solady", "corpus:solady"] },
      { ...base, id: "tob", spdx: "CC-BY-SA-4.0", ships_as: "skills-bundle", copyleft: "weak", covers: ["skills:trailofbits"], source_offer: "upstream", licence_texts: ["src-tauri/licenses/tob.LICENSE"] },
      { ...base, id: "searxng", spdx: "AGPL-3.0-or-later", ships_as: "not-shipped", copyleft: "network", covers: ["planned:searxng"], review: "owner", gap: "not shipped yet" },
    ],
  };
}

const errorsOf = (inv, root, opts = {}) => checkInventory(inv, { repoRoot: root, ...opts }).errors;

describe("resourceKey", () => {
  it("drops the glob tail and keeps a plain file as is", () => {
    expect(resourceKey("llama/*")).toBe("llama");
    expect(resourceKey("skills/**/*")).toBe("skills");
    expect(resourceKey("models/bge-base-en-v1.5/*")).toBe("models/bge-base-en-v1.5");
    expect(resourceKey("capsules/echo-chain/*")).toBe("capsules/echo-chain");
    expect(resourceKey("models/gemma-4-E4B-it-Q4_0.gguf")).toBe("models/gemma-4-E4B-it-Q4_0.gguf");
  });
});

describe("the committed inventory", () => {
  it("covers every sidecar, resource, tool, library and skills source in this repo, with no error", () => {
    const r = checkInventory(committed, { repoRoot });
    expect(r.errors).toEqual([]);
  });

  it("requires the sidecars and resources of every bundle config, including the release (node) overlay", () => {
    const req = requiredKeys(repoRoot);
    for (const k of [
      "externalBin:binaries/citrate",
      "externalBin:binaries/hermes",
      "externalBin:binaries/ipfs",
      "resource:knowledge-corpus",
      "resource:licenses",
      "resource:models/gemma-4-E4B-it-Q4_0.gguf",
      "toolchain:slither",
      "toolchain:medusa",
      "library:openzeppelin-contracts",
      "skills:trailofbits",
    ]) {
      expect(req.has(k), k).toBe(true);
    }
  });

  it("names Medusa and Slither docs as AGPL corpus sources with a source offer", () => {
    for (const id of ["medusa-docs", "slither-docs"]) {
      const c = committed.components.find((x) => x.id === id);
      expect(c.spdx).toBe("AGPL-3.0-only");
      expect(c.covers).toContain(`corpus:${id}`);
      expect(c.source_offer).toMatch(/github\.com\/crytic\//);
      expect(c.licence_texts.length).toBeGreaterThan(0);
    }
  });

  it("stays pending owner sign-off, so --require-sign-off fails", () => {
    expect(committed.sign_off.status).toBe("pending owner sign-off");
    const r = spawnSync(process.execPath, [SCRIPT, "--require-sign-off"], { cwd: repoRoot, encoding: "utf8" });
    expect(r.status).toBe(1);
    expect(r.stderr + r.stdout).toMatch(/sign-off/);
  });
});

describe("checkInventory on a fixture repo", () => {
  it("passes a complete inventory", () => {
    const root = fixtureRepo();
    expect(errorsOf(fixtureInventory(), root)).toEqual([]);
  });

  it("fails a sidecar with no licence entry", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components = inv.components.filter((c) => c.id !== "kubo");
    fs.rmSync(path.join(root, "src-tauri/licenses/kubo.LICENSE"));
    expect(errorsOf(inv, root).join("\n")).toMatch(/externalBin:binaries\/ipfs.*no licence entry/);
  });

  it("fails a key covered twice", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components[1].covers.push("externalBin:binaries/ipfs");
    expect(errorsOf(inv, root).join("\n")).toMatch(/externalBin:binaries\/ipfs.*covered by kubo and llama/);
  });

  it("fails a stale cover that matches nothing the repo ships", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components[4].covers.push("toolchain:aderyn");
    expect(errorsOf(inv, root).join("\n")).toMatch(/toolchain:aderyn.*not shipped by this repo/);
  });

  it("fails a licence text that does not exist", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components[1].licence_texts = ["src-tauri/licenses/llama.LICENSE"];
    expect(errorsOf(inv, root).join("\n")).toMatch(/llama\.LICENSE.*does not exist/);
  });

  it("fails a bundled licence text that no entry names", () => {
    const root = fixtureRepo();
    write(root, "src-tauri/licenses/orphan.LICENSE", "x\n");
    expect(errorsOf(fixtureInventory(), root).join("\n")).toMatch(/orphan\.LICENSE.*no entry names it/);
  });

  it("fails a bundle config that does not ship the licence texts", () => {
    const root = fixtureRepo();
    write(root, "src-tauri/tauri.bundle-linux.conf.json", { bundle: { externalBin: ["binaries/ipfs"], resources: ["llama/*"] } });
    expect(errorsOf(fixtureInventory(), root).join("\n")).toMatch(/tauri\.bundle-linux\.conf\.json.*licenses\/\*/);
  });

  it("fails a shipped copyleft component with no source offer, but not a planned one", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components[4].source_offer = null;
    expect(errorsOf(inv, root).join("\n")).toMatch(/slither.*source_offer/);
    const ok = fixtureInventory();
    expect(ok.components[7].source_offer).toBeNull();
    expect(errorsOf(ok, root)).toEqual([]);
  });

  it("fails an entry with no SPDX expression, an unknown review state, or a gap-less owner item", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components[0].spdx = "";
    inv.components[1].review = "fine";
    const errs = errorsOf(inv, root).join("\n");
    expect(errs).toMatch(/kubo.*spdx/);
    expect(errs).toMatch(/llama.*review/);
    const inv2 = fixtureInventory();
    delete inv2.components[4].gap;
    expect(errorsOf(inv2, root).join("\n")).toMatch(/slither.*gap/);
  });

  it("fails a shipped third-party component that names no bundled licence text", () => {
    // The class of the Kubo / BGE finding: an entry with no text passes every other check.
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components[0].licence_texts = [];
    fs.rmSync(path.join(root, "src-tauri/licenses/kubo.LICENSE"));
    expect(errorsOf(inv, root).join("\n")).toMatch(/kubo: shipped third-party component names no licence text under src-tauri\/licenses/);
    // A text outside src-tauri/licenses/ does not ship in the installer, so it does not count.
    const inv2 = fixtureInventory();
    write(root, "components/licenses/tob.LICENSE", "x\n");
    inv2.components[6].licence_texts = ["components/licenses/tob.LICENSE"];
    write(root, "src-tauri/licenses/kubo.LICENSE", "MIT text\n");
    fs.rmSync(path.join(root, "src-tauri/licenses/tob.LICENSE"));
    expect(errorsOf(inv2, root).join("\n")).toMatch(/tob: shipped third-party component names no licence text/);
    // First-party, first-run downloads, planned items and per-source umbrellas are exempt.
    const ok = fixtureInventory();
    write(root, "src-tauri/licenses/tob.LICENSE", "CC BY-SA text\n");
    expect(ok.components.find((c) => c.id === "slither").licence_texts).toEqual([]);
    expect(ok.components.find((c) => c.id === "corpus").licence_texts).toEqual([]);
    expect(errorsOf(ok, root)).toEqual([]);
  });

  it("fails an entry whose first_party is not a boolean", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    delete inv.components[2].first_party;
    expect(errorsOf(inv, root).join("\n")).toMatch(/texts: first_party must be true or false/);
  });

  it("checks corpus sources against a staged manifest: missing, extra and excluded sources", () => {
    const root = fixtureRepo();
    const manifest = {
      sources: [
        { id: "solady", included: true },
        { id: "medusa-docs", included: true },
        { id: "never-read", included: false },
      ],
    };
    const errs = errorsOf(fixtureInventory(), root, { corpusSources: corpusSourceIds(manifest) }).join("\n");
    expect(errs).toMatch(/corpus:medusa-docs.*no licence entry/);
    expect(errs).not.toMatch(/never-read/);
    const inv = fixtureInventory();
    inv.components[5].covers.push("corpus:gone");
    const errs2 = errorsOf(inv, root, { corpusSources: ["solady"] }).join("\n");
    expect(errs2).toMatch(/corpus:gone.*not in the corpus manifest/);
  });
});

describe("skillsLockLabels", () => {
  it("reads only [[source]] labels", () => {
    const text = '[[source]]\nlabel = "a"\n\n[[skill]]\nlabel = "not-a-source"\n\n[[source]]\nlabel = "b"\n';
    expect(skillsLockLabels(text)).toEqual(["a", "b"]);
  });
});

describe("CLI", () => {
  it("exits 0 on the committed repo and prints the review summary", () => {
    const r = spawnSync(process.execPath, [SCRIPT], { cwd: repoRoot, encoding: "utf8" });
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
    expect(r.stdout).toMatch(/licence inventory: OK/);
    expect(r.stdout).toMatch(/owner decisions open: \d+/);
  });

  it("exits 1 on a defect and 2 on a usage error", () => {
    const root = fixtureRepo();
    const inv = fixtureInventory();
    inv.components = inv.components.filter((c) => c.id !== "llama");
    write(root, "release/licences.json", inv);
    const bad = spawnSync(process.execPath, [SCRIPT, "--repo", root], { encoding: "utf8" });
    expect(bad.status).toBe(1);
    expect(bad.stderr).toMatch(/resource:llama/);
    const usage = spawnSync(process.execPath, [SCRIPT, "--bogus"], { cwd: repoRoot, encoding: "utf8" });
    expect(usage.status).toBe(2);
    const noCorpus = spawnSync(process.execPath, [SCRIPT, "--corpus", path.join(root, "nope")], { cwd: repoRoot, encoding: "utf8" });
    expect(noCorpus.status).toBe(2);
  });
});

describe("third-party notices (scripts/third-party-notices.mjs output)", () => {
  /** The fixture plus a first-party sidecar, an app entry and a generated notices file. */
  function withSidecar({ notices = true, appNotices = true } = {}) {
    const root = fixtureRepo();
    write(root, "src-tauri/tauri.bundle-lite.conf.json", {
      bundle: { externalBin: ["binaries/ipfs", "binaries/hermes"], resources: ["llama/*", "licenses/*", "knowledge-corpus/**/*"] },
    });
    write(root, "src-tauri/licenses/THIRD-PARTY-NOTICES.txt", "Citrate Core: third-party notices\n");
    const inv = fixtureInventory();
    const n = { file: "src-tauri/licenses/THIRD-PARTY-NOTICES.txt", source: "repo@abc", packages: 3 };
    inv.app = { name: "app", spdx: "BUSL-1.1", licence_texts: [], ...(appNotices ? { third_party_notices: n } : {}) };
    inv.components.push({
      id: "hermes",
      spdx: "Apache-2.0",
      first_party: true,
      ships_as: "installer",
      covers: ["externalBin:binaries/hermes"],
      copyleft: "none",
      licence_texts: [],
      source_offer: null,
      review: "ok",
      ...(notices ? { third_party_notices: n } : {}),
    });
    return { root, inv };
  }

  it("accepts a notices file under src-tauri/licenses and counts it as named", () => {
    const { root, inv } = withSidecar();
    expect(errorsOf(inv, root)).toEqual([]);
    expect(errorsOf(inv, root, { requireNotices: true })).toEqual([]);
  });

  it("fails a notices file that does not exist or lies outside the bundled licence dir", () => {
    const { root, inv } = withSidecar();
    fs.rmSync(path.join(root, "src-tauri/licenses/THIRD-PARTY-NOTICES.txt"));
    expect(errorsOf(inv, root).join("\n")).toMatch(/hermes: third-party notices .* do not exist/);

    const b = withSidecar();
    b.inv.components.at(-1).third_party_notices = { file: "release/NOTICES.txt" };
    expect(errorsOf(b.inv, b.root).join("\n")).toMatch(/hermes: third_party_notices\.file must be a file under src-tauri\/licenses/);
  });

  it("fails an unnamed notices file like any other bundled text", () => {
    const { root, inv } = withSidecar({ notices: false, appNotices: false });
    expect(errorsOf(inv, root)).toContain("src-tauri/licenses/THIRD-PARTY-NOTICES.txt: bundled licence text, but no entry names it");
  });

  it("--require-notices fails a first-party sidecar or an app without notices, and only then", () => {
    const a = withSidecar({ notices: false });
    expect(errorsOf(a.inv, a.root)).toEqual([]);
    expect(errorsOf(a.inv, a.root, { requireNotices: true })).toEqual(["hermes: no third_party_notices for the packages compiled into this sidecar"]);

    const b = withSidecar({ appNotices: false });
    expect(errorsOf(b.inv, b.root, { requireNotices: true })).toEqual(["app: no third_party_notices for the crates compiled into the app"]);
  });

  it("the committed inventory carries notices for the app and every first-party sidecar", () => {
    expect(checkInventory(committed, { repoRoot, requireNotices: true }).errors).toEqual([]);
  });

  it("CLI: --require-notices is accepted on the committed repo", () => {
    const r = spawnSync(process.execPath, [SCRIPT, "--require-notices"], { cwd: repoRoot, encoding: "utf8" });
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
  });
});

// v0.5.0 gate prep (g3-licence): the webview bundle (the frontendDist Vite builds) ships the npm
// packages it imports, so the inventory must know it ships and carry its third-party notices.
describe("the webview bundle (frontendDist)", () => {
  function withWebview({ notices = true } = {}) {
    const root = fixtureRepo();
    write(root, "src-tauri/tauri.conf.json", { build: { frontendDist: "../dist" }, bundle: { resources: [] } });
    write(root, "src-tauri/licenses/THIRD-PARTY-NOTICES.txt", "Citrate Core: third-party notices\n");
    const inv = fixtureInventory();
    const n = { file: "src-tauri/licenses/THIRD-PARTY-NOTICES.txt", source: "repo@abc", packages: 3 };
    inv.app = { name: "app", spdx: "BUSL-1.1", licence_texts: [], third_party_notices: n };
    inv.components.push({
      id: "app-webview",
      spdx: "BUSL-1.1",
      first_party: true,
      ships_as: "installer",
      covers: ["frontendDist:../dist"],
      copyleft: "none",
      licence_texts: [],
      source_offer: null,
      review: "ok",
      ...(notices ? { third_party_notices: n } : {}),
    });
    return { root, inv };
  }

  it("requires the frontendDist of tauri.conf.json, so the webview cannot ship without an entry", () => {
    expect(requiredKeys(repoRoot).get("frontendDist:../dist")).toBe("tauri.conf.json");
    const { root, inv } = withWebview();
    expect(errorsOf(inv, root)).toEqual([]);
    inv.components.pop();
    expect(errorsOf(inv, root)).toContain("frontendDist:../dist (from tauri.conf.json): no licence entry");
  });

  it("--require-notices fails a webview entry with no third-party notices", () => {
    const { root, inv } = withWebview({ notices: false });
    expect(errorsOf(inv, root)).toEqual([]);
    expect(errorsOf(inv, root, { requireNotices: true })).toEqual([
      "app-webview: no third_party_notices for the packages compiled into this bundle",
    ]);
  });
});
