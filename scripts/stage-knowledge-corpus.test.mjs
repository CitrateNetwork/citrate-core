// @vitest-environment node
//
// HUP-S3.1: release staging gate for the bundled knowledge corpus
// (scripts/stage-knowledge-corpus.mjs).
//
// The committed fixture corpus (src-tauri/tests/fixtures/knowledge-corpus, a byte-exact copy of
// the citrate-memories mem-corpus golden bundle, format citrate-corpus/2) is copied to a temp dir
// and then tampered with one rule at a time. The CLI runs as a child process to pin its exit
// contract: 0 staged, 1 refused, 2 usage.
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkEmbedder, checkSourceCommits, computeDigest, listFiles, memMcpSupportsImport, stageInto, tenantFile, verifyCorpus } from "./stage-knowledge-corpus.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, "stage-knowledge-corpus.mjs");
const FIXTURE = path.resolve(here, "../src-tauri/tests/fixtures/knowledge-corpus");
const FIXTURE_DIGEST = JSON.parse(fs.readFileSync(path.join(FIXTURE, "manifest.json"), "utf8")).bundle_digest;

let tmp;
let corpus;

/** Copy the fixture corpus (without its README) into a fresh temp dir. */
function freshCorpus() {
  const dir = path.join(tmp, `corpus-${Math.random().toString(16).slice(2)}`);
  for (const f of listFiles(FIXTURE).filter((f) => f !== "README.md" && f !== ".gitattributes")) {
    fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    fs.copyFileSync(path.join(FIXTURE, f), path.join(dir, f));
  }
  return dir;
}

/** Edit the manifest and re-seal its digest, so only the rule under test can refuse it. */
function reseal(dir, edit) {
  const p = path.join(dir, "manifest.json");
  const m = JSON.parse(fs.readFileSync(p, "utf8"));
  edit(m);
  m.bundle_digest = "";
  const text = JSON.stringify(m, null, 2) + "\n";
  m.bundle_digest = computeDigest(text);
  fs.writeFileSync(p, JSON.stringify(m, null, 2) + "\n");
}

/** A BGE model directory with the three files the daemon loads (tiny stand-ins, real hashes). */
function fakeBge(dir = path.join(tmp, "bge"), { dim = 4, weights = "weights-v1" } = {}) {
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "config.json"), JSON.stringify({ hidden_size: dim }));
  fs.writeFileSync(path.join(dir, "tokenizer.json"), "{}");
  fs.writeFileSync(path.join(dir, "model.safetensors"), weights);
  return { dir, sha: createHash("sha256").update(weights).digest("hex") };
}

/** Give every tenant vectors for `bge` (dim + weights hash) and re-seal the manifest. */
function embedAll(dir, bge, dim = 4) {
  const m = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
  const entries = {};
  for (const t of m.tenants) {
    const buf = Buffer.alloc(t.nodes * dim * 2, 0x3c);
    const file = `tenants/${t.tenant}.vectors.f16`;
    fs.writeFileSync(path.join(dir, file), buf);
    entries[t.tenant] = { file, sha256: createHash("sha256").update(buf).digest("hex"), encoding: "f16le", model: "bge-base-en-v1.5", dim, weights_sha256: bge.sha };
  }
  reseal(dir, (mm) => {
    for (const t of mm.tenants) t.vectors = entries[t.tenant];
  });
}

function run(...args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
}

beforeEach(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "stage-corpus-"));
  corpus = freshCorpus();
});
afterEach(() => fs.rmSync(tmp, { recursive: true, force: true }));

describe("verifyCorpus", () => {
  it("accepts the fixture corpus and recomputes the Rust bundle_digest", () => {
    const r = verifyCorpus(corpus);
    expect(r.manifest.format).toBe("citrate-corpus/2");
    expect(r.manifest.bundle_digest).toBe(FIXTURE_DIGEST);
    expect(computeDigest(fs.readFileSync(path.join(corpus, "manifest.json"), "utf8"))).toBe(FIXTURE_DIGEST);
    expect(r.files).toContain(tenantFile("citrate-docs"));
    expect(r.files).toContain("skills.lock");
    expect(r.bytes).toBeGreaterThan(0);
  });

  it("refuses a tampered tenant file", () => {
    const p = path.join(corpus, tenantFile("refs"));
    fs.appendFileSync(p, " ");
    expect(() => verifyCorpus(corpus)).toThrow(/does not match its manifest hash/);
  });

  it("refuses a manifest edited without its digest", () => {
    const p = path.join(corpus, "manifest.json");
    fs.writeFileSync(p, fs.readFileSync(p, "utf8").replace("Apache-2.0", "MIT"));
    expect(() => verifyCorpus(corpus)).toThrow(/digest does not match/);
  });

  it("refuses a format 1 corpus even when re-sealed", () => {
    reseal(corpus, (m) => (m.format = "citrate-corpus/1"));
    expect(() => verifyCorpus(corpus)).toThrow(/unsupported corpus format/);
  });

  it("refuses a runtime tenant, a renamed tenant file and a duplicated tenant", () => {
    reseal(corpus, (m) => (m.tenants[0].tenant = "personal"));
    expect(() => verifyCorpus(corpus)).toThrow(/not a knowledge tenant/);

    corpus = freshCorpus();
    reseal(corpus, (m) => (m.tenants[0].file = "tenants/other.json"));
    expect(() => verifyCorpus(corpus)).toThrow(/file must be/);

    corpus = freshCorpus();
    reseal(corpus, (m) => m.tenants.push({ ...m.tenants[0] }));
    expect(() => verifyCorpus(corpus)).toThrow(/listed twice/);
  });

  it("refuses a skills.lock that differs from the manifest", () => {
    fs.appendFileSync(path.join(corpus, "skills.lock"), "# edited\n");
    expect(() => verifyCorpus(corpus)).toThrow(/skills.lock does not match/);
  });

  it("refuses extra files, missing files and symlinks", () => {
    fs.writeFileSync(path.join(corpus, "tenants", "extra.json"), "{}");
    expect(() => verifyCorpus(corpus)).toThrow(/unexpected file/);

    corpus = freshCorpus();
    fs.rmSync(path.join(corpus, "NOTICE.md"));
    expect(() => verifyCorpus(corpus)).toThrow(/missing file/);

    corpus = freshCorpus();
    fs.symlinkSync(path.join(corpus, "NOTICE.md"), path.join(corpus, "link.md"));
    expect(() => verifyCorpus(corpus)).toThrow(/symlink/);
  });

  it("ignores only the files it is told to (the committed README)", () => {
    fs.writeFileSync(path.join(corpus, "README.md"), "staging notes");
    expect(() => verifyCorpus(corpus)).toThrow(/unexpected file/);
    expect(verifyCorpus(corpus, { ignore: ["README.md"] }).files).not.toContain("README.md");
  });
});

describe("precomputed vectors (format 2, optional)", () => {
  /** Give one tenant a vectors file of `n` nodes x `dim` f16 values and re-seal the manifest. */
  function addVectors(dir, tenant, { dim = 4, bytes } = {}) {
    const m = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
    const t = m.tenants.find((x) => x.tenant === tenant);
    const buf = bytes ?? Buffer.alloc(t.nodes * dim * 2, 0x3c);
    const file = `tenants/${tenant}.vectors.f16`;
    fs.writeFileSync(path.join(dir, file), buf);
    const sha = createHash("sha256").update(buf).digest("hex");
    reseal(dir, (mm) => {
      mm.tenants.find((x) => x.tenant === tenant).vectors = {
        file,
        sha256: sha,
        encoding: "f16le",
        model: "bge-base-en-v1.5",
        dim,
        weights_sha256: "0".repeat(64),
      };
    });
    return file;
  }

  it("accepts and stages a tenant's vectors file", () => {
    const file = addVectors(corpus, "refs");
    const r = verifyCorpus(corpus);
    expect(r.files).toContain(file);
    const dest = path.join(tmp, "dest");
    stageInto(corpus, dest, r.files);
    expect(fs.existsSync(path.join(dest, file))).toBe(true);
  });

  it("refuses a vectors file that is tampered, mis-sized, misnamed or oddly encoded", () => {
    const file = addVectors(corpus, "refs");
    fs.appendFileSync(path.join(corpus, file), Buffer.from([0]));
    expect(() => verifyCorpus(corpus)).toThrow(/does not match its manifest hash/);

    corpus = freshCorpus();
    addVectors(corpus, "refs", { bytes: Buffer.alloc(6, 0x3c) });
    expect(() => verifyCorpus(corpus)).toThrow(/expected/);

    corpus = freshCorpus();
    addVectors(corpus, "refs");
    reseal(corpus, (m) => (m.tenants.find((x) => x.tenant === "refs").vectors.file = "tenants/other.f16"));
    expect(() => verifyCorpus(corpus)).toThrow(/vectors file must be/);

    corpus = freshCorpus();
    addVectors(corpus, "refs");
    reseal(corpus, (m) => (m.tenants.find((x) => x.tenant === "refs").vectors.encoding = "f32le"));
    expect(() => verifyCorpus(corpus)).toThrow(/encoding/);
  });
});

describe("the bundled BGE embedder is a hard dependency of the corpus (US-3.1 offline)", () => {
  it("accepts a corpus whose every tenant carries vectors made with the bundled weights", () => {
    const bge = fakeBge();
    embedAll(corpus, bge);
    const { manifest } = verifyCorpus(corpus);
    expect(checkEmbedder(manifest, bge.dir)).toEqual({ model: "bge-base-en-v1.5", dim: 4, weightsSha256: bge.sha, embeddedNodes: manifest.tenants.reduce((n, t) => n + t.nodes, 0) });
  });

  it("refuses when the BGE model is missing or partial: the first-run import would skip as not-semantic", () => {
    const bge = fakeBge();
    embedAll(corpus, bge);
    const { manifest } = verifyCorpus(corpus);
    expect(() => checkEmbedder(manifest, path.join(tmp, "nowhere"))).toThrow(/not-semantic/);
    fs.rmSync(path.join(bge.dir, "tokenizer.json"));
    expect(() => checkEmbedder(manifest, bge.dir)).toThrow(/tokenizer\.json/);
  });

  it("refuses vectors made with other weights or another dimension: the importer would ignore them", () => {
    embedAll(corpus, fakeBge(path.join(tmp, "old"), { weights: "weights-v0" }));
    const { manifest } = verifyCorpus(corpus);
    expect(() => checkEmbedder(manifest, fakeBge().dir)).toThrow(/other model weights/);
    const wide = fakeBge(path.join(tmp, "wide"), { dim: 8, weights: "weights-v0" });
    expect(() => checkEmbedder(manifest, wide.dir)).toThrow(/dimension/);
  });

  it("refuses a tenant without vectors unless unembedded staging is asked for, and says what it costs", () => {
    const bge = fakeBge();
    const { manifest } = verifyCorpus(corpus);
    expect(() => checkEmbedder(manifest, bge.dir)).toThrow(/no precomputed vectors.*each member's first launch would embed \d+ nodes on their own CPU/);
    expect(checkEmbedder(manifest, bge.dir, { allowUnembedded: true }).embeddedNodes).toBe(0);
  });
});

describe("source commits must be clean (a -dirty commit cannot be reproduced)", () => {
  const dirtyCorpus = (dir) =>
    reseal(dir, (m) => {
      for (const s of m.sources) if (s.id === "gradient-papers" || s.id === "agentile") s.commit = `${"a".repeat(40)}-dirty`;
    });

  it("accepts the fixture, whose included sources all record clean commits", () => {
    expect(checkSourceCommits(verifyCorpus(corpus).manifest)).toEqual([]);
  });

  it("refuses an included source recorded at a -dirty commit and names every one", () => {
    dirtyCorpus(corpus);
    const { manifest } = verifyCorpus(corpus);
    expect(() => checkSourceCommits(manifest)).toThrow(/gradient-papers @ a{40}-dirty, agentile @ a{40}-dirty: built from a work tree with local changes/);
  });

  it("returns the dirty sources instead of throwing under allowDirty", () => {
    dirtyCorpus(corpus);
    const { manifest } = verifyCorpus(corpus);
    expect(checkSourceCommits(manifest, { allowDirty: true }).map((d) => d.id)).toEqual(["gradient-papers", "agentile"]);
  });

  it("ignores excluded sources, which ship no text", () => {
    reseal(corpus, (m) => {
      for (const s of m.sources) if (s.included === false) s.commit = `${"b".repeat(40)}-dirty`;
    });
    expect(checkSourceCommits(verifyCorpus(corpus).manifest)).toEqual([]);
  });

  it("CLI: refuses a dirty corpus with exit 1 and stages it under --allow-dirty with a warning", () => {
    dirtyCorpus(corpus);
    const dest = path.join(tmp, "dest");
    const refused = run(corpus, "--dest", dest, "--bge-dir", fakeBge().dir, "--allow-unembedded");
    expect(refused.status).toBe(1);
    expect(refused.stderr).toMatch(/knowledge corpus refused: .*-dirty.*--allow-dirty/);
    expect(fs.existsSync(path.join(dest, "manifest.json"))).toBe(false);

    const allowed = run(corpus, "--dest", dest, "--bge-dir", fakeBge().dir, "--allow-unembedded", "--allow-dirty");
    expect(allowed.status, allowed.stderr).toBe(0);
    expect(allowed.stderr).toMatch(/warning: source gradient-papers was built from a work tree with local changes/);
    expect(allowed.stderr).toMatch(/warning: source agentile was built/);
  });
});

describe("memMcpSupportsImport", () => {
  it("detects the import-corpus usage string in a binary", () => {
    const yes = path.join(tmp, "new");
    const no = path.join(tmp, "old");
    fs.writeFileSync(yes, Buffer.concat([Buffer.alloc(64, 0), Buffer.from("usage: mem-mcp import-corpus <store-path> <corpus-dir>"), Buffer.alloc(8, 0)]));
    fs.writeFileSync(no, Buffer.from("usage: mem-mcp <store-path> <sock-path>"));
    expect(memMcpSupportsImport(yes)).toBe(true);
    expect(memMcpSupportsImport(no)).toBe(false);
  });
});

describe("stageInto", () => {
  it("keeps the README and replaces everything else", () => {
    const dest = path.join(tmp, "dest");
    fs.mkdirSync(path.join(dest, "tenants"), { recursive: true });
    fs.writeFileSync(path.join(dest, "README.md"), "kept");
    fs.writeFileSync(path.join(dest, "tenants", "stale.corpus.json"), "old");
    const { files } = verifyCorpus(corpus);
    stageInto(corpus, dest, files);
    expect(fs.readFileSync(path.join(dest, "README.md"), "utf8")).toBe("kept");
    expect(fs.existsSync(path.join(dest, "tenants", "stale.corpus.json"))).toBe(false);
    expect(verifyCorpus(dest, { ignore: ["README.md"] }).manifest.bundle_digest).toBe(FIXTURE_DIGEST);
  });
});

describe("CLI", () => {
  it("stages a directory and exits 0", () => {
    const dest = path.join(tmp, "dest");
    const bge = fakeBge();
    embedAll(corpus, bge);
    const r = run(corpus, "--dest", dest, "--bge-dir", bge.dir);
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toMatch(/staged knowledge corpus [0-9a-f]{64}/);
    expect(r.stdout).toContain("vectors for every node match the bundled bge-base-en-v1.5");
    expect(fs.existsSync(path.join(dest, "manifest.json"))).toBe(true);
  });

  it("refuses to stage without --bge-dir: the corpus is useless without the bundled embedder", () => {
    const r = run(corpus, "--dest", path.join(tmp, "dest"));
    expect(r.status).toBe(2);
    expect(r.stderr).toMatch(/--bge-dir/);
  });

  it("stages an unembedded corpus only when asked, and warns about the CPU cost", () => {
    const r = run(corpus, "--dest", path.join(tmp, "dest"), "--bge-dir", fakeBge().dir, "--allow-unembedded");
    expect(r.status, r.stderr).toBe(0);
    expect(r.stderr).toMatch(/warning: .*embed/);
  });

  it("stages a .tar.gz release asset with a top-level directory", () => {
    const tgz = path.join(tmp, "knowledge-corpus.tar.gz");
    const parent = path.join(tmp, "pack");
    fs.mkdirSync(parent);
    fs.renameSync(corpus, path.join(parent, "knowledge-corpus"));
    const t = spawnSync("tar", ["-czf", tgz, "-C", parent, "knowledge-corpus"]);
    expect(t.status).toBe(0);
    const dest = path.join(tmp, "dest");
    const r = run(tgz, "--dest", dest, "--bge-dir", fakeBge().dir, "--allow-unembedded");
    expect(r.status, r.stderr).toBe(0);
    expect(verifyCorpus(dest).manifest.bundle_digest).toBe(FIXTURE_DIGEST);
  });

  it("refuses a bad corpus with exit 1 and leaves the destination untouched", () => {
    fs.appendFileSync(path.join(corpus, tenantFile("skills")), " ");
    const dest = path.join(tmp, "dest");
    fs.mkdirSync(dest);
    fs.writeFileSync(path.join(dest, "README.md"), "kept");
    const r = run(corpus, "--dest", dest, "--bge-dir", fakeBge().dir, "--allow-unembedded");
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/knowledge corpus refused/);
    expect(fs.readdirSync(dest)).toEqual(["README.md"]);
  });

  it("refuses a mem-mcp that predates import-corpus", () => {
    const old = path.join(tmp, "mem-mcp-old");
    fs.writeFileSync(old, "usage: mem-mcp <store-path> <sock-path>");
    const r = run(corpus, "--dest", path.join(tmp, "dest"), "--mem-mcp", old, "--bge-dir", fakeBge().dir, "--allow-unembedded");
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/predates/);
  });

  it("exits 2 on usage errors", () => {
    expect(run().status).toBe(2);
    expect(run(corpus, "--dest").status).toBe(2);
    expect(run(corpus, "--surprise").status).toBe(2);
    expect(run(corpus, "--bge-dir").status).toBe(2);
  });
});
